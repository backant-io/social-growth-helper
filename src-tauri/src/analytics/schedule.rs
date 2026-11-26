//! Time-series snapshot checkpoint scheduler (ANL-05).
//!
//! Target checkpoints: 1h, 6h, 24h, 72h, 7d (168h) after publish.
//! Missed windows when the app was closed are flagged for backfill on the next sync.

use crate::error::AppResult;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CHECKPOINT_HOURS: &[f64] = &[1.0, 6.0, 24.0, 72.0, 168.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointState {
    Captured,
    DueNow,
    MissedBackfill,
    Upcoming,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CheckpointStatus {
    pub label: String,
    pub target_hours: f64,
    pub state: CheckpointState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrackedPostSchedule {
    pub platform_post_id: String,
    pub draft_kind: String,
    pub draft_id: String,
    pub published_url: Option<String>,
    pub published_at: String,
    pub age_hours: f64,
    pub captured_count: usize,
    pub needs_sync: bool,
    pub missed_backfill_count: usize,
    pub checkpoints: Vec<CheckpointStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleSummary {
    pub tracked_posts: usize,
    pub posts_due_now: usize,
    pub missed_windows: usize,
    pub items: Vec<TrackedPostSchedule>,
}

pub fn checkpoint_label(hours: f64) -> &'static str {
    match hours as u64 {
        1 => "1h",
        6 => "6h",
        24 => "24h",
        72 => "72h",
        168 => "7d",
        _ => "custom",
    }
}

/// Evaluates all 5 target checkpoints for a post given its current age and previously captured snapshot ages.
pub fn evaluate_checkpoints(age_hours: f64, captured_ages: &[f64]) -> Vec<CheckpointStatus> {
    CHECKPOINT_HOURS
        .iter()
        .enumerate()
        .map(|(idx, &target)| {
            let tolerance = (target * 0.35).max(0.5);
            let next_target = CHECKPOINT_HOURS.get(idx + 1).copied().unwrap_or(target * 2.5);
            let captured = captured_ages
                .iter()
                .any(|&a| (a - target).abs() <= tolerance || (a >= target && a < next_target));
            let state = if captured {
                CheckpointState::Captured
            } else if age_hours < target {
                CheckpointState::Upcoming
            } else if age_hours <= next_target {
                CheckpointState::DueNow
            } else {
                CheckpointState::MissedBackfill
            };
            CheckpointStatus {
                label: checkpoint_label(target).to_string(),
                target_hours: target,
                state,
            }
        })
        .collect()
}

/// Parses an RFC 3339 UTC timestamp (`YYYY-MM-DDTHH:MM:SSZ`, optional fractional seconds) to Unix seconds.
pub fn parse_rfc3339_unix(ts: &str) -> Option<i64> {
    let s = ts.trim();
    if s.len() < 19 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let hh: i64 = s.get(11..13)?.parse().ok()?;
    let mm: i64 = s.get(14..16)?.parse().ok()?;
    let ss: i64 = s.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

// Inverse of Howard Hinnant's civil_from_days.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = y - if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

pub fn hours_since(published_at: &str, now_unix: i64) -> f64 {
    match parse_rfc3339_unix(published_at) {
        Some(pub_unix) => ((now_unix - pub_unix).max(0) as f64) / 3600.0,
        None => 0.0,
    }
}

pub fn now_unix_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Computes the snapshot schedule for all published replies and OGC posts for a profile.
pub fn compute_schedule(conn: &Connection, profile_id: &str, now_unix: i64) -> AppResult<ScheduleSummary> {
    let mut tracked: Vec<(String, String, String, Option<String>, String)> = Vec::new();

    {
        let mut stmt = conn.prepare(
            "SELECT published_post_id, COALESCE(draft_kind, 'reply'), id, published_url, COALESCE(published_at, created_at)
             FROM reply_drafts
             WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([profile_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        tracked.extend(rows);
    }

    {
        let mut stmt = conn.prepare(
            "SELECT published_post_id, 'ogc', id, published_url, COALESCE(published_at, created_at)
             FROM ogc_drafts
             WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NOT NULL",
        )?;
        let rows = stmt
            .query_map([profile_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        tracked.extend(rows);
    }

    let mut items = Vec::with_capacity(tracked.len());
    let mut posts_due_now = 0usize;
    let mut missed_windows = 0usize;

    for (post_id, kind, draft_id, url, published_at) in tracked {
        let mut snap_stmt = conn.prepare(
            "SELECT age_hours FROM analytics_snapshots
             WHERE profile_id = ?1 AND scope = 'post' AND platform_post_id = ?2 AND age_hours IS NOT NULL",
        )?;
        let ages: Vec<f64> = snap_stmt
            .query_map(params![profile_id, post_id], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        let age = hours_since(&published_at, now_unix);
        let checkpoints = evaluate_checkpoints(age, &ages);
        let missed = checkpoints
            .iter()
            .filter(|c| c.state == CheckpointState::MissedBackfill)
            .count();
        let due = checkpoints
            .iter()
            .any(|c| matches!(c.state, CheckpointState::DueNow | CheckpointState::MissedBackfill))
            || ages.is_empty();

        if due {
            posts_due_now += 1;
        }
        missed_windows += missed;

        items.push(TrackedPostSchedule {
            platform_post_id: post_id,
            draft_kind: kind,
            draft_id,
            published_url: url,
            published_at,
            age_hours: (age * 10.0).round() / 10.0,
            captured_count: ages.len(),
            needs_sync: due,
            missed_backfill_count: missed,
            checkpoints,
        });
    }

    // Prioritize posts that need sync and have missed windows first.
    items.sort_by(|a, b| {
        b.needs_sync
            .cmp(&a.needs_sync)
            .then(b.missed_backfill_count.cmp(&a.missed_backfill_count))
            .then(b.published_at.cmp(&a.published_at))
    });

    Ok(ScheduleSummary {
        tracked_posts: items.len(),
        posts_due_now,
        missed_windows,
        items,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::format_unix;

    #[test]
    fn rfc3339_parser_round_trips_known_instants() {
        for ts in [0, 951_782_400, 1_700_000_000, 1_795_680_000] {
            let s = format_unix(ts);
            assert_eq!(parse_rfc3339_unix(&s), Some(ts));
        }
    }

    #[test]
    fn evaluates_due_captured_missed_and_upcoming_checkpoints() {
        // Post is 30 hours old, only 1h snapshot captured -> 6h missed, 24h due now, 72h & 7d upcoming.
        let cps = evaluate_checkpoints(30.0, &[1.1]);
        assert_eq!(cps[0].state, CheckpointState::Captured); // 1h
        assert_eq!(cps[1].state, CheckpointState::MissedBackfill); // 6h
        assert_eq!(cps[2].state, CheckpointState::DueNow); // 24h
        assert_eq!(cps[3].state, CheckpointState::Upcoming); // 72h
        assert_eq!(cps[4].state, CheckpointState::Upcoming); // 7d
    }
}
