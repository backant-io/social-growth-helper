//! Writes what the background worker reads from X into `analytics_snapshots`.
//!
//! No page is loaded here: the worker hands over a capture and this module validates it, stores it, matches it to
//! a tracked draft (or marks it `external`) and schedules the next checkpoint. Public counters only; nothing in
//! this path depends on X Premium analytics or any GraphQL operation name.

use super::extract::{parse_profile_timeline, AccountMetrics, PostMetrics};
use super::health;
use super::pacing;
use super::sandbox::canonical_period_days;
use super::schedule::{hours_since, parse_rfc3339_unix, TrackedPostSchedule};
use crate::browser::service::BrowserService;
use crate::browser::{AnalyticsPageCapture, AnalyticsPageRequest, BrowserError};
use crate::error::AppResult;
use crate::util::{format_unix, new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};

/// Narrow async boundary so the worker talks only to the browser driver hook (or a test double).
#[async_trait::async_trait]
pub trait AnalyticsPageFetcher: Send + Sync {
    async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError>;
}

#[async_trait::async_trait]
impl AnalyticsPageFetcher for BrowserService {
    async fn fetch_page(&self, req: AnalyticsPageRequest) -> Result<AnalyticsPageCapture, BrowserError> {
        self.capture_analytics_page(req).await
    }
}

fn store_account_snapshot(
    conn: &Connection,
    profile_id: &str,
    impressions: Option<i64>,
    engagement_rate: Option<f64>,
    profile_visits: Option<i64>,
    followers_total: Option<i64>,
    raw_json: &serde_json::Value,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO analytics_snapshots
         (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours,
          impressions, likes, replies, reposts, bookmarks, profile_visits, link_clicks, followers_total, engagement_rate, raw_json)
         VALUES (?1, ?2, 'account', NULL, NULL, NULL, ?3, NULL, ?4, NULL, NULL, NULL, NULL, ?5, NULL, ?6, ?7, ?8)",
        params![
            new_id(),
            profile_id,
            now_rfc3339(),
            impressions,
            profile_visits,
            followers_total,
            engagement_rate,
            raw_json.to_string(),
        ],
    )?;
    Ok(())
}

fn store_post_snapshot(
    conn: &Connection,
    profile_id: &str,
    m: &PostMetrics,
    draft_kind: &str,
    draft_id: Option<&str>,
    age_hours: Option<f64>,
) -> AppResult<()> {
    let mut raw = m.raw_json.clone();
    if let Some(obj) = raw.as_object_mut() {
        if let Some(t) = &m.text {
            obj.entry("text").or_insert_with(|| serde_json::Value::String(t.clone()));
        }
        if let Some(p) = &m.posted_at {
            obj.entry("postedAt").or_insert_with(|| serde_json::Value::String(p.clone()));
        }
        obj.insert("extraction_source".into(), serde_json::Value::String(m.source.clone()));
    }

    conn.execute(
        "INSERT INTO analytics_snapshots
         (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours,
          impressions, likes, replies, reposts, bookmarks, profile_visits, link_clicks, followers_total, engagement_rate, raw_json)
         VALUES (?1, ?2, 'post', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, ?15, ?16)",
        params![
            new_id(),
            profile_id,
            m.platform_post_id,
            draft_kind,
            draft_id,
            now_rfc3339(),
            age_hours,
            m.impressions,
            m.likes,
            m.replies,
            m.reposts,
            m.bookmarks,
            m.profile_visits,
            m.link_clicks,
            m.engagement_rate,
            raw.to_string(),
        ],
    )?;
    Ok(())
}

/// Resolves whether a discovered post belongs to a tracked reply/OGC draft or is an `external` post (ANL-17).
fn match_draft_for_post(
    conn: &Connection,
    profile_id: &str,
    platform_post_id: &str,
    text: Option<&str>,
) -> AppResult<(String, Option<String>)> {
    if let Some((id, kind)) = conn
        .query_row(
            "SELECT id, COALESCE(draft_kind, 'reply') FROM reply_drafts
             WHERE profile_id = ?1 AND published_post_id = ?2 LIMIT 1",
            params![profile_id, platform_post_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        return Ok((kind, Some(id)));
    }
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM ogc_drafts WHERE profile_id = ?1 AND published_post_id = ?2 LIMIT 1",
            params![profile_id, platform_post_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(("ogc".into(), Some(id)));
    }

    // Fallback: match by exact trimmed text for drafts whose permalink wasn't captured at publish time (DRF-18).
    if let Some(raw_text) = text.map(str::trim).filter(|t| t.len() >= 12) {
        if let Some((id, kind)) = conn
            .query_row(
                "SELECT id, COALESCE(draft_kind, 'reply') FROM reply_drafts
                 WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NULL AND trim(edited_text) = ?2
                 LIMIT 1",
                params![profile_id, raw_text],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .optional()?
        {
            let _ = conn.execute(
                "UPDATE reply_drafts SET published_post_id = ?1 WHERE id = ?2",
                params![platform_post_id, id],
            );
            return Ok((kind, Some(id)));
        }
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM ogc_drafts
                 WHERE profile_id = ?1 AND status = 'published' AND published_post_id IS NULL AND trim(edited_text) = ?2
                 LIMIT 1",
                params![profile_id, raw_text],
                |r| r.get::<_, String>(0),
            )
            .optional()?
        {
            let _ = conn.execute(
                "UPDATE ogc_drafts SET published_post_id = ?1 WHERE id = ?2",
                params![platform_post_id, id],
            );
            return Ok(("ogc".into(), Some(id)));
        }
    }

    Ok(("external".into(), None))
}

pub fn profile_handle(conn: &Connection, profile_id: &str) -> AppResult<Option<String>> {
    let h: Option<String> = conn
        .query_row("SELECT handle FROM profiles WHERE id = ?1", [profile_id], |r| r.get(0))
        .optional()?;
    Ok(h.map(|s| s.trim().trim_start_matches('@').to_string()).filter(|s| !s.is_empty()))
}

pub fn update_profile_handle_if_empty(conn: &Connection, profile_id: &str, handle: &str) {
    let clean = handle.trim().trim_start_matches('@');
    if !clean.is_empty() {
        let _ = conn.execute(
            "UPDATE profiles SET handle = ?1 WHERE id = ?2 AND (handle IS NULL OR handle = '')",
            params![clean, profile_id],
        );
    }
}

/// Counters X could never show: negative, not finite, or more engagements than views.
pub fn metrics_are_sane(m: &PostMetrics) -> bool {
    let counters = [m.impressions, m.likes, m.replies, m.reposts, m.bookmarks, m.profile_visits, m.link_clicks];
    if counters.iter().flatten().any(|v| *v < 0) {
        return false;
    }
    if m.engagement_rate.is_some_and(|r| !r.is_finite() || !(0.0..=100.0).contains(&r)) {
        return false;
    }
    match m.impressions {
        Some(imp) if imp > 0 => [m.likes, m.replies, m.reposts, m.bookmarks].iter().flatten().sum::<i64>() <= imp,
        _ => true,
    }
}

/// Sanity check for X Creator / Premium account-period totals before storing.
pub fn account_metrics_are_sane(m: &AccountMetrics) -> bool {
    let non_negative = [
        m.impressions,
        m.engagements,
        m.profile_visits,
        m.followers_total,
        m.new_follows,
        m.unfollows,
        m.likes,
        m.replies,
        m.reposts,
        m.bookmarks,
    ];
    if non_negative.iter().flatten().any(|v| *v < 0) {
        return false;
    }
    if m.engagement_rate.is_some_and(|r| !r.is_finite() || !(0.0..=100.0).contains(&r)) {
        return false;
    }
    let has_any = m.impressions.is_some()
        || m.engagements.is_some()
        || m.engagement_rate.is_some()
        || m.profile_visits.is_some()
        || m.follower_delta.is_some()
        || m.new_follows.is_some();
    if !has_any {
        return false;
    }
    if let (Some(imp), Some(eng)) = (m.impressions, m.engagements) {
        if imp > 0 && eng > imp * 10 {
            return false;
        }
    }
    true
}

/// Stores one period snapshot (`7` or `30` days) from X Creator Analytics (`x.com/i/account_analytics`).
pub fn ingest_account_period_metrics(
    conn: &Connection,
    profile_id: &str,
    period_days: u32,
    m: &AccountMetrics,
    now: i64,
) -> AppResult<bool> {
    if !account_metrics_are_sane(m) {
        return Ok(false);
    }
    if let Some(h) = &m.handle {
        update_profile_handle_if_empty(conn, profile_id, h);
    }
    let canonical = canonical_period_days(period_days);
    let label = m
        .period_label
        .clone()
        .unwrap_or_else(|| if canonical == 7 { "7D".into() } else { "30D".into() });
    let follower_delta = m.follower_delta.or_else(|| match (m.new_follows, m.unfollows) {
        (Some(n), Some(u)) => Some(n - u),
        (Some(n), None) => Some(n),
        _ => None,
    });
    let captured_at = format_unix(now);
    conn.execute(
        "INSERT INTO analytics_period_snapshots
         (id, profile_id, period_days, period_label, captured_at, impressions, engagements, engagement_rate,
          profile_visits, followers_total, follower_delta, new_follows, unfollows,
          likes, replies, reposts, bookmarks, source, raw_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
        params![
            new_id(),
            profile_id,
            canonical,
            label,
            captured_at,
            m.impressions,
            m.engagements,
            m.engagement_rate,
            m.profile_visits,
            m.followers_total,
            follower_delta,
            m.new_follows,
            m.unfollows,
            m.likes,
            m.replies,
            m.reposts,
            m.bookmarks,
            if m.source.is_empty() { "dom_fallback" } else { m.source.as_str() },
            m.raw_json.to_string(),
        ],
    )?;
    health::mark_updated(conn)?;
    Ok(true)
}

fn round_age(hours: f64) -> f64 {
    (hours * 10.0).round() / 10.0
}

/// Stores one post snapshot for a tracked draft and schedules its next checkpoint. Returns false (and stores
/// nothing) for metrics that fail the sanity check.
pub fn ingest_post_metrics(conn: &Connection, profile_id: &str, item: &TrackedPostSchedule, m: &PostMetrics, now: i64) -> AppResult<bool> {
    if !metrics_are_sane(m) {
        return Ok(false);
    }
    let age = round_age(hours_since(&item.published_at, now));
    store_post_snapshot(conn, profile_id, m, &item.draft_kind, Some(&item.draft_id), Some(age))?;
    pacing::mark_synced(conn, profile_id, &item.platform_post_id, age, now)?;
    let _ = crate::formats::winrate::recompute_personal_win_rates(conn, profile_id);
    health::mark_updated(conn)?;
    Ok(true)
}

fn has_recent_snapshot(conn: &Connection, profile_id: &str, post_id: &str, since_unix: i64) -> AppResult<bool> {
    let hit: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM analytics_snapshots WHERE profile_id = ?1 AND scope = 'post' AND platform_post_id = ?2 AND captured_at >= ?3 LIMIT 1",
            params![profile_id, post_id, format_unix(since_unix)],
            |r| r.get(0),
        )
        .optional()?;
    Ok(hit.is_some())
}

/// Stores follower count and the public counters of recent own posts from a profile page (`/handle` or
/// `/handle/with_replies`). Posts seen in the last three hours are not stored again. Returns how many post
/// snapshots were written.
pub fn ingest_profile_page(conn: &Connection, profile_id: &str, dom: &serde_json::Value, now: i64, window_days: i64) -> AppResult<usize> {
    let timeline = parse_profile_timeline(dom);
    if let Some(h) = &timeline.handle {
        update_profile_handle_if_empty(conn, profile_id, h);
    }
    let mut stored = 0;
    if timeline.followers_total.is_some() {
        store_account_snapshot(conn, profile_id, None, None, None, timeline.followers_total, &serde_json::json!({ "source": "profile_header" }))?;
        stored += 1;
    }
    let cutoff = now - window_days * 86_400;
    for post in timeline.posts {
        if post.posted_at.as_deref().and_then(parse_rfc3339_unix).is_some_and(|ts| ts < cutoff) {
            continue;
        }
        if !metrics_are_sane(&post) || has_recent_snapshot(conn, profile_id, &post.platform_post_id, now - 3 * 3600)? {
            continue;
        }
        let (kind, draft_id) = match_draft_for_post(conn, profile_id, &post.platform_post_id, post.text.as_deref())?;
        let age = post.posted_at.as_deref().map(|p| round_age(hours_since(p, now)));
        store_post_snapshot(conn, profile_id, &post, &kind, draft_id.as_deref(), age)?;
        if draft_id.is_some() {
            if let Some(age) = age {
                pacing::mark_synced(conn, profile_id, &post.platform_post_id, age, now)?;
            }
        }
        stored += 1;
    }
    if stored > 0 {
        let _ = crate::formats::winrate::recompute_personal_win_rates(conn, profile_id);
        health::mark_updated(conn)?;
    }
    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::schedule::{compute_schedule, now_unix_secs};
    use crate::storage::{open, repo};
    use serde_json::json;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, Connection, String) {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        let published_at = format_unix(now_unix_secs() - 2 * 3600);
        conn.execute(
            "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, author_followers, text, feed_source, posted_at, first_seen_at, metrics_at_capture_json)
             VALUES ('op1', ?1, 'src1', 'naval', 200000, 'Build leverage', 'for_you', ?2, ?2, '{}')",
            params![pid, published_at],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, edit_distance, model_used, style_guide_version, draft_kind, actions_json, status, published_at, published_post_id, published_url, created_at)
             VALUES ('rd1', ?1, 'op1', 'hi', 'sharp reply', 5, 'claude-sonnet', 1, 'reply', '[\"like\",\"reply\"]', 'published', ?2, '9001', 'https://x.com/pavel/status/9001', ?2)",
            params![pid, published_at],
        )
        .unwrap();
        (dir, conn, pid)
    }

    fn metrics(imp: Option<i64>, likes: i64) -> PostMetrics {
        PostMetrics {
            platform_post_id: "9001".into(),
            impressions: imp,
            likes: Some(likes),
            replies: Some(1),
            source: "dom_fallback".into(),
            raw_json: json!({}),
            ..Default::default()
        }
    }

    #[test]
    fn stores_a_post_snapshot_and_schedules_the_next_checkpoint() {
        let (_d, conn, pid) = setup();
        let now = now_unix_secs();
        let item = compute_schedule(&conn, &pid, now).unwrap().items.remove(0);
        assert!(ingest_post_metrics(&conn, &pid, &item, &metrics(Some(900), 22), now).unwrap());

        let (likes, kind, age): (i64, String, f64) = conn
            .query_row("SELECT likes, draft_kind, age_hours FROM analytics_snapshots WHERE platform_post_id = '9001'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        assert_eq!((likes, kind.as_str()), (22, "reply"));
        assert!((age - 2.0).abs() < 0.2);
        let st = pacing::load_state(&conn, &pid, "9001").unwrap().unwrap();
        assert!(st.next_due_at > now + 3 * 3600, "next look is the 6h checkpoint");
        assert!(health::load_status(&conn).last_update_at.is_some());
    }

    #[test]
    fn rejects_impossible_numbers_instead_of_storing_them() {
        let (_d, conn, pid) = setup();
        let now = now_unix_secs();
        let item = compute_schedule(&conn, &pid, now).unwrap().items.remove(0);
        assert!(!ingest_post_metrics(&conn, &pid, &item, &metrics(Some(-5), 1), now).unwrap());
        assert!(!ingest_post_metrics(&conn, &pid, &item, &metrics(Some(10), 500), now).unwrap(), "more likes than views");
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM analytics_snapshots", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(pacing::load_state(&conn, &pid, "9001").unwrap().is_none());
    }

    #[test]
    fn profile_page_stores_followers_matches_drafts_and_flags_external_posts() {
        let (_d, conn, pid) = setup();
        let now = now_unix_secs();
        let recent = format_unix(now - 3600);
        let old = format_unix(now - 45 * 86_400);
        let dom = json!({
            "handle": "pavel",
            "followersText": "4,510 Followers",
            "posts": [
                { "id": "9001", "text": "sharp reply", "postedAt": recent, "views": "5.4K Views", "likes": "140 Likes" },
                { "id": "9999", "text": "Posted manually from phone", "postedAt": recent, "views": "1.1K Views", "likes": "25 Likes" },
                { "id": "7777", "text": "Too old", "postedAt": old, "views": "300 Views", "likes": "5 Likes" }
            ]
        });
        assert_eq!(ingest_profile_page(&conn, &pid, &dom, now, 30).unwrap(), 3, "followers plus two recent posts");

        let ext: String = conn.query_row("SELECT draft_kind FROM analytics_snapshots WHERE platform_post_id = '9999'", [], |r| r.get(0)).unwrap();
        assert_eq!(ext, "external");
        let old_n: i64 = conn.query_row("SELECT COUNT(*) FROM analytics_snapshots WHERE platform_post_id = '7777'", [], |r| r.get(0)).unwrap();
        assert_eq!(old_n, 0);
        let followers: i64 = conn.query_row("SELECT followers_total FROM analytics_snapshots WHERE scope = 'account'", [], |r| r.get(0)).unwrap();
        assert_eq!(followers, 4510);
        let handle: String = conn.query_row("SELECT handle FROM profiles WHERE id = ?1", [&pid], |r| r.get(0)).unwrap();
        assert_eq!(handle, "pavel");
        assert!(pacing::load_state(&conn, &pid, "9001").unwrap().is_some(), "a tracked post seen on the profile is not refetched right away");
        assert!(pacing::load_state(&conn, &pid, "9999").unwrap().is_none());

        assert_eq!(ingest_profile_page(&conn, &pid, &dom, now + 600, 30).unwrap(), 1, "posts seen within three hours are not stored twice");
    }

    #[test]
    fn stores_account_period_snapshots_separately_for_7d_and_30d_and_rejects_bad_values() {
        let (_d, conn, pid) = setup();
        let now = now_unix_secs();

        let bad = AccountMetrics {
            impressions: Some(-10),
            ..Default::default()
        };
        assert!(!ingest_account_period_metrics(&conn, &pid, 7, &bad, now).unwrap());

        let m7 = AccountMetrics {
            handle: Some("pavel".into()),
            period_days: Some(7),
            period_label: Some("7D".into()),
            impressions: Some(42_500),
            engagements: Some(2_125),
            engagement_rate: Some(5.0),
            profile_visits: Some(610),
            new_follows: Some(40),
            unfollows: Some(8),
            source: "dom_fallback".into(),
            raw_json: json!({ "selectedRangeLabel": "7D" }),
            ..Default::default()
        };
        let m30 = AccountMetrics {
            handle: Some("pavel".into()),
            period_days: Some(28),
            period_label: Some("4W".into()),
            impressions: Some(184_000),
            engagements: Some(7_360),
            engagement_rate: Some(4.0),
            profile_visits: Some(2_450),
            new_follows: Some(190),
            unfollows: Some(35),
            source: "dom_fallback".into(),
            raw_json: json!({ "selectedRangeLabel": "4W" }),
            ..Default::default()
        };

        assert!(ingest_account_period_metrics(&conn, &pid, 7, &m7, now).unwrap());
        assert!(ingest_account_period_metrics(&conn, &pid, 30, &m30, now + 60).unwrap());

        let row7: (i64, f64, i64, String) = conn
            .query_row(
                "SELECT impressions, engagement_rate, follower_delta, period_label
                 FROM v_analytics_account_overview WHERE profile_id = ?1 AND period_days = 7",
                [&pid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row7, (42_500, 5.0, 32, "7D".to_string()));

        let row30: (i64, f64, i64, String) = conn
            .query_row(
                "SELECT impressions, engagement_rate, follower_delta, period_label
                 FROM v_analytics_account_overview WHERE profile_id = ?1 AND period_days = 30",
                [&pid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(row30, (184_000, 4.0, 155, "4W".to_string()));
    }
}
