//! Optional pacing caps (SHL-22, DRF-22). Everything is off by default; the app never throttles you
//! unless you turn it on. When on, caps count successful actions in a rolling 24 hours.

use crate::drafts::bundle::ActionKind;
use crate::error::{AppError, AppResult};
use crate::storage::repo::{get_setting, set_setting};
use crate::util::format_unix;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PacingConfig {
    pub enabled: bool,
    pub like: u32,
    pub reply: u32,
    pub repost: u32,
    pub quote: u32,
    pub bookmark: u32,
    pub follow: u32,
    /// Minimum seconds between two text publishes (reply or quote). 0 means none.
    pub min_gap_sec: u32,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self { enabled: false, like: 120, reply: 60, repost: 30, quote: 20, bookmark: 120, follow: 40, min_gap_sec: 90 }
    }
}

impl PacingConfig {
    pub fn cap(&self, a: ActionKind) -> u32 {
        match a {
            ActionKind::Like => self.like,
            ActionKind::Reply => self.reply,
            ActionKind::Repost => self.repost,
            ActionKind::Quote => self.quote,
            ActionKind::Bookmark => self.bookmark,
            ActionKind::Follow => self.follow,
        }
    }
}

const KEY: &str = "pacing_config";

pub fn get(conn: &Connection) -> AppResult<PacingConfig> {
    Ok(get_setting(conn, KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

pub fn set(conn: &Connection, cfg: &PacingConfig) -> AppResult<PacingConfig> {
    for v in [cfg.like, cfg.reply, cfg.repost, cfg.quote, cfg.bookmark, cfg.follow] {
        if v > 10_000 {
            return Err(AppError::Validation("a daily cap above 10000 is not meaningful".into()));
        }
    }
    if cfg.min_gap_sec > 3600 {
        return Err(AppError::Validation("the minimum gap can be at most 3600 seconds".into()));
    }
    set_setting(conn, KEY, &serde_json::to_string(cfg).unwrap_or_default())?;
    Ok(cfg.clone())
}

fn count_since(conn: &Connection, profile_id: &str, action: &str, since: &str) -> AppResult<u32> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM action_events WHERE profile_id = ?1 AND action = ?2 AND ok = 1 AND created_at >= ?3",
        params![profile_id, action, since],
        |r| r.get::<_, u32>(0),
    )?)
}

/// Successful text publishes since `since`: replies, quotes and original (OGC) posts.
fn recent_text_posts(conn: &Connection, profile_id: &str, since: &str) -> AppResult<u32> {
    let ogc: u32 = conn.query_row(
        "SELECT COUNT(*) FROM publish_audit_log WHERE profile_id = ?1 AND target_kind = 'ogc' AND success = 1 AND created_at >= ?2",
        params![profile_id, since],
        |r| r.get(0),
    )?;
    Ok(count_since(conn, profile_id, "reply", since)? + count_since(conn, profile_id, "quote", since)? + ogc)
}

/// `Ok` when an original (OGC) post may be published now. It is a text publish, so the minimum gap applies;
/// there is no per-day cap for it in [`PacingConfig`]. With pacing off this is always `Ok`.
pub fn check_original_post(conn: &Connection, profile_id: &str, now_unix: i64) -> AppResult<Result<(), String>> {
    let cfg = get(conn)?;
    if !cfg.enabled || cfg.min_gap_sec == 0 {
        return Ok(Ok(()));
    }
    let since = format_unix(now_unix - cfg.min_gap_sec as i64);
    if recent_text_posts(conn, profile_id, &since)? > 0 {
        return Ok(Err(format!("Pacing: wait {} seconds between posts.", cfg.min_gap_sec)));
    }
    Ok(Ok(()))
}

/// `Ok` when the action may run now. With pacing off this is always `Ok`.
pub fn check(conn: &Connection, profile_id: &str, action: ActionKind, now_unix: i64) -> AppResult<Result<(), String>> {
    let cfg = get(conn)?;
    if !cfg.enabled {
        return Ok(Ok(()));
    }
    let used = count_since(conn, profile_id, action.as_str(), &format_unix(now_unix - 86_400))?;
    let cap = cfg.cap(action);
    if cap > 0 && used >= cap {
        return Ok(Err(format!("Daily limit reached: {used} of {cap} {}s in the last 24 hours. Raise or turn off pacing in Settings.", action.as_str())));
    }
    if action.is_text() && cfg.min_gap_sec > 0 {
        let since = format_unix(now_unix - cfg.min_gap_sec as i64);
        if recent_text_posts(conn, profile_id, &since)? > 0 {
            return Ok(Err(format!("Pacing: wait {} seconds between posts.", cfg.min_gap_sec)));
        }
    }
    Ok(Ok(()))
}

#[derive(Debug, Clone, Serialize)]
pub struct PacingUsage {
    pub action: String,
    pub used_24h: u32,
    pub cap: u32,
}

pub fn usage(conn: &Connection, profile_id: &str, now_unix: i64) -> AppResult<Vec<PacingUsage>> {
    let cfg = get(conn)?;
    let since = format_unix(now_unix - 86_400);
    crate::drafts::bundle::EXEC_ORDER
        .iter()
        .map(|a| Ok(PacingUsage { action: a.as_str().into(), used_24h: count_since(conn, profile_id, a.as_str(), &since)?, cap: cfg.cap(*a) }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::repo::testutil::db;
    use crate::util::{new_id, now_rfc3339};

    fn event(conn: &Connection, profile: &str, action: &str, ok: bool) {
        conn.execute(
            "INSERT INTO action_events (id, profile_id, platform_post_id, action, source, ok, created_at) VALUES (?1, ?2, 'p', ?3, 'publish', ?4, ?5)",
            params![new_id(), profile, action, ok as i64, now_rfc3339()],
        )
        .unwrap();
    }

    fn now() -> i64 {
        crate::scout::model::parse_rfc3339_unix(&now_rfc3339()).unwrap()
    }

    #[test]
    fn off_by_default_never_blocks() {
        let (conn, profile) = db();
        assert!(!get(&conn).unwrap().enabled);
        for _ in 0..500 {
            event(&conn, &profile, "like", true);
        }
        assert_eq!(check(&conn, &profile, ActionKind::Like, now()).unwrap(), Ok(()));
    }

    #[test]
    fn enabled_caps_count_successes_in_the_last_day() {
        let (conn, profile) = db();
        set(&conn, &PacingConfig { enabled: true, like: 3, min_gap_sec: 0, ..Default::default() }).unwrap();
        event(&conn, &profile, "like", true);
        event(&conn, &profile, "like", true);
        event(&conn, &profile, "like", false);
        assert_eq!(check(&conn, &profile, ActionKind::Like, now()).unwrap(), Ok(()));
        event(&conn, &profile, "like", true);
        assert!(check(&conn, &profile, ActionKind::Like, now()).unwrap().unwrap_err().contains("3 of 3"));
        assert_eq!(check(&conn, &profile, ActionKind::Like, now() + 2 * 86_400).unwrap(), Ok(()), "old events fall out of the window");
        assert_eq!(check(&conn, &profile, ActionKind::Follow, now()).unwrap(), Ok(()));
    }

    #[test]
    fn zero_cap_means_unlimited_and_min_gap_applies_to_text_only() {
        let (conn, profile) = db();
        set(&conn, &PacingConfig { enabled: true, like: 0, min_gap_sec: 120, ..Default::default() }).unwrap();
        for _ in 0..50 {
            event(&conn, &profile, "like", true);
        }
        assert_eq!(check(&conn, &profile, ActionKind::Like, now()).unwrap(), Ok(()));
        event(&conn, &profile, "reply", true);
        assert!(check(&conn, &profile, ActionKind::Quote, now()).unwrap().is_err());
        assert_eq!(check(&conn, &profile, ActionKind::Bookmark, now()).unwrap(), Ok(()));
    }

    #[test]
    fn validates_ranges_and_usage_lists_all_six() {
        let (conn, profile) = db();
        assert!(set(&conn, &PacingConfig { like: 20_000, ..Default::default() }).is_err());
        assert!(set(&conn, &PacingConfig { min_gap_sec: 99_999, ..Default::default() }).is_err());
        assert_eq!(usage(&conn, &profile, now()).unwrap().len(), 6);
    }

    #[test]
    fn original_posts_respect_the_min_gap_and_count_toward_it() {
        let (conn, profile) = db();
        assert_eq!(check_original_post(&conn, &profile, now()).unwrap(), Ok(()), "off by default");
        set(&conn, &PacingConfig { enabled: true, min_gap_sec: 120, ..Default::default() }).unwrap();
        assert_eq!(check_original_post(&conn, &profile, now()).unwrap(), Ok(()));
        event(&conn, &profile, "reply", true);
        assert!(check_original_post(&conn, &profile, now()).unwrap().unwrap_err().contains("120 seconds"));
        assert_eq!(check_original_post(&conn, &profile, now() + 3600).unwrap(), Ok(()));

        let (conn, profile) = db();
        set(&conn, &PacingConfig { enabled: true, min_gap_sec: 120, ..Default::default() }).unwrap();
        crate::creator::drafts::write_audit(&conn, &profile, "d1", "ref", true, None, None, None).unwrap();
        assert!(check_original_post(&conn, &profile, now()).unwrap().is_err(), "an OGC post blocks the next one");
        assert!(check(&conn, &profile, ActionKind::Reply, now()).unwrap().is_err(), "and blocks a reply");
        assert_eq!(check(&conn, &profile, ActionKind::Like, now()).unwrap(), Ok(()));
    }
}
