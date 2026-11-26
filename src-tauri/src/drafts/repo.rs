use super::bundle::{self, ActionKind};
use super::edit::edit_distance;
use super::generate::{StyleContext, Variant};
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PostView {
    pub observed_id: String,
    pub platform_post_id: String,
    pub handle: String,
    pub text: String,
    pub has_media: bool,
    pub url: String,
}

/// Quote and thread context captured with the post (SCT-20), if any.
pub fn post_context(conn: &Connection, observed_id: &str) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT context_json FROM observed_posts WHERE id = ?1", [observed_id], |r| r.get::<_, Option<String>>(0))
        .optional()?
        .flatten())
}

#[derive(Debug, Clone, Serialize)]
pub struct VariantView {
    pub id: String,
    pub text: String,
    pub tone: Option<String>,
    pub model_used: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepView {
    pub action: String,
    pub status: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub id: String,
    pub evaluation_id: Option<String>,
    pub post: PostView,
    pub angle: Option<String>,
    pub reasoning: Option<String>,
    pub score: Option<f64>,
    pub kind: String,
    pub initial_text: String,
    pub edited_text: String,
    pub edit_distance: i64,
    pub tone_tag: Option<String>,
    pub model_used: String,
    pub actions: Vec<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub published_url: Option<String>,
    pub variants: Vec<VariantView>,
    pub steps: Vec<StepView>,
    pub created_at: String,
}

pub fn post_url(handle: &str, id: &str) -> String {
    format!("https://x.com/{handle}/status/{id}")
}

pub fn load_style(conn: &Connection, profile_id: &str) -> AppResult<StyleContext> {
    let row = conn
        .query_row(
            "SELECT version, free_text_guide, banned_patterns_json FROM style_guides
             WHERE profile_id = ?1 AND is_active = 1 ORDER BY version DESC LIMIT 1",
            [profile_id],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((version, guide, banned)) => StyleContext { version, guide, banned: serde_json::from_str(&banned).unwrap_or_default() },
        None => StyleContext::default(),
    })
}

pub fn post_by_observed(conn: &Connection, observed_id: &str) -> AppResult<PostView> {
    conn.query_row(
        "SELECT id, platform_post_id, author_handle, text, has_media FROM observed_posts WHERE id = ?1",
        [observed_id],
        |r| {
            let (handle, pid): (String, String) = (r.get(2)?, r.get(1)?);
            Ok(PostView { observed_id: r.get(0)?, url: post_url(&handle, &pid), platform_post_id: pid, handle, text: r.get(3)?, has_media: r.get::<_, i64>(4)? != 0 })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::Validation("that post is no longer stored".into()))
}

/// One live draft per evaluation. Returns the existing one when present.
pub fn create_for_evaluation(conn: &Connection, profile_id: &str, evaluation_id: &str) -> AppResult<String> {
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM reply_drafts WHERE evaluation_id = ?1 AND status <> 'discarded' ORDER BY created_at DESC LIMIT 1",
            [evaluation_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let observed: String = conn
        .query_row("SELECT observed_post_id FROM scout_evaluations WHERE id = ?1", [evaluation_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::Validation("that queue entry no longer exists".into()))?;
    create(conn, profile_id, &observed, Some(evaluation_id))
}

/// A draft for a post that has no evaluation (for example a direct engagement from the Studio).
pub fn create(conn: &Connection, profile_id: &str, observed_post_id: &str, evaluation_id: Option<&str>) -> AppResult<String> {
    let style = load_style(conn, profile_id)?;
    let id = new_id();
    conn.execute(
        "INSERT INTO reply_drafts (id, profile_id, observed_post_id, evaluation_id, initial_text, edited_text, model_used,
            style_guide_version, draft_kind, actions_json, status, created_at)
         VALUES (?1, ?2, ?3, ?4, '', '', '', ?5, 'reply', '[\"like\",\"reply\"]', 'draft', ?6)",
        params![id, profile_id, observed_post_id, evaluation_id, style.version, now_rfc3339()],
    )?;
    if let Some(e) = evaluation_id {
        conn.execute("UPDATE scout_evaluations SET status = 'drafted' WHERE id = ?1 AND status = 'queued'", [e])?;
    }
    Ok(id)
}

fn is_editable(status: &str) -> bool {
    matches!(status, "draft" | "failed")
}

fn status_of(conn: &Connection, draft_id: &str) -> AppResult<String> {
    conn.query_row("SELECT status FROM reply_drafts WHERE id = ?1", [draft_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::Validation("draft not found".into()))
}

fn require_editable(conn: &Connection, draft_id: &str) -> AppResult<()> {
    let st = status_of(conn, draft_id)?;
    if !is_editable(&st) {
        return Err(AppError::Validation(format!("this draft is {st} and can no longer be changed")));
    }
    Ok(())
}

/// Stores generated variants, selects the first when the draft has no text yet.
pub fn add_variants(conn: &Connection, draft_id: &str, variants: &[Variant], model: &str) -> AppResult<Vec<String>> {
    let now = now_rfc3339();
    let mut ids = vec![];
    for v in variants {
        let id = new_id();
        conn.execute(
            "INSERT INTO draft_variants (id, draft_id, text, tone, model_used, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, draft_id, v.text, v.tone, model, now],
        )?;
        ids.push(id);
    }
    let empty: bool = conn.query_row("SELECT edited_text = '' FROM reply_drafts WHERE id = ?1", [draft_id], |r| r.get(0))?;
    if empty {
        if let Some(first) = variants.first() {
            conn.execute(
                "UPDATE reply_drafts SET initial_text = ?2, edited_text = ?2, edit_distance = 0, tone_tag = ?3, model_used = ?4 WHERE id = ?1",
                params![draft_id, first.text, first.tone, model],
            )?;
        }
    }
    Ok(ids)
}

pub fn select_variant(conn: &Connection, draft_id: &str, variant_id: &str) -> AppResult<()> {
    require_editable(conn, draft_id)?;
    let (text, tone, model): (String, Option<String>, String) = conn
        .query_row("SELECT text, tone, model_used FROM draft_variants WHERE id = ?1 AND draft_id = ?2", params![variant_id, draft_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .optional()?
        .ok_or_else(|| AppError::Validation("variant not found".into()))?;
    conn.execute(
        "UPDATE reply_drafts SET initial_text = ?2, edited_text = ?2, edit_distance = 0, tone_tag = ?3, model_used = ?4 WHERE id = ?1",
        params![draft_id, text, tone, model],
    )?;
    Ok(())
}

/// Saves the user's edits. The distance to the generated text is kept current (DRF-07).
pub fn save_edit(conn: &Connection, draft_id: &str, text: &str) -> AppResult<i64> {
    require_editable(conn, draft_id)?;
    let initial: String = conn.query_row("SELECT initial_text FROM reply_drafts WHERE id = ?1", [draft_id], |r| r.get(0))?;
    let distance = edit_distance(&initial, text) as i64;
    conn.execute("UPDATE reply_drafts SET edited_text = ?2, edit_distance = ?3 WHERE id = ?1", params![draft_id, text, distance])?;
    Ok(distance)
}

pub fn set_actions(conn: &Connection, draft_id: &str, actions: &[String]) -> AppResult<Vec<ActionKind>> {
    require_editable(conn, draft_id)?;
    let normalized = bundle::normalize(actions)?;
    let names: Vec<&str> = normalized.iter().map(|a| a.as_str()).collect();
    conn.execute(
        "UPDATE reply_drafts SET actions_json = ?2, draft_kind = ?3 WHERE id = ?1",
        params![draft_id, serde_json::to_string(&names).unwrap_or_default(), bundle::draft_kind(&normalized)],
    )?;
    Ok(normalized)
}

pub fn set_status(conn: &Connection, draft_id: &str, status: &str, error: Option<&str>) -> AppResult<()> {
    conn.execute("UPDATE reply_drafts SET status = ?2, error_message = ?3 WHERE id = ?1", params![draft_id, status, error])?;
    Ok(())
}

pub fn discard(conn: &Connection, draft_id: &str, reason: Option<&str>) -> AppResult<()> {
    let st = status_of(conn, draft_id)?;
    if matches!(st.as_str(), "publishing" | "queued_for_publish" | "published") {
        return Err(AppError::Validation(format!("this draft is {st} and cannot be discarded")));
    }
    conn.execute("UPDATE reply_drafts SET status = 'discarded' WHERE id = ?1", [draft_id])?;
    conn.execute(
        "UPDATE scout_evaluations SET status = 'dismissed', dismiss_reason = COALESCE(?2, dismiss_reason, 'discarded_draft')
         WHERE id = (SELECT evaluation_id FROM reply_drafts WHERE id = ?1)",
        params![draft_id, reason],
    )?;
    Ok(())
}

pub fn bundle_of(conn: &Connection, draft_id: &str) -> AppResult<Vec<ActionKind>> {
    let raw: String = conn
        .query_row("SELECT actions_json FROM reply_drafts WHERE id = ?1", [draft_id], |r| r.get(0))
        .optional()?
        .ok_or_else(|| AppError::Validation("draft not found".into()))?;
    let names: Vec<String> = serde_json::from_str(&raw).unwrap_or_default();
    bundle::normalize(&names)
}

pub fn recent_published_texts(conn: &Connection, profile_id: &str, limit: i64) -> AppResult<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT edited_text FROM reply_drafts WHERE profile_id = ?1 AND status = 'published' AND edited_text <> ''
         ORDER BY published_at DESC LIMIT ?2",
    )?;
    let rows = st.query_map(params![profile_id, limit], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn get_view(conn: &Connection, draft_id: &str) -> AppResult<DraftView> {
    let mut view = conn
        .query_row(
            "SELECT d.id, d.evaluation_id, d.observed_post_id, d.draft_kind, d.initial_text, d.edited_text, d.edit_distance, d.tone_tag,
                    d.model_used, d.actions_json, d.status, d.error_message, d.published_url, d.created_at,
                    e.angle_summary, e.reasoning, e.potential_score
             FROM reply_drafts d LEFT JOIN scout_evaluations e ON e.id = d.evaluation_id WHERE d.id = ?1",
            [draft_id],
            |r| {
                let actions: String = r.get(9)?;
                Ok((
                    DraftView {
                        id: r.get(0)?,
                        evaluation_id: r.get(1)?,
                        post: PostView { observed_id: r.get(2)?, platform_post_id: String::new(), handle: String::new(), text: String::new(), has_media: false, url: String::new() },
                        kind: r.get(3)?,
                        initial_text: r.get(4)?,
                        edited_text: r.get(5)?,
                        edit_distance: r.get(6)?,
                        tone_tag: r.get(7)?,
                        model_used: r.get(8)?,
                        actions: serde_json::from_str(&actions).unwrap_or_default(),
                        status: r.get(10)?,
                        error_message: r.get(11)?,
                        published_url: r.get(12)?,
                        created_at: r.get(13)?,
                        angle: r.get(14)?,
                        reasoning: r.get(15)?,
                        score: r.get(16)?,
                        variants: vec![],
                        steps: vec![],
                    },
                    (),
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::Validation("draft not found".into()))?
        .0;
    view.post = post_by_observed(conn, &view.post.observed_id)?;
    let mut st = conn.prepare("SELECT id, text, tone, model_used FROM draft_variants WHERE draft_id = ?1 ORDER BY created_at, rowid")?;
    view.variants = st
        .query_map([draft_id], |r| Ok(VariantView { id: r.get(0)?, text: r.get(1)?, tone: r.get(2)?, model_used: r.get(3)? }))?
        .collect::<Result<_, _>>()?;
    view.steps = steps(conn, draft_id)?;
    Ok(view)
}

pub fn steps(conn: &Connection, draft_id: &str) -> AppResult<Vec<StepView>> {
    let mut st = conn.prepare("SELECT action, status, detail FROM draft_steps WHERE draft_id = ?1")?;
    let mut rows: Vec<StepView> = st
        .query_map([draft_id], |r| Ok(StepView { action: r.get(0)?, status: r.get(1)?, detail: r.get(2)? }))?
        .collect::<Result<_, _>>()?;
    rows.sort_by_key(|s| ActionKind::parse(&s.action).ok().and_then(|a| bundle::EXEC_ORDER.iter().position(|x| *x == a)).unwrap_or(99));
    Ok(rows)
}

pub fn upsert_step(conn: &Connection, draft_id: &str, action: ActionKind, status: &str, detail: Option<&str>) -> AppResult<()> {
    conn.execute(
        "INSERT INTO draft_steps (draft_id, action, status, detail, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(draft_id, action) DO UPDATE SET status = ?3, detail = ?4, updated_at = ?5",
        params![draft_id, action.as_str(), status, detail, now_rfc3339()],
    )?;
    Ok(())
}

pub fn step_status(conn: &Connection, draft_id: &str, action: ActionKind) -> AppResult<Option<String>> {
    Ok(conn
        .query_row("SELECT status FROM draft_steps WHERE draft_id = ?1 AND action = ?2", params![draft_id, action.as_str()], |r| r.get(0))
        .optional()?)
}

#[cfg(test)]
pub mod testutil {
    use super::*;
    use crate::storage;

    pub fn db() -> (Connection, String) {
        let conn = storage::open_in_memory().expect("in-memory db");
        let profile = crate::storage::repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        (conn, profile)
    }

    pub fn acknowledge(conn: &Connection) {
        crate::onboarding::acknowledge(conn, crate::onboarding::NOTICE_VERSION, true).unwrap();
    }

    pub fn post(conn: &Connection, profile: &str, pid: &str, handle: &str, text: &str) -> String {
        let id = new_id();
        conn.execute(
            "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, text, has_media, feed_source, first_seen_at, metrics_at_capture_json)
             VALUES (?1, ?2, ?3, ?4, ?5, 0, 'for_you', ?6, '{}')",
            params![id, profile, pid, handle, text, now_rfc3339()],
        )
        .unwrap();
        id
    }

    pub fn evaluation(conn: &Connection, observed: &str, score: f64) -> String {
        let id = new_id();
        conn.execute(
            "INSERT INTO scout_evaluations (id, observed_post_id, rubric_version, model_used, potential_score, angle_summary, reasoning, status, created_at)
             VALUES (?1, ?2, 0, 'scripted', ?3, 'angle', 'because', 'queued', ?4)",
            params![id, observed, score, now_rfc3339()],
        )
        .unwrap();
        id
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn setup() -> (Connection, String, String) {
        let (conn, profile) = db();
        let p = post(&conn, &profile, "111", "naval", "text");
        let e = evaluation(&conn, &p, 80.0);
        (conn, profile, e)
    }

    #[test]
    fn creates_one_live_draft_per_evaluation() {
        let (conn, profile, e) = setup();
        let a = create_for_evaluation(&conn, &profile, &e).unwrap();
        let b = create_for_evaluation(&conn, &profile, &e).unwrap();
        assert_eq!(a, b);
        discard(&conn, &a, None).unwrap();
        assert_ne!(create_for_evaluation(&conn, &profile, &e).unwrap(), a);
    }

    #[test]
    fn variants_select_edit_and_distance() {
        let (conn, profile, e) = setup();
        let d = create_for_evaluation(&conn, &profile, &e).unwrap();
        add_variants(&conn, &d, &[Variant { text: "hello there".into(), tone: "witty".into() }, Variant { text: "yo".into(), tone: "dry".into() }], "m1").unwrap();
        let v = get_view(&conn, &d).unwrap();
        assert_eq!((v.initial_text.as_str(), v.variants.len(), v.model_used.as_str()), ("hello there", 2, "m1"));
        assert_eq!(save_edit(&conn, &d, "hello there!").unwrap(), 1);
        select_variant(&conn, &d, &v.variants[1].id).unwrap();
        let v = get_view(&conn, &d).unwrap();
        assert_eq!((v.edited_text.as_str(), v.edit_distance, v.tone_tag.as_deref()), ("yo", 0, Some("dry")));
    }

    #[test]
    fn bundle_change_updates_kind_and_rejects_two_text_actions() {
        let (conn, profile, e) = setup();
        let d = create_for_evaluation(&conn, &profile, &e).unwrap();
        set_actions(&conn, &d, &["quote".into(), "like".into(), "follow".into()]).unwrap();
        assert_eq!(get_view(&conn, &d).unwrap().kind, "quote");
        assert!(set_actions(&conn, &d, &["reply".into(), "quote".into()]).is_err());
        assert_eq!(bundle_of(&conn, &d).unwrap(), vec![ActionKind::Like, ActionKind::Follow, ActionKind::Quote]);
    }

    #[test]
    fn published_drafts_are_frozen_and_discard_dismisses_the_queue_entry() {
        let (conn, profile, e) = setup();
        let d = create_for_evaluation(&conn, &profile, &e).unwrap();
        discard(&conn, &d, Some("off_topic")).unwrap();
        let (st, reason): (String, String) = conn.query_row("SELECT status, dismiss_reason FROM scout_evaluations WHERE id = ?1", [&e], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!((st.as_str(), reason.as_str()), ("dismissed", "off_topic"));
        let e2 = {
            let p2 = post(&conn, &profile, "222", "a", "t");
            evaluation(&conn, &p2, 70.0)
        };
        let d2 = create_for_evaluation(&conn, &profile, &e2).unwrap();
        set_status(&conn, &d2, "published", None).unwrap();
        assert!(save_edit(&conn, &d2, "late edit").is_err());
        assert!(discard(&conn, &d2, None).is_err());
    }

    #[test]
    fn steps_upsert_in_execution_order() {
        let (conn, profile, e) = setup();
        let d = create_for_evaluation(&conn, &profile, &e).unwrap();
        upsert_step(&conn, &d, ActionKind::Reply, "pending", None).unwrap();
        upsert_step(&conn, &d, ActionKind::Like, "done", None).unwrap();
        upsert_step(&conn, &d, ActionKind::Reply, "failed", Some("boom")).unwrap();
        let s = steps(&conn, &d).unwrap();
        assert_eq!(s.iter().map(|x| (x.action.as_str(), x.status.as_str())).collect::<Vec<_>>(), vec![("like", "done"), ("reply", "failed")]);
        assert_eq!(step_status(&conn, &d, ActionKind::Like).unwrap().as_deref(), Some("done"));
    }
}
