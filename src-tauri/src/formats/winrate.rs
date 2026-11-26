//! Personal win rate per viral format (OGC-10): of the user's own published posts that used a format, the share
//! that did at least as well as their typical post. Derived from analytics snapshots, recomputed after every
//! stats sync, and left empty (`None`) until a format has enough measured posts.

use crate::error::AppResult;
use crate::learning::outcomes::{median, MATURE_AGE_HOURS, MIN_BASELINE_ITEMS};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Measured posts a format needs before its personal win rate is shown.
pub const MIN_FORMAT_POSTS: usize = 2;

#[derive(Debug, Clone)]
struct Measured {
    draft_id: Option<String>,
    age_hours: Option<f64>,
    rate: Option<f64>,
}

/// Engagement rate in percent, from the stored value or from raw counters when only those were captured.
fn rate_of(stored: Option<f64>, impressions: i64, interactions: i64) -> Option<f64> {
    stored.or_else(|| (impressions > 0).then(|| interactions as f64 * 100.0 / impressions as f64))
}

fn latest_post_snapshots(conn: &Connection, profile_id: &str) -> AppResult<HashMap<String, Measured>> {
    let mut stmt = conn.prepare(
        "SELECT platform_post_id, draft_id, age_hours, COALESCE(impressions,0), \
                COALESCE(likes,0)+COALESCE(replies,0)+COALESCE(reposts,0)+COALESCE(bookmarks,0), engagement_rate, captured_at \
         FROM analytics_snapshots WHERE profile_id=?1 AND scope='post' AND platform_post_id IS NOT NULL \
         ORDER BY captured_at ASC, rowid ASC",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Measured { draft_id: r.get(1)?, age_hours: r.get(2)?, rate: rate_of(r.get(5)?, r.get(3)?, r.get(4)?) },
        ))
    })?;
    let mut latest = HashMap::new();
    for row in rows {
        let (post, m) = row?;
        latest.insert(post, m);
    }
    Ok(latest)
}

fn is_mature(m: &Measured) -> bool {
    m.age_hours.is_some_and(|h| h >= MATURE_AGE_HOURS) && m.rate.is_some()
}

/// Recomputes `viral_formats.personal_win_rate` for the profile and returns how many formats now have a value.
///
/// A post counts once its latest snapshot is at least 24 hours old. A win is an engagement rate at or above the
/// median across all the user's mature posts (replies and external posts included), so the bar is the user's own
/// normal, not a fixed number. With fewer than three mature posts overall there is no baseline and every format
/// stays empty.
pub fn recompute_personal_win_rates(conn: &Connection, profile_id: &str) -> AppResult<usize> {
    let snaps = latest_post_snapshots(conn, profile_id)?;
    let mut all_rates: Vec<f64> = snaps.values().filter(|m| is_mature(m)).filter_map(|m| m.rate).collect();
    let baseline = if all_rates.len() >= MIN_BASELINE_ITEMS { median(&mut all_rates) } else { None };

    let mut per_format: HashMap<String, (usize, usize)> = HashMap::new();
    if let Some(base) = baseline {
        let mut stmt = conn.prepare(
            "SELECT id, viral_format_id, published_post_id FROM ogc_drafts \
             WHERE profile_id=?1 AND status='published' AND viral_format_id IS NOT NULL",
        )?;
        let drafts: Vec<(String, String, Option<String>)> =
            stmt.query_map([profile_id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        for (draft_id, format_id, post_id) in drafts {
            let measured = post_id
                .as_deref()
                .and_then(|p| snaps.get(p))
                .or_else(|| snaps.values().find(|m| m.draft_id.as_deref() == Some(draft_id.as_str())));
            let Some(rate) = measured.filter(|m| is_mature(m)).and_then(|m| m.rate) else { continue };
            let e = per_format.entry(format_id).or_default();
            e.0 += 1;
            if rate >= base {
                e.1 += 1;
            }
        }
    }

    let mut with_value = 0;
    let mut stmt = conn.prepare("SELECT id FROM viral_formats WHERE profile_id=?1")?;
    let ids: Vec<String> = stmt.query_map([profile_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in ids {
        let value = per_format.get(&id).filter(|(n, _)| *n >= MIN_FORMAT_POSTS).map(|(n, wins)| *wins as f64 / *n as f64);
        if value.is_some() {
            with_value += 1;
        }
        conn.execute("UPDATE viral_formats SET personal_win_rate=?1 WHERE id=?2 AND profile_id=?3", params![value, id, profile_id])?;
    }
    Ok(with_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::blueprint::FormatBlueprint;
    use crate::formats::repo::{self, testing::db};

    fn format(conn: &Connection, pid: &str, name: &str) -> String {
        let bp = FormatBlueprint {
            name: name.into(),
            archetype: "contrarian_take".into(),
            hook_formula: "h".into(),
            structural_pattern: "p".into(),
            skeleton: "Opinion: {belief} is wrong.".into(),
            slots: vec![crate::formats::blueprint::Slot { key: "belief".into(), label: "Belief".into(), hint: "".into() }],
            media_hint: "text_only".into(),
        };
        repo::insert_format(conn, pid, &bp, "manual", None, None).unwrap().id
    }

    fn ogc(conn: &Connection, pid: &str, format_id: &str, post_id: &str, status: &str) -> String {
        let id = crate::util::new_id();
        conn.execute(
            "INSERT INTO ogc_drafts (id, profile_id, viral_format_id, topic_seed, initial_text, edited_text, model_used, status, published_post_id, created_at) \
             VALUES (?1,?2,?3,'t','x','x','m',?4,?5,'2026-01-01T00:00:00Z')",
            params![id, pid, format_id, status, post_id],
        )
        .unwrap();
        id
    }

    fn snap(conn: &Connection, pid: &str, post_id: &str, draft_id: Option<&str>, age: f64, rate: f64, captured: &str) {
        conn.execute(
            "INSERT INTO analytics_snapshots (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours, impressions, engagement_rate) \
             VALUES (?1,?2,'post',?3,'ogc',?4,?5,?6,1000,?7)",
            params![crate::util::new_id(), pid, post_id, draft_id, captured, age, rate],
        )
        .unwrap();
    }

    fn rate_for(conn: &Connection, pid: &str, id: &str) -> Option<f64> {
        repo::get_format(conn, pid, id).unwrap().unwrap().personal_win_rate
    }

    #[test]
    fn stays_empty_without_a_baseline_or_enough_posts() {
        let (conn, pid, _d) = db();
        let f = format(&conn, &pid, "Hot take");
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 0);
        assert_eq!(rate_for(&conn, &pid, &f), None);

        ogc(&conn, &pid, &f, "p1", "published");
        snap(&conn, &pid, "p1", None, 30.0, 5.0, "2026-02-01T00:00:00Z");
        snap(&conn, &pid, "o1", None, 30.0, 1.0, "2026-02-01T00:00:00Z");
        snap(&conn, &pid, "o2", None, 30.0, 2.0, "2026-02-01T00:00:00Z");
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 0, "one measured post is not enough");
        assert_eq!(rate_for(&conn, &pid, &f), None);
    }

    #[test]
    fn counts_wins_against_the_users_own_median_using_the_latest_mature_snapshot() {
        let (conn, pid, _d) = db();
        let strong = format(&conn, &pid, "Strong");
        let weak = format(&conn, &pid, "Weak");
        let unused = format(&conn, &pid, "Unused");
        // Baseline posts (replies / external): rates 1, 2, 3 plus the four OGC posts below.
        for (i, r) in [1.0, 2.0, 3.0].iter().enumerate() {
            snap(&conn, &pid, &format!("base{i}"), None, 48.0, *r, "2026-02-01T00:00:00Z");
        }
        for (post, fmt, rate) in [("s1", &strong, 6.0), ("s2", &strong, 5.0), ("w1", &weak, 0.5), ("w2", &weak, 4.0)] {
            ogc(&conn, &pid, fmt, post, "published");
            snap(&conn, &pid, post, None, 48.0, rate, "2026-02-02T00:00:00Z");
        }
        // An older, immature snapshot of w2 must not override its latest one.
        snap(&conn, &pid, "w2", None, 3.0, 0.1, "2026-02-01T12:00:00Z");
        // All rates: 0.5,1,2,3,4,5,6 -> median 3.0. strong: 2/2 wins; weak: 0.5 loses, 4.0 wins.
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 2);
        assert_eq!(rate_for(&conn, &pid, &strong), Some(1.0));
        assert_eq!(rate_for(&conn, &pid, &weak), Some(0.5));
        assert_eq!(rate_for(&conn, &pid, &unused), None);
    }

    #[test]
    fn immature_unpublished_and_draft_linked_posts() {
        let (conn, pid, _d) = db();
        let f = format(&conn, &pid, "F");
        for (i, r) in [1.0, 2.0, 3.0].iter().enumerate() {
            snap(&conn, &pid, &format!("b{i}"), None, 48.0, *r, "2026-02-01T00:00:00Z");
        }
        ogc(&conn, &pid, &f, "young", "published");
        snap(&conn, &pid, "young", None, 2.0, 9.0, "2026-02-02T00:00:00Z");
        ogc(&conn, &pid, &f, "never", "draft");
        snap(&conn, &pid, "never", None, 48.0, 9.0, "2026-02-02T00:00:00Z");
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 0, "young and unpublished posts are not counted");

        // Matched through draft_id when the permalink was never captured.
        let a = ogc(&conn, &pid, &f, "", "published");
        ogc(&conn, &pid, &f, "pb", "published");
        snap(&conn, &pid, "found-by-text", Some(&a), 48.0, 8.0, "2026-02-03T00:00:00Z");
        snap(&conn, &pid, "pb", None, 48.0, 0.2, "2026-02-03T00:00:00Z");
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 1);
        assert_eq!(rate_for(&conn, &pid, &f), Some(0.5));
    }

    #[test]
    fn is_recomputed_and_cleared_when_the_data_changes() {
        let (conn, pid, _d) = db();
        let f = format(&conn, &pid, "F");
        conn.execute("UPDATE viral_formats SET personal_win_rate=0.9 WHERE id=?1", [&f]).unwrap();
        assert_eq!(recompute_personal_win_rates(&conn, &pid).unwrap(), 0);
        assert_eq!(rate_for(&conn, &pid, &f), None, "a stale value does not survive without evidence");
    }

    #[test]
    fn falls_back_to_raw_counters_when_no_rate_was_captured() {
        assert_eq!(rate_of(Some(2.5), 0, 0), Some(2.5));
        assert_eq!(rate_of(None, 1000, 50), Some(5.0));
        assert_eq!(rate_of(None, 0, 50), None);
    }
}
