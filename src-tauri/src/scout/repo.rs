use super::model::{MediaItem, Metrics, ScrapedPost};
use crate::drafts::repo::{post_url, PostView};
use crate::error::{AppError, AppResult};
use crate::storage::repo::{get_setting, set_setting};
use crate::util::{format_unix, new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

pub const DEFAULT_THRESHOLD: f64 = 72.0;
pub const QUEUE_TTL_SECS: i64 = 24 * 3600;
pub const FEED_SOURCES: &[&str] = &["for_you", "following", "watchlist", "both", "all"];
pub const DISMISS_REASONS: &[&str] = &["off_topic", "low_quality", "not_my_voice", "already_replied", "too_late", "other"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionParams {
    pub feed_source: String,
    pub max_duration_sec: i64,
    pub max_posts: i64,
    pub stop_after_queued: Option<i64>,
    pub threshold: f64,
}

impl SessionParams {
    /// Manual duration and post amount are required, with sane bounds (SCT-01, SCT-22).
    pub fn validate(&self) -> AppResult<()> {
        if !FEED_SOURCES.contains(&self.feed_source.as_str()) {
            return Err(AppError::Validation("feed source must be For You, Following, Watchlist, both (For You + Watchlist) or all".into()));
        }
        if !(30..=4 * 3600).contains(&self.max_duration_sec) {
            return Err(AppError::Validation("duration must be between 30 seconds and 4 hours".into()));
        }
        if !(5..=2000).contains(&self.max_posts) {
            return Err(AppError::Validation("post amount must be between 5 and 2000".into()));
        }
        if self.stop_after_queued.is_some_and(|n| !(1..=500).contains(&n)) {
            return Err(AppError::Validation("stop-after must be between 1 and 500 queued posts".into()));
        }
        if !(0.0..=100.0).contains(&self.threshold) {
            return Err(AppError::Validation("threshold must be between 0 and 100".into()));
        }
        Ok(())
    }
}

const LAST_PARAMS_KEY: &str = "scout_last_params";

/// Remembers the budget of the most recent session so the Scout page can prefill it (SCT-24).
pub fn save_last_params(conn: &Connection, params: &SessionParams) -> AppResult<()> {
    let json = serde_json::to_string(params).map_err(|e| AppError::Other(format!("encode scout params: {e}")))?;
    set_setting(conn, LAST_PARAMS_KEY, &json)
}

/// The remembered budget, or `None` when nothing is stored or it no longer validates.
pub fn last_params(conn: &Connection) -> AppResult<Option<SessionParams>> {
    Ok(get_setting(conn, LAST_PARAMS_KEY)?
        .and_then(|v| serde_json::from_str::<SessionParams>(&v).ok())
        .filter(|p| p.validate().is_ok()))
}

pub fn get_threshold(conn: &Connection) -> AppResult<f64> {
    Ok(get_setting(conn, "scout_threshold")?.and_then(|v| v.parse().ok()).unwrap_or(DEFAULT_THRESHOLD))
}

pub fn set_threshold(conn: &Connection, v: f64) -> AppResult<f64> {
    if !(0.0..=100.0).contains(&v) {
        return Err(AppError::Validation("threshold must be between 0 and 100".into()));
    }
    set_setting(conn, "scout_threshold", &v.to_string())?;
    Ok(v)
}

/// Best known follower count for `@handle`: checks `creator_watchlist.followers` first, then the most recent
/// `observed_posts.author_followers` row for that handle.
pub fn lookup_author_followers(conn: &Connection, profile_id: &str, handle: &str) -> Option<i64> {
    let clean = handle.trim().trim_start_matches('@');
    if clean.is_empty() {
        return None;
    }
    if let Ok(Some(f)) = conn.query_row(
        "SELECT followers FROM creator_watchlist WHERE profile_id = ?1 AND lower(handle) = lower(?2) AND followers IS NOT NULL LIMIT 1",
        params![profile_id, clean],
        |r| r.get::<_, Option<i64>>(0),
    ) {
        return Some(f);
    }
    conn.query_row(
        "SELECT author_followers FROM observed_posts WHERE profile_id = ?1 AND lower(author_handle) = lower(?2) AND author_followers IS NOT NULL ORDER BY first_seen_at DESC LIMIT 1",
        params![profile_id, clean],
        |r| r.get::<_, Option<i64>>(0),
    )
    .ok()
    .flatten()
}

pub fn upsert_observed(
    conn: &Connection,
    profile_id: &str,
    p: &ScrapedPost,
    m: &Metrics,
    velocity: f64,
    feed_source: &str,
    author_followers: Option<i64>,
) -> AppResult<String> {
    let pid = p.id.clone().unwrap_or_default();
    let metrics = serde_json::to_string(m).unwrap_or_else(|_| "{}".into());
    let media_items = p.resolved_media();
    let media_json = serde_json::to_string(&media_items).unwrap_or_else(|_| "[]".into());
    if let Some(id) = conn
        .query_row("SELECT id FROM observed_posts WHERE profile_id = ?1 AND platform_post_id = ?2", params![profile_id, pid], |r| r.get::<_, String>(0))
        .optional()?
    {
        let _ = conn.execute(
            "UPDATE observed_posts
             SET author_followers = COALESCE(?2, author_followers),
                 author_verified = MAX(author_verified, ?3),
                 media_json = CASE WHEN ?4 <> '[]' THEN ?4 ELSE media_json END,
                 author_avatar_url = COALESCE(?5, author_avatar_url)
             WHERE id = ?1",
            params![id, author_followers, p.author_verified as i64, media_json, p.avatar_url],
        );
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, author_display_name, author_followers, author_verified, text, has_media, media_json, feed_source,
            posted_at, first_seen_at, metrics_at_capture_json, velocity_score, context_json, author_avatar_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)",
        params![
            id,
            profile_id,
            pid,
            p.handle.clone().unwrap_or_default(),
            p.author,
            author_followers,
            p.author_verified as i64,
            p.text,
            (p.has_media || !media_items.is_empty()) as i64,
            media_json,
            feed_source,
            p.time,
            now_rfc3339(),
            metrics,
            velocity,
            p.context_text(),
            p.avatar_url
        ],
    )?;
    Ok(id)
}

pub fn has_evaluation(conn: &Connection, profile_id: &str, platform_post_id: &str) -> bool {
    conn.query_row(
        "SELECT 1 FROM scout_evaluations e JOIN observed_posts o ON o.id = e.observed_post_id
         WHERE o.profile_id = ?1 AND o.platform_post_id = ?2",
        params![profile_id, platform_post_id],
        |_| Ok(()),
    )
    .optional()
    .map(|r| r.is_some())
    .unwrap_or(false)
}

/// SQL predicate (over `observed_posts o`) true when we already replied to or quoted the post: an executed
/// reply/quote action event, or a published draft whose bundle carried a reply/quote. Durable across sessions.
const ENGAGED_PREDICATE: &str = "(
    EXISTS (SELECT 1 FROM action_events ae
            WHERE ae.profile_id = o.profile_id AND ae.platform_post_id = o.platform_post_id AND ae.ok = 1 AND ae.action IN ('reply', 'quote'))
    OR EXISTS (SELECT 1 FROM reply_drafts pd
               WHERE pd.observed_post_id = o.id AND pd.status = 'published'
                 AND (pd.actions_json LIKE '%\"reply\"%' OR pd.actions_json LIKE '%\"quote\"%'))
)";

/// True when we already replied to or quoted this post, so Scout must never surface it again.
pub fn already_engaged(conn: &Connection, profile_id: &str, platform_post_id: &str) -> bool {
    let sql = format!(
        "SELECT EXISTS (SELECT 1 FROM action_events WHERE profile_id = ?1 AND platform_post_id = ?2 AND ok = 1 AND action IN ('reply', 'quote'))
             OR EXISTS (SELECT 1 FROM observed_posts o WHERE o.profile_id = ?1 AND o.platform_post_id = ?2 AND {ENGAGED_PREDICATE})"
    );
    conn.query_row(&sql, params![profile_id, platform_post_id], |r| r.get::<_, bool>(0)).unwrap_or(false)
}

/// Active rubric text and version: the learned rubric when Phase 4 has produced one, else version 0.
pub fn active_rubric(conn: &Connection, profile_id: &str) -> AppResult<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT version, scout_criteria_text FROM learning_rubrics WHERE profile_id = ?1 AND is_active = 1 ORDER BY version DESC LIMIT 1",
            [profile_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Active rubric, storing the built-in default as version 1 when the profile has no rubric yet (SCT-09).
pub fn ensure_default_rubric(conn: &Connection, profile_id: &str) -> AppResult<(i64, String)> {
    if let Some(active) = active_rubric(conn, profile_id)? {
        return Ok(active);
    }
    let any: i64 = conn.query_row("SELECT count(*) FROM learning_rubrics WHERE profile_id = ?1", [profile_id], |r| r.get(0))?;
    if any == 0 {
        conn.execute(
            "INSERT INTO learning_rubrics (profile_id, scout_criteria_text, weights_json, is_active, created_at) VALUES (?1, ?2, '{}', 1, ?3)",
            rusqlite::params![profile_id, super::score::DEFAULT_RUBRIC, crate::util::now_rfc3339()],
        )?;
    }
    Ok(active_rubric(conn, profile_id)?.unwrap_or((0, super::score::DEFAULT_RUBRIC.to_string())))
}

#[allow(clippy::too_many_arguments)]
pub fn insert_evaluation(conn: &Connection, observed_id: &str, session_id: &str, rubric_version: i64, model: &str, score: f64, angle: &str, reasoning: &str) -> AppResult<String> {
    insert_evaluation_with_outlier(conn, observed_id, session_id, rubric_version, model, score, angle, reasoning, None, &[])
}

#[allow(clippy::too_many_arguments)]
pub fn insert_evaluation_with_outlier(
    conn: &Connection,
    observed_id: &str,
    session_id: &str,
    rubric_version: i64,
    model: &str,
    score: f64,
    angle: &str,
    reasoning: &str,
    outlier_score: Option<f64>,
    breakout_flags: &[String],
) -> AppResult<String> {
    let id = new_id();
    let flags_json = serde_json::to_string(breakout_flags).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT OR IGNORE INTO scout_evaluations (id, observed_post_id, session_id, rubric_version, model_used, potential_score, angle_summary, reasoning, outlier_score, breakout_flags_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'queued', ?11)",
        params![id, observed_id, session_id, rubric_version, model, score, angle, reasoning, outlier_score, flags_json, now_rfc3339()],
    )?;
    Ok(id)
}

pub fn create_session(conn: &Connection, profile_id: &str, p: &SessionParams) -> AppResult<String> {
    let id = new_id();
    conn.execute(
        "INSERT INTO scout_sessions (id, profile_id, feed_source, max_duration_sec, max_posts, stop_after_queued, started_at, threshold)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![id, profile_id, p.feed_source, p.max_duration_sec, p.max_posts, p.stop_after_queued, now_rfc3339(), p.threshold],
    )?;
    Ok(id)
}

pub fn update_progress(conn: &Connection, id: &str, scanned: i64, queued: i64) -> AppResult<()> {
    conn.execute("UPDATE scout_sessions SET posts_scanned = ?2, posts_queued = ?3 WHERE id = ?1", params![id, scanned, queued])?;
    Ok(())
}

pub fn finish_session(conn: &Connection, id: &str, reason: &str, summary_json: &str) -> AppResult<()> {
    conn.execute("UPDATE scout_sessions SET ended_at = ?2, ended_reason = ?3, summary_json = ?4 WHERE id = ?1", params![id, now_rfc3339(), reason, summary_json])?;
    Ok(())
}

/// Closes sessions left open by a crash or force-quit so the UI never shows a ghost run.
pub fn close_orphans(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute("UPDATE scout_sessions SET ended_at = ?1, ended_reason = 'interrupted' WHERE ended_at IS NULL", [now_rfc3339()])?)
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub ended_reason: Option<String>,
    pub max_duration_sec: i64,
    pub max_posts: i64,
    pub stop_after_queued: Option<i64>,
    pub threshold: f64,
    pub posts_scanned: i64,
    pub posts_queued: i64,
    pub summary: Option<serde_json::Value>,
}

pub fn recent_sessions(conn: &Connection, profile_id: &str, limit: i64) -> AppResult<Vec<SessionView>> {
    let mut st = conn.prepare(
        "SELECT id, started_at, ended_at, ended_reason, max_duration_sec, max_posts, stop_after_queued, threshold, posts_scanned, posts_queued, summary_json
         FROM scout_sessions WHERE profile_id = ?1 ORDER BY started_at DESC, rowid DESC LIMIT ?2",
    )?;
    let rows = st.query_map(params![profile_id, limit], |r| {
        let summary: Option<String> = r.get(10)?;
        Ok(SessionView {
            id: r.get(0)?,
            started_at: r.get(1)?,
            ended_at: r.get(2)?,
            ended_reason: r.get(3)?,
            max_duration_sec: r.get(4)?,
            max_posts: r.get(5)?,
            stop_after_queued: r.get(6)?,
            threshold: r.get(7)?,
            posts_scanned: r.get(8)?,
            posts_queued: r.get(9)?,
            summary: summary.and_then(|s| serde_json::from_str(&s).ok()),
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueItem {
    pub evaluation_id: String,
    pub session_id: Option<String>,
    pub post: PostView,
    pub score: f64,
    pub outlier_score: Option<f64>,
    pub breakout_flags: Vec<String>,
    pub author_followers: Option<i64>,
    pub author_verified: bool,
    pub angle: String,
    pub reasoning: String,
    pub model_used: String,
    pub status: String,
    pub metrics: Metrics,
    pub posted_at: Option<String>,
    pub draft_id: Option<String>,
    pub draft_text: Option<String>,
    pub draft_status: Option<String>,
    pub draft_error: Option<String>,
    pub draft_published_url: Option<String>,
    pub created_at: String,
}

/// Queue entries at or above `threshold`, newest-best first. Entries older than 24 hours expire (SCT-16).
pub fn queue(conn: &Connection, profile_id: &str, threshold: f64, now_unix: i64, limit: i64) -> AppResult<Vec<QueueItem>> {
    let cutoff = format_unix(now_unix - QUEUE_TTL_SECS);
    let sql = format!(
        "SELECT e.id, e.session_id, e.potential_score, e.angle_summary, e.reasoning, e.model_used, e.status, e.created_at,
                o.id, o.platform_post_id, o.author_handle, o.text, o.has_media, o.metrics_at_capture_json, o.posted_at,
                e.outlier_score, e.breakout_flags_json, o.author_followers, o.author_verified, o.media_json,
                d.id, COALESCE(d.edited_text, d.initial_text), d.status, d.error_message, d.published_url,
                o.author_display_name, o.author_avatar_url
         FROM scout_evaluations e
         JOIN observed_posts o ON o.id = e.observed_post_id
         LEFT JOIN reply_drafts d ON d.id = (
             SELECT d2.id FROM reply_drafts d2
             WHERE d2.evaluation_id = e.id AND d2.status <> 'discarded'
             ORDER BY d2.created_at DESC LIMIT 1
         )
         WHERE o.profile_id = ?1 AND e.status IN ('queued', 'drafted') AND e.potential_score >= ?2 AND e.created_at >= ?3
           AND NOT {ENGAGED_PREDICATE}
         ORDER BY e.potential_score DESC, e.created_at DESC LIMIT ?4"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map(params![profile_id, threshold, cutoff, limit], |r| {
        let (handle, pid): (String, String) = (r.get(10)?, r.get(9)?);
        let metrics: String = r.get(13)?;
        let flags_raw: String = r.get::<_, Option<String>>(16)?.unwrap_or_else(|| "[]".into());
        let media_raw: String = r.get::<_, Option<String>>(19)?.unwrap_or_else(|| "[]".into());
        let media: Vec<MediaItem> = serde_json::from_str(&media_raw).unwrap_or_default();
        Ok(QueueItem {
            evaluation_id: r.get(0)?,
            session_id: r.get(1)?,
            score: r.get(2)?,
            outlier_score: r.get(15)?,
            breakout_flags: serde_json::from_str(&flags_raw).unwrap_or_default(),
            author_followers: r.get(17)?,
            author_verified: r.get::<_, Option<i64>>(18)?.unwrap_or(0) != 0,
            angle: r.get(3)?,
            reasoning: r.get(4)?,
            model_used: r.get(5)?,
            status: r.get(6)?,
            created_at: r.get(7)?,
            post: PostView {
                observed_id: r.get(8)?,
                url: post_url(&handle, &pid),
                platform_post_id: pid,
                handle,
                author_name: r.get::<_, Option<String>>(25)?.unwrap_or_default(),
                avatar_url: r.get(26)?,
                text: r.get(11)?,
                has_media: r.get::<_, i64>(12)? != 0 || !media.is_empty(),
                media,
            },
            metrics: serde_json::from_str(&metrics).unwrap_or_default(),
            posted_at: r.get(14)?,
            draft_id: r.get(20)?,
            draft_text: r.get(21)?,
            draft_status: r.get(22)?,
            draft_error: r.get(23)?,
            draft_published_url: r.get(24)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn dismiss(conn: &Connection, evaluation_id: &str, reason: &str) -> AppResult<()> {
    if !DISMISS_REASONS.contains(&reason) {
        return Err(AppError::Validation(format!("unknown dismiss reason \"{reason}\"")));
    }
    let n = conn.execute("UPDATE scout_evaluations SET status = 'dismissed', dismiss_reason = ?2 WHERE id = ?1 AND status IN ('queued', 'drafted')", params![evaluation_id, reason])?;
    if n == 0 {
        return Err(AppError::Validation("that queue entry is already handled".into()));
    }
    conn.execute("UPDATE reply_drafts SET status = 'discarded' WHERE evaluation_id = ?1 AND status IN ('draft', 'failed')", [evaluation_id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::repo::testutil::*;

    fn params_ok() -> SessionParams {
        SessionParams { feed_source: "for_you".into(), max_duration_sec: 600, max_posts: 50, stop_after_queued: None, threshold: 72.0 }
    }

    #[test]
    fn params_require_sane_manual_values() {
        assert!(params_ok().validate().is_ok());
        assert!(SessionParams { max_duration_sec: 5, ..params_ok() }.validate().is_err());
        assert!(SessionParams { max_posts: 0, ..params_ok() }.validate().is_err());
        assert!(SessionParams { stop_after_queued: Some(0), ..params_ok() }.validate().is_err());
        assert!(SessionParams { threshold: 101.0, ..params_ok() }.validate().is_err());
        assert!(SessionParams { feed_source: "timeline".into(), ..params_ok() }.validate().is_err());
        assert!(SessionParams { feed_source: "following".into(), ..params_ok() }.validate().is_ok());
        assert!(SessionParams { feed_source: "all".into(), ..params_ok() }.validate().is_ok());
    }

    #[test]
    fn threshold_defaults_and_persists() {
        let (conn, _) = db();
        assert_eq!(get_threshold(&conn).unwrap(), DEFAULT_THRESHOLD);
        set_threshold(&conn, 60.0).unwrap();
        assert_eq!(get_threshold(&conn).unwrap(), 60.0);
        assert!(set_threshold(&conn, -1.0).is_err());
    }

    #[test]
    fn last_used_budget_is_remembered() {
        let (conn, _) = db();
        assert!(last_params(&conn).unwrap().is_none());
        let p = SessionParams { feed_source: "both".into(), max_duration_sec: 900, max_posts: 200, stop_after_queued: Some(5), threshold: 65.0 };
        save_last_params(&conn, &p).unwrap();
        let got = last_params(&conn).unwrap().expect("stored");
        assert_eq!((got.feed_source.as_str(), got.max_duration_sec, got.max_posts, got.stop_after_queued), ("both", 900, 200, Some(5)));
        set_setting(&conn, "scout_last_params", "{not json").unwrap();
        assert!(last_params(&conn).unwrap().is_none(), "a corrupt value falls back to the defaults");
    }

    #[test]
    fn default_rubric_is_stored_once_as_version_one() {
        let (conn, profile) = db();
        let (v, text) = ensure_default_rubric(&conn, &profile).unwrap();
        assert_eq!(text, crate::scout::score::DEFAULT_RUBRIC);
        let (v2, _) = ensure_default_rubric(&conn, &profile).unwrap();
        assert_eq!(v, v2);
        let n: i64 = conn.query_row("SELECT count(*) FROM learning_rubrics", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // Learning builds on the stored v1: the first change creates v2 and keeps v1 restorable.
        let patch = crate::learning::patch::Patch {
            target: crate::learning::patch::PatchTarget::Rubric,
            ops: vec![crate::learning::patch::PatchOp::AddCriterion { text: "Prefer fresh posts".into() }],
        };
        let ch = crate::learning::patch::apply(&conn, &profile, &patch, None).unwrap();
        assert_eq!((ch.from_version, ch.to_version), (Some(v), Some(v + 1)));
        let n: i64 = conn.query_row("SELECT count(*) FROM learning_rubrics", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 2, "no second copy of the default is inserted");
    }

    #[test]
    fn queue_applies_threshold_at_read_time_and_ttl() {
        let (conn, profile) = db();
        let s = create_session(&conn, &profile, &params_ok()).unwrap();
        for (pid, score) in [("1", 90.0), ("2", 75.0), ("3", 50.0)] {
            let o = post(&conn, &profile, pid, "a", "t");
            insert_evaluation(&conn, &o, &s, 0, "m", score, "angle", "why").unwrap();
        }
        let now = crate::scout::model::parse_rfc3339_unix(&now_rfc3339()).unwrap();
        let q = queue(&conn, &profile, 72.0, now, 50).unwrap();
        assert_eq!(q.iter().map(|i| i.score).collect::<Vec<_>>(), vec![90.0, 75.0]);
        assert_eq!(queue(&conn, &profile, 40.0, now, 50).unwrap().len(), 3, "lowering the threshold reveals stored entries");
        assert!(queue(&conn, &profile, 40.0, now + 2 * QUEUE_TTL_SECS, 50).unwrap().is_empty(), "entries expire after 24h");
    }

    #[test]
    fn dismiss_validates_reason_and_hides_entry() {
        let (conn, profile) = db();
        let o = post(&conn, &profile, "1", "a", "t");
        let e = evaluation(&conn, &o, 90.0);
        assert!(dismiss(&conn, &e, "because").is_err());
        dismiss(&conn, &e, "off_topic").unwrap();
        assert!(dismiss(&conn, &e, "off_topic").is_err());
        let now = crate::scout::model::parse_rfc3339_unix(&now_rfc3339()).unwrap();
        assert!(queue(&conn, &profile, 0.0, now, 10).unwrap().is_empty());
    }

    #[test]
    fn evaluation_is_unique_per_post_and_seen_check_works() {
        let (conn, profile) = db();
        let s = create_session(&conn, &profile, &params_ok()).unwrap();
        let o = post(&conn, &profile, "9", "a", "t");
        assert!(!has_evaluation(&conn, &profile, "9"));
        insert_evaluation(&conn, &o, &s, 0, "m", 80.0, "", "").unwrap();
        insert_evaluation(&conn, &o, &s, 0, "m", 10.0, "", "").unwrap();
        assert!(has_evaluation(&conn, &profile, "9"));
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM scout_evaluations", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    fn queued_ids(conn: &Connection, profile: &str) -> Vec<String> {
        let now = crate::scout::model::parse_rfc3339_unix(&now_rfc3339()).unwrap();
        queue(conn, profile, 0.0, now, 50).unwrap().into_iter().map(|q| q.post.platform_post_id).collect()
    }

    #[test]
    fn replied_posts_never_reappear_in_the_queue_but_open_ones_do() {
        let (conn, profile) = db();
        let open = post(&conn, &profile, "10", "a", "open");
        evaluation(&conn, &open, 80.0);

        let via_draft = post(&conn, &profile, "11", "b", "replied via draft");
        let e = evaluation(&conn, &via_draft, 90.0);
        let d = crate::drafts::repo::create(&conn, &profile, &via_draft, Some(&e)).unwrap();
        conn.execute("UPDATE reply_drafts SET status = 'published', actions_json = '[\"like\",\"reply\"]' WHERE id = ?1", [&d]).unwrap();

        let via_event = post(&conn, &profile, "12", "c", "replied via event");
        evaluation(&conn, &via_event, 85.0);
        crate::publish::repo::record_event(&conn, &profile, "12", "reply", "publish", true).unwrap();

        let quoted = post(&conn, &profile, "13", "d", "quoted");
        evaluation(&conn, &quoted, 70.0);
        crate::publish::repo::record_event(&conn, &profile, "13", "quote", "action_click", true).unwrap();

        let failed = post(&conn, &profile, "14", "e", "failed reply attempt");
        evaluation(&conn, &failed, 60.0);
        crate::publish::repo::record_event(&conn, &profile, "14", "reply", "publish", false).unwrap();

        let mut ids = queued_ids(&conn, &profile);
        ids.sort();
        assert_eq!(ids, vec!["10", "14"]);
    }

    #[test]
    fn already_engaged_covers_events_and_published_drafts_only_for_reply_or_quote() {
        let (conn, profile) = db();
        assert!(!already_engaged(&conn, &profile, "20"));
        crate::publish::repo::record_event(&conn, &profile, "20", "like", "action_click", true).unwrap();
        assert!(!already_engaged(&conn, &profile, "20"), "a like is not a reply");
        crate::publish::repo::record_event(&conn, &profile, "20", "reply", "publish", false).unwrap();
        assert!(!already_engaged(&conn, &profile, "20"), "a failed reply does not count");
        crate::publish::repo::record_event(&conn, &profile, "20", "reply", "publish", true).unwrap();
        assert!(already_engaged(&conn, &profile, "20"));

        let o = post(&conn, &profile, "21", "a", "t");
        assert!(!already_engaged(&conn, &profile, "21"));
        let d = crate::drafts::repo::create(&conn, &profile, &o, None).unwrap();
        conn.execute("UPDATE reply_drafts SET status = 'published', actions_json = '[\"quote\"]' WHERE id = ?1", [&d]).unwrap();
        assert!(already_engaged(&conn, &profile, "21"));
        assert!(!already_engaged(&conn, "other-profile", "21"));
    }

    #[test]
    fn publishing_marks_every_evaluation_of_the_post_acted() {
        let (conn, profile) = db();
        let o = post(&conn, &profile, "30", "a", "t");
        let e = evaluation(&conn, &o, 80.0);
        let d = crate::drafts::repo::create(&conn, &profile, &o, None).unwrap();
        crate::publish::repo::mark_published(&conn, &d, Some("999"), None).unwrap();
        let st: String = conn.query_row("SELECT status FROM scout_evaluations WHERE id = ?1", [&e], |r| r.get(0)).unwrap();
        assert_eq!(st, "acted");
    }

    #[test]
    fn orphaned_sessions_are_closed() {
        let (conn, profile) = db();
        create_session(&conn, &profile, &params_ok()).unwrap();
        assert_eq!(close_orphans(&conn).unwrap(), 1);
        assert_eq!(recent_sessions(&conn, &profile, 5).unwrap()[0].ended_reason.as_deref(), Some("interrupted"));
    }
}
