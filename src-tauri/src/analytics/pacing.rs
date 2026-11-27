//! Pacing, budgets and per-post state for the background analytics worker.
//!
//! One unit is one page load on the invisible publish browser. An AI self-heal costs four. Budgets are counted
//! from `analytics_budget_log`, so a restart never resets them. Everything here is plain data in, data out so the
//! worker's behaviour can be tested without a browser.

use super::schedule::CHECKPOINT_HOURS;
use crate::error::AppResult;
use crate::storage::repo::{get_setting, set_setting};
use rusqlite::{params, Connection, OptionalExtension};

pub const HOURLY_UNIT_CAP: i64 = 8;
pub const DAILY_UNIT_CAP: i64 = 40;
pub const DOM_UNITS: i64 = 1;
pub const AI_HEAL_UNITS: i64 = 4;

pub const AI_PER_HOUR: i64 = 1;
pub const AI_PER_DAY: i64 = 3;
pub const AI_COOLDOWN_SECS: i64 = 15 * 60;
pub const AI_POST_COOLDOWN_SECS: i64 = 24 * 3600;
pub const AI_BREAKER_FAILS: usize = 2;
pub const AI_BREAKER_SECS: i64 = 6 * 3600;

pub const JITTER_MIN_SECS: u64 = 90;
pub const JITTER_MAX_SECS: u64 = 180;
pub const IDLE_POLL_SECS: u64 = 300;
pub const THROTTLED_POLL_SECS: u64 = 600;
pub const PAUSED_POLL_SECS: u64 = 900;
pub const BLOCKED_POLL_SECS: u64 = 1800;

pub const BACKOFF_SECS: [i64; 4] = [15 * 60, 3600, 4 * 3600, 24 * 3600];
pub const TICK_FAILS_BEFORE_PAUSE: i64 = 3;
pub const TICK_FAIL_PAUSE_SECS: i64 = 6 * 3600;

pub const SWEEP_INTERVAL_SECS: i64 = 5 * 3600;
pub const MAX_TRACKED_AGE_HOURS: f64 = 14.0 * 24.0;
pub const MISSING_BEFORE_RETIRE: i64 = 3;
const MIN_RECHECK_SECS: i64 = 30 * 60;

const PAUSE_UNTIL_KEY: &str = "analytics_sync_pause_until";
const TICK_FAILS_KEY: &str = "analytics_sync_tick_fails";

/// Randomised spacing between two page loads (90 to 180 seconds). `entropy` is any varying number.
pub fn jitter_secs(entropy: u64) -> u64 {
    JITTER_MIN_SECS + entropy % (JITTER_MAX_SECS - JITTER_MIN_SECS + 1)
}

/// Retry delay after the Nth consecutive failure of one post: 15 min, 1 h, 4 h, then 24 h.
pub fn backoff_secs(fail_count: i64) -> i64 {
    BACKOFF_SECS[(fail_count - 1).clamp(0, BACKOFF_SECS.len() as i64 - 1) as usize]
}

/// When a post should next be looked at: the next checkpoint (1h, 6h, 24h, 72h, 7d) after its current age,
/// never sooner than 30 minutes. `None` once the 7d checkpoint has passed, which retires the post.
pub fn next_checkpoint_due(age_hours: f64, now: i64) -> Option<i64> {
    let target = CHECKPOINT_HOURS.iter().copied().find(|t| *t > age_hours + 0.01)?;
    Some(now + (((target - age_hours) * 3600.0) as i64).max(MIN_RECHECK_SECS))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitKind {
    Dom,
    AiOk,
    AiFail,
}

impl UnitKind {
    fn as_str(self) -> &'static str {
        match self {
            UnitKind::Dom => "dom",
            UnitKind::AiOk => "ai_ok",
            UnitKind::AiFail => "ai_fail",
        }
    }
}

pub fn record_units(conn: &Connection, profile_id: &str, now: i64, units: i64, kind: UnitKind) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_budget_log (profile_id, at, units, kind) VALUES (?1, ?2, ?3, ?4)",
        params![profile_id, now, units, kind.as_str()],
    )?;
    Ok(())
}

fn units_since(conn: &Connection, profile_id: &str, since: i64) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(units), 0) FROM analytics_budget_log WHERE profile_id = ?1 AND at >= ?2",
        params![profile_id, since],
        |r| r.get(0),
    )?)
}

/// True when `units` more would stay inside both the hourly (8) and daily (40) caps.
pub fn can_spend(conn: &Connection, profile_id: &str, now: i64, units: i64) -> AppResult<bool> {
    Ok(units_since(conn, profile_id, now - 3600)? + units <= HOURLY_UNIT_CAP
        && units_since(conn, profile_id, now - 86_400)? + units <= DAILY_UNIT_CAP)
}

fn ai_attempts_since(conn: &Connection, profile_id: &str, since: i64) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM analytics_budget_log WHERE profile_id = ?1 AND at >= ?2 AND kind IN ('ai_ok', 'ai_fail')",
        params![profile_id, since],
        |r| r.get(0),
    )?)
}

/// Whether one AI self-heal may run now: inside the shared unit budget, at most 1 per hour and 3 per day,
/// 15 minutes apart, once per post per day, and not while the two-failure circuit breaker is open.
pub fn can_ai_heal(conn: &Connection, profile_id: &str, now: i64, post_last_heal: Option<i64>) -> AppResult<bool> {
    if post_last_heal.is_some_and(|t| now - t < AI_POST_COOLDOWN_SECS) {
        return Ok(false);
    }
    if ai_attempts_since(conn, profile_id, now - 3600)? >= AI_PER_HOUR || ai_attempts_since(conn, profile_id, now - 86_400)? >= AI_PER_DAY {
        return Ok(false);
    }
    let mut stmt = conn.prepare(
        "SELECT at, kind FROM analytics_budget_log WHERE profile_id = ?1 AND kind IN ('ai_ok', 'ai_fail') ORDER BY at DESC, id DESC LIMIT ?2",
    )?;
    let recent: Vec<(i64, String)> = stmt
        .query_map(params![profile_id, AI_BREAKER_FAILS as i64], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if let Some((last_at, _)) = recent.first() {
        if now - last_at < AI_COOLDOWN_SECS {
            return Ok(false);
        }
        if recent.len() == AI_BREAKER_FAILS && recent.iter().all(|(_, k)| k == "ai_fail") && now - last_at < AI_BREAKER_SECS {
            return Ok(false);
        }
    }
    can_spend(conn, profile_id, now, AI_HEAL_UNITS)
}

pub fn pause_until(conn: &Connection) -> Option<i64> {
    get_setting(conn, PAUSE_UNTIL_KEY).ok().flatten().and_then(|v| v.parse().ok())
}

pub fn set_pause(conn: &Connection, until: i64) -> AppResult<()> {
    set_setting(conn, PAUSE_UNTIL_KEY, &until.to_string())
}

/// Records whether a tick that touched the browser worked. Three failures in a row pause the worker for six
/// hours; returns true when that pause was just started. Quiet blocks (logged out) never count.
pub fn note_tick_result(conn: &Connection, ok: bool, now: i64) -> AppResult<bool> {
    if ok {
        set_setting(conn, TICK_FAILS_KEY, "0")?;
        return Ok(false);
    }
    let fails: i64 = get_setting(conn, TICK_FAILS_KEY)?.and_then(|v| v.parse().ok()).unwrap_or(0) + 1;
    if fails >= TICK_FAILS_BEFORE_PAUSE {
        set_setting(conn, TICK_FAILS_KEY, "0")?;
        set_pause(conn, now + TICK_FAIL_PAUSE_SECS)?;
        return Ok(true);
    }
    set_setting(conn, TICK_FAILS_KEY, &fails.to_string())?;
    Ok(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostState {
    pub next_due_at: i64,
    pub fail_count: i64,
    pub missing_count: i64,
    pub last_heal_at: Option<i64>,
    pub retired: bool,
}

pub fn load_state(conn: &Connection, profile_id: &str, post_id: &str) -> AppResult<Option<PostState>> {
    Ok(conn
        .query_row(
            "SELECT next_due_at, fail_count, missing_count, last_heal_at, retired FROM analytics_sync_state
             WHERE profile_id = ?1 AND platform_post_id = ?2",
            params![profile_id, post_id],
            |r| {
                Ok(PostState {
                    next_due_at: r.get(0)?,
                    fail_count: r.get(1)?,
                    missing_count: r.get(2)?,
                    last_heal_at: r.get(3)?,
                    retired: r.get::<_, i64>(4)? != 0,
                })
            },
        )
        .optional()?)
}

pub fn is_due(state: Option<&PostState>, now: i64) -> bool {
    match state {
        None => true,
        Some(s) => !s.retired && s.next_due_at <= now,
    }
}

fn upsert(conn: &Connection, profile_id: &str, post_id: &str, now: i64, next_due: i64, fail: i64, missing: i64, retired: bool, synced: bool) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_sync_state (profile_id, platform_post_id, next_due_at, last_synced_at, last_attempt_at, fail_count, missing_count, retired)
         VALUES (?1, ?2, ?3, CASE WHEN ?8 THEN ?4 END, ?4, ?5, ?6, ?7)
         ON CONFLICT(profile_id, platform_post_id) DO UPDATE SET
           next_due_at = excluded.next_due_at,
           last_synced_at = CASE WHEN ?8 THEN ?4 ELSE last_synced_at END,
           last_attempt_at = ?4,
           fail_count = excluded.fail_count,
           missing_count = excluded.missing_count,
           retired = excluded.retired",
        params![profile_id, post_id, next_due, now, fail, missing, retired as i64, synced],
    )?;
    Ok(())
}

/// A snapshot was stored: schedule the next checkpoint, or retire the post when none are left.
pub fn mark_synced(conn: &Connection, profile_id: &str, post_id: &str, age_hours: f64, now: i64) -> AppResult<()> {
    let next = next_checkpoint_due(age_hours, now);
    upsert(conn, profile_id, post_id, now, next.unwrap_or(i64::MAX), 0, 0, next.is_none(), true)
}

/// A load or extraction failed: retry later with backoff.
pub fn mark_failed(conn: &Connection, profile_id: &str, post_id: &str, now: i64) -> AppResult<()> {
    let prev = load_state(conn, profile_id, post_id)?;
    let fails = prev.as_ref().map_or(0, |s| s.fail_count) + 1;
    let missing = prev.as_ref().map_or(0, |s| s.missing_count);
    upsert(conn, profile_id, post_id, now, now + backoff_secs(fails), fails, missing, false, false)
}

/// The post page said the post no longer exists. Three of these retire it.
pub fn mark_missing(conn: &Connection, profile_id: &str, post_id: &str, now: i64) -> AppResult<()> {
    let prev = load_state(conn, profile_id, post_id)?;
    let missing = prev.as_ref().map_or(0, |s| s.missing_count) + 1;
    let fails = prev.as_ref().map_or(0, |s| s.fail_count);
    let retired = missing >= MISSING_BEFORE_RETIRE;
    upsert(conn, profile_id, post_id, now, if retired { i64::MAX } else { now + backoff_secs(missing) }, fails, missing, retired, false)
}

pub fn retire(conn: &Connection, profile_id: &str, post_id: &str, now: i64) -> AppResult<()> {
    let prev = load_state(conn, profile_id, post_id)?;
    upsert(conn, profile_id, post_id, now, i64::MAX, prev.as_ref().map_or(0, |s| s.fail_count), prev.as_ref().map_or(0, |s| s.missing_count), true, false)
}

pub fn mark_healed(conn: &Connection, profile_id: &str, post_id: &str, now: i64) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_sync_state (profile_id, platform_post_id, last_heal_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(profile_id, platform_post_id) DO UPDATE SET last_heal_at = ?3",
        params![profile_id, post_id, now],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{open, repo};
    use tempfile::tempdir;

    fn db() -> (tempfile::TempDir, Connection, String) {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        (dir, conn, pid)
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn jitter_stays_between_90_and_180_seconds() {
        for e in [0u64, 1, 89, 90, 91, 12345, u64::MAX] {
            let j = jitter_secs(e);
            assert!((90..=180).contains(&j), "{j}");
        }
        assert_eq!(jitter_secs(0), 90);
        assert_eq!(jitter_secs(90), 180);
    }

    #[test]
    fn backoff_climbs_then_holds_at_a_day() {
        assert_eq!([1, 2, 3, 4, 5, 50].map(backoff_secs), [900, 3600, 14_400, 86_400, 86_400, 86_400]);
        assert_eq!(backoff_secs(0), 900);
    }

    #[test]
    fn next_checkpoint_follows_post_age_and_retires_after_seven_days() {
        assert_eq!(next_checkpoint_due(0.2, NOW), Some(NOW + 2880), "1h checkpoint is 0.8h away");
        assert_eq!(next_checkpoint_due(0.9, NOW), Some(NOW + 1800), "never sooner than 30 minutes");
        assert_eq!(next_checkpoint_due(2.0, NOW), Some(NOW + 4 * 3600), "6h checkpoint is 4h away");
        assert_eq!(next_checkpoint_due(30.0, NOW), Some(NOW + 42 * 3600));
        assert_eq!(next_checkpoint_due(100.0, NOW), Some(NOW + 68 * 3600));
        assert_eq!(next_checkpoint_due(168.5, NOW), None);
    }

    #[test]
    fn hourly_cap_is_eight_units_and_frees_up_after_an_hour() {
        let (_d, conn, pid) = db();
        for i in 0..8 {
            assert!(can_spend(&conn, &pid, NOW + i, DOM_UNITS).unwrap());
            record_units(&conn, &pid, NOW + i, DOM_UNITS, UnitKind::Dom).unwrap();
        }
        assert!(!can_spend(&conn, &pid, NOW + 60, DOM_UNITS).unwrap());
        assert!(can_spend(&conn, &pid, NOW + 3601 + 8, DOM_UNITS).unwrap(), "the window rolls forward");
    }

    #[test]
    fn daily_cap_is_forty_units_even_when_each_hour_has_room() {
        let (_d, conn, pid) = db();
        for i in 0..40 {
            record_units(&conn, &pid, NOW + i * 1800, 1, UnitKind::Dom).unwrap();
        }
        let later = NOW + 39 * 1800 + 60;
        assert!(!can_spend(&conn, &pid, later, DOM_UNITS).unwrap());
        assert!(can_spend(&conn, &pid, NOW + 86_400 + 1800 * 40, DOM_UNITS).unwrap());
    }

    #[test]
    fn an_ai_heal_costs_four_units_of_the_shared_budget() {
        let (_d, conn, pid) = db();
        for i in 0..5 {
            record_units(&conn, &pid, NOW + i, 1, UnitKind::Dom).unwrap();
        }
        assert!(!can_ai_heal(&conn, &pid, NOW + 10, None).unwrap(), "5 + 4 would pass the hourly 8");
        assert!(can_ai_heal(&conn, &pid, NOW + 4000, None).unwrap());
    }

    #[test]
    fn ai_heals_are_capped_per_hour_day_and_post() {
        let (_d, conn, pid) = db();
        assert!(can_ai_heal(&conn, &pid, NOW, None).unwrap());
        assert!(!can_ai_heal(&conn, &pid, NOW, Some(NOW - 3600)).unwrap(), "one heal per post per day");
        assert!(can_ai_heal(&conn, &pid, NOW, Some(NOW - 90_000)).unwrap());

        record_units(&conn, &pid, NOW, AI_HEAL_UNITS, UnitKind::AiOk).unwrap();
        assert!(!can_ai_heal(&conn, &pid, NOW + 1800, None).unwrap(), "1 per hour");
        assert!(can_ai_heal(&conn, &pid, NOW + 3700, None).unwrap());

        record_units(&conn, &pid, NOW + 3700, AI_HEAL_UNITS, UnitKind::AiOk).unwrap();
        record_units(&conn, &pid, NOW + 7500, AI_HEAL_UNITS, UnitKind::AiOk).unwrap();
        assert!(!can_ai_heal(&conn, &pid, NOW + 11_000, None).unwrap(), "3 per day");
        assert!(can_ai_heal(&conn, &pid, NOW + 90_000, None).unwrap());
    }

    #[test]
    fn two_failed_heals_open_a_six_hour_breaker() {
        let (_d, conn, pid) = db();
        record_units(&conn, &pid, NOW, AI_HEAL_UNITS, UnitKind::AiFail).unwrap();
        record_units(&conn, &pid, NOW + 4000, AI_HEAL_UNITS, UnitKind::AiFail).unwrap();
        assert!(!can_ai_heal(&conn, &pid, NOW + 4000 + 3700 * 2, None).unwrap(), "inside 6h of the second failure");
        assert!(can_ai_heal(&conn, &pid, NOW + 4000 + AI_BREAKER_SECS + 1, None).unwrap());
    }

    #[test]
    fn a_success_between_failures_keeps_the_breaker_closed() {
        let (_d, conn, pid) = db();
        record_units(&conn, &pid, NOW, AI_HEAL_UNITS, UnitKind::AiFail).unwrap();
        record_units(&conn, &pid, NOW + 4000, AI_HEAL_UNITS, UnitKind::AiOk).unwrap();
        assert!(can_ai_heal(&conn, &pid, NOW + 8000, None).unwrap());
    }

    #[test]
    fn three_failed_ticks_pause_the_worker_for_six_hours() {
        let (_d, conn, pid) = db();
        let _ = pid;
        assert!(!note_tick_result(&conn, false, NOW).unwrap());
        assert!(!note_tick_result(&conn, false, NOW + 100).unwrap());
        assert_eq!(pause_until(&conn), None);
        assert!(note_tick_result(&conn, false, NOW + 200).unwrap());
        assert_eq!(pause_until(&conn), Some(NOW + 200 + TICK_FAIL_PAUSE_SECS));
    }

    #[test]
    fn a_good_tick_resets_the_failure_streak() {
        let (_d, conn, _pid) = db();
        note_tick_result(&conn, false, NOW).unwrap();
        note_tick_result(&conn, false, NOW).unwrap();
        note_tick_result(&conn, true, NOW).unwrap();
        assert!(!note_tick_result(&conn, false, NOW).unwrap());
        assert!(!note_tick_result(&conn, false, NOW).unwrap());
        assert_eq!(pause_until(&conn), None);
    }

    #[test]
    fn post_state_tracks_success_backoff_missing_and_retirement() {
        let (_d, conn, pid) = db();
        assert!(is_due(load_state(&conn, &pid, "1").unwrap().as_ref(), NOW), "unknown posts are due");

        mark_failed(&conn, &pid, "1", NOW).unwrap();
        let s = load_state(&conn, &pid, "1").unwrap().unwrap();
        assert_eq!((s.fail_count, s.next_due_at), (1, NOW + 900));
        mark_failed(&conn, &pid, "1", NOW + 900).unwrap();
        assert_eq!(load_state(&conn, &pid, "1").unwrap().unwrap().next_due_at, NOW + 900 + 3600);

        mark_synced(&conn, &pid, "1", 2.0, NOW + 5000).unwrap();
        let s = load_state(&conn, &pid, "1").unwrap().unwrap();
        assert_eq!((s.fail_count, s.retired, s.next_due_at), (0, false, NOW + 5000 + 4 * 3600));
        assert!(!is_due(Some(&s), NOW + 6000));
        assert!(is_due(Some(&s), NOW + 5000 + 4 * 3600));

        mark_synced(&conn, &pid, "1", 170.0, NOW + 9000).unwrap();
        assert!(load_state(&conn, &pid, "1").unwrap().unwrap().retired, "7d captured retires the post");
        assert!(!is_due(load_state(&conn, &pid, "1").unwrap().as_ref(), i64::MAX - 1));

        for i in 0..3 {
            mark_missing(&conn, &pid, "gone", NOW + i).unwrap();
        }
        let s = load_state(&conn, &pid, "gone").unwrap().unwrap();
        assert!(s.retired && s.missing_count == 3, "three deleted/404 results retire a post");
    }

    #[test]
    fn healing_a_post_is_remembered_without_changing_its_schedule() {
        let (_d, conn, pid) = db();
        mark_failed(&conn, &pid, "7", NOW).unwrap();
        mark_healed(&conn, &pid, "7", NOW + 5).unwrap();
        let s = load_state(&conn, &pid, "7").unwrap().unwrap();
        assert_eq!((s.last_heal_at, s.fail_count), (Some(NOW + 5), 1));
    }
}
