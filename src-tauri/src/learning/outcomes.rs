//! Joins published drafts with their analytics and normalizes results (LRN-01).
//!
//! Raw impressions depend heavily on who you replied to and how your account is doing, so each result is
//! divided by a mild reach factor for the target author and then by your own median. A score of 1.0 means
//! "typical for you"; 2.0 means twice your usual after adjusting for author reach.

use crate::error::AppResult;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

/// Snapshots at least this old are treated as mature (the 24h snapshot is the main outcome).
pub const MATURE_AGE_HOURS: f64 = 24.0;
/// A personal baseline needs at least this many mature results; below it nothing is normalized.
pub const MIN_BASELINE_ITEMS: usize = 3;
const MAX_NORMALIZED: f64 = 10.0;

#[derive(Debug, Clone, Serialize)]
pub struct OutcomeItem {
    pub draft_id: String,
    pub evaluation_id: Option<String>,
    pub model_used: String,
    pub initial_text: String,
    pub final_text: String,
    pub edit_distance: i64,
    pub author_handle: String,
    pub author_followers: Option<i64>,
    pub target_text: String,
    pub has_media: bool,
    pub potential_score: Option<f64>,
    pub published_at: Option<String>,
    pub reply_age_minutes: Option<f64>,
    pub impressions: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub snapshot_age_hours: Option<f64>,
    pub mature: bool,
    pub normalized: Option<f64>,
}

/// Mild, log-shaped reach factor: 1.0 at 1k followers, ~1.3 at 10k, ~1.7 at 100k. Unknown reach is neutral.
pub fn reach_factor(followers: Option<i64>) -> f64 {
    match followers {
        Some(f) if f >= 0 => ((f as f64 + 10.0).log10() / 3.0).max(0.4),
        _ => 1.0,
    }
}

pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    Some(if n % 2 == 1 { values[n / 2] } else { (values[n / 2 - 1] + values[n / 2]) / 2.0 })
}

pub fn parse_ts(s: &str) -> Option<i64> {
    // YYYY-MM-DDTHH:MM:SSZ
    let b = s.as_bytes();
    if b.len() < 19 {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d, hh, mm, ss) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let (y2, m2) = if m <= 2 { (y - 1, m + 9) } else { (y, m - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2.rem_euclid(400);
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hh * 3600 + mm * 60 + ss)
}

pub fn minutes_between(earlier: &str, later: &str) -> Option<f64> {
    let (a, b) = (parse_ts(earlier)?, parse_ts(later)?);
    (b >= a).then_some((b - a) as f64 / 60.0)
}

/// Sets `normalized` on every mature item that has impressions, when the baseline is large enough.
pub fn normalize(items: &mut [OutcomeItem]) -> Option<f64> {
    let mut adjusted: Vec<f64> = items
        .iter()
        .filter(|i| i.mature)
        .filter_map(|i| i.impressions.map(|v| v as f64 / reach_factor(i.author_followers)))
        .collect();
    for i in items.iter_mut() {
        i.normalized = None;
    }
    if adjusted.len() < MIN_BASELINE_ITEMS {
        return None;
    }
    let baseline = median(&mut adjusted).filter(|b| *b > 0.0)?;
    for i in items.iter_mut().filter(|i| i.mature) {
        if let Some(v) = i.impressions {
            i.normalized = Some(((v as f64 / reach_factor(i.author_followers)) / baseline).clamp(0.0, MAX_NORMALIZED));
        }
    }
    Some(baseline)
}

/// All published replies with their best available snapshot, normalized.
pub fn load(conn: &Connection, profile_id: &str) -> AppResult<(Vec<OutcomeItem>, Option<f64>)> {
    let mut stmt = conn.prepare(
        "SELECT d.id, d.evaluation_id, d.model_used, d.initial_text, d.edited_text, d.edit_distance,
                o.author_handle, o.author_followers, o.text, o.has_media, o.posted_at,
                e.potential_score, d.published_at, d.published_post_id
         FROM reply_drafts d
         JOIN observed_posts o ON o.id = d.observed_post_id
         LEFT JOIN scout_evaluations e ON e.id = d.evaluation_id
         WHERE d.profile_id=?1 AND d.status='published'
         ORDER BY d.published_at, d.id",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, i64>(9)?,
            r.get::<_, Option<String>>(10)?,
            r.get::<_, Option<f64>>(11)?,
            r.get::<_, Option<String>>(12)?,
            r.get::<_, Option<String>>(13)?,
        ))
    })?;
    let mut items = vec![];
    for row in rows {
        let (id, eval, model, initial, edited, dist, handle, followers, target, media, posted_at, score, published_at, post_id) = row?;
        let snap = conn
            .query_row(
                "SELECT impressions, engagement_rate, age_hours FROM analytics_snapshots
                 WHERE profile_id=?1 AND scope='post'
                   AND ((draft_kind='reply' AND draft_id=?2) OR (?3 IS NOT NULL AND platform_post_id=?3))
                 ORDER BY (COALESCE(age_hours,0) >= ?4) DESC, captured_at DESC LIMIT 1",
                params![profile_id, id, post_id, MATURE_AGE_HOURS],
                |r| Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, Option<f64>>(1)?, r.get::<_, Option<f64>>(2)?)),
            )
            .optional()?;
        let (impressions, engagement_rate, age) = snap.unwrap_or((None, None, None));
        let reply_age_minutes = match (&posted_at, &published_at) {
            (Some(p), Some(q)) => minutes_between(p, q),
            _ => None,
        };
        items.push(OutcomeItem {
            draft_id: id,
            evaluation_id: eval,
            model_used: model,
            initial_text: initial,
            final_text: edited,
            edit_distance: dist,
            author_handle: handle,
            author_followers: followers,
            target_text: target,
            has_media: media != 0,
            potential_score: score,
            published_at,
            reply_age_minutes,
            impressions,
            engagement_rate,
            snapshot_age_hours: age,
            mature: age.map(|a| a >= MATURE_AGE_HOURS).unwrap_or(false) && impressions.is_some(),
            normalized: None,
        });
    }
    let baseline = normalize(&mut items);
    Ok((items, baseline))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, followers: Option<i64>, impressions: Option<i64>, age: f64) -> OutcomeItem {
        OutcomeItem {
            draft_id: id.into(),
            evaluation_id: None,
            model_used: "m".into(),
            initial_text: "a".into(),
            final_text: "a".into(),
            edit_distance: 0,
            author_handle: "h".into(),
            author_followers: followers,
            target_text: "t".into(),
            has_media: false,
            potential_score: None,
            published_at: None,
            reply_age_minutes: None,
            impressions,
            engagement_rate: None,
            snapshot_age_hours: Some(age),
            mature: age >= MATURE_AGE_HOURS && impressions.is_some(),
            normalized: None,
        }
    }

    #[test]
    fn median_item_scores_one_and_reach_is_discounted() {
        let mut items = vec![
            item("a", Some(1_000), Some(100), 24.0),
            item("b", Some(1_000), Some(200), 24.0),
            item("c", Some(1_000), Some(400), 24.0),
        ];
        let base = normalize(&mut items).unwrap();
        assert!((base - 200.0).abs() < 1.0);
        assert!((items[1].normalized.unwrap() - 1.0).abs() < 0.01);
        assert!(items[2].normalized.unwrap() > items[0].normalized.unwrap());

        let mut same_raw = vec![
            item("small", Some(500), Some(300), 30.0),
            item("big", Some(500_000), Some(300), 30.0),
            item("mid", Some(5_000), Some(300), 30.0),
        ];
        normalize(&mut same_raw);
        assert!(same_raw[0].normalized.unwrap() > same_raw[1].normalized.unwrap(), "same impressions from a huge account is worth less");
    }

    #[test]
    fn immature_or_tiny_samples_are_not_normalized() {
        let mut few = vec![item("a", None, Some(10), 30.0), item("b", None, Some(20), 30.0)];
        assert!(normalize(&mut few).is_none());
        assert!(few.iter().all(|i| i.normalized.is_none()));

        let mut mixed = vec![
            item("a", None, Some(10), 30.0),
            item("b", None, Some(20), 30.0),
            item("c", None, Some(30), 30.0),
            item("fresh", None, Some(9_999), 2.0),
        ];
        normalize(&mut mixed);
        assert!(mixed[3].normalized.is_none(), "a 2-hour-old snapshot never enters the baseline or the ranking");
    }

    #[test]
    fn timestamps_diff_in_minutes() {
        assert_eq!(minutes_between("2026-01-01T00:00:00Z", "2026-01-01T01:30:00Z"), Some(90.0));
        assert_eq!(minutes_between("2026-01-01T00:00:00Z", "2025-12-31T23:00:00Z"), None);
        assert_eq!(minutes_between("2026-02-28T23:30:00Z", "2026-03-01T00:30:00Z"), Some(60.0));
        assert_eq!(minutes_between("junk", "2026-01-01T00:00:00Z"), None);
    }
}
