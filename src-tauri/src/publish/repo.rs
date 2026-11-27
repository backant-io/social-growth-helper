use crate::drafts::bundle::ActionKind;
use crate::error::AppResult;
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

pub fn record_event(conn: &Connection, profile_id: &str, platform_post_id: &str, action: &str, source: &str, ok: bool) -> AppResult<()> {
    conn.execute(
        "INSERT INTO action_events (id, profile_id, platform_post_id, action, source, ok, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![new_id(), profile_id, platform_post_id, action, source, ok as i64, now_rfc3339()],
    )?;
    Ok(())
}

/// The observed-post row a check or action on a pasted link hangs off. Never evaluated, so it stays out of the queue.
pub fn ensure_observed(conn: &Connection, profile_id: &str, handle: &str, post_id: &str) -> AppResult<String> {
    if let Some(id) = conn
        .query_row("SELECT id FROM observed_posts WHERE profile_id = ?1 AND platform_post_id = ?2", params![profile_id, post_id], |r| r.get::<_, String>(0))
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, text, has_media, feed_source, first_seen_at, metrics_at_capture_json)
         VALUES (?1, ?2, ?3, ?4, '', 0, 'checklist', ?5, '{}')",
        params![id, profile_id, post_id, handle, now_rfc3339()],
    )?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub fn write_audit(
    conn: &Connection,
    profile_id: &str,
    kind: &str,
    draft_id: Option<&str>,
    approval_ref: &str,
    executed: &[String],
    before: Option<&str>,
    after: Option<&str>,
    success: bool,
    error: Option<&str>,
) -> AppResult<()> {
    conn.execute(
        "INSERT INTO publish_audit_log (id, profile_id, target_kind, target_draft_id, approval_ref, actions_executed_json,
            before_screenshot_path, after_screenshot_path, success, error_message, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![new_id(), profile_id, kind, draft_id, approval_ref, serde_json::to_string(executed).unwrap_or_default(), before, after, success as i64, error, now_rfc3339()],
    )?;
    Ok(())
}

pub fn mark_approved(conn: &Connection, draft_id: &str) -> AppResult<()> {
    conn.execute("UPDATE reply_drafts SET user_approved_at = ?2, status = 'queued_for_publish', error_message = NULL WHERE id = ?1", params![draft_id, now_rfc3339()])?;
    Ok(())
}

pub fn mark_published(conn: &Connection, draft_id: &str, post_id: Option<&str>, url: Option<&str>) -> AppResult<()> {
    conn.execute(
        "UPDATE reply_drafts SET status = 'published', published_at = ?2, published_post_id = ?3, published_url = ?4, error_message = NULL WHERE id = ?1",
        params![draft_id, now_rfc3339(), post_id, url],
    )?;
    conn.execute(
        "UPDATE scout_evaluations SET status = 'acted'
         WHERE status IN ('queued', 'drafted')
           AND (id = (SELECT evaluation_id FROM reply_drafts WHERE id = ?1)
                OR observed_post_id = (SELECT observed_post_id FROM reply_drafts WHERE id = ?1))",
        [draft_id],
    )?;
    Ok(())
}

/// Anything still queued or mid-publish when the app starts was interrupted. It is not re-run (approval
/// was for that session); it becomes retryable so the user decides.
pub fn recover_interrupted(conn: &Connection) -> AppResult<usize> {
    Ok(conn.execute(
        "UPDATE reply_drafts SET status = 'failed', error_message = 'Not confirmed yet'
         WHERE status IN ('queued_for_publish', 'publishing')",
        [],
    )?)
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionState {
    pub like: bool,
    pub bookmark: bool,
    pub repost: bool,
    pub follow: bool,
    pub replied: bool,
    pub quoted: bool,
}

/// Best-known state of the five reversible/terminal actions on a post, from this app's own event log.
/// The Studio refines it with a live page probe when the window is open.
pub fn known_state(conn: &Connection, profile_id: &str, platform_post_id: &str) -> AppResult<ActionState> {
    let last = |on: &str, off: &str| -> AppResult<bool> {
        let row: Option<String> = conn
            .query_row(
                "SELECT action FROM action_events WHERE profile_id = ?1 AND platform_post_id = ?2 AND ok = 1 AND action IN (?3, ?4)
                 ORDER BY created_at DESC, rowid DESC LIMIT 1",
                params![profile_id, platform_post_id, on, off],
                |r| r.get(0),
            )
            .optional()?;
        Ok(row.as_deref() == Some(on))
    };
    let once = |a: &str| -> AppResult<bool> {
        Ok(conn
            .query_row("SELECT 1 FROM action_events WHERE profile_id = ?1 AND platform_post_id = ?2 AND ok = 1 AND action = ?3 LIMIT 1", params![profile_id, platform_post_id, a], |_| Ok(()))
            .optional()?
            .is_some())
    };
    Ok(ActionState {
        like: last("like", "unlike")?,
        bookmark: last("bookmark", "unbookmark")?,
        repost: last("repost", "unrepost")?,
        follow: last("follow", "unfollow")?,
        replied: once("reply")?,
        quoted: once("quote")?,
    })
}

pub fn event_name(kind: ActionKind, undo: bool) -> String {
    if undo {
        format!("un{}", kind.as_str())
    } else {
        kind.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::repo::testutil::db;

    #[test]
    fn known_state_follows_the_latest_toggle() {
        let (conn, profile) = db();
        record_event(&conn, &profile, "1", "like", "action_click", true).unwrap();
        assert!(known_state(&conn, &profile, "1").unwrap().like);
        record_event(&conn, &profile, "1", "unlike", "action_click", true).unwrap();
        assert!(!known_state(&conn, &profile, "1").unwrap().like);
        record_event(&conn, &profile, "1", "follow", "publish", false).unwrap();
        assert!(!known_state(&conn, &profile, "1").unwrap().follow, "failed events do not count");
        record_event(&conn, &profile, "1", "reply", "publish", true).unwrap();
        assert!(known_state(&conn, &profile, "1").unwrap().replied);
        assert!(!known_state(&conn, &profile, "2").unwrap().replied);
    }

    #[test]
    fn interrupted_publishes_become_retryable_not_rerun() {
        let (conn, profile) = db();
        let o = crate::drafts::repo::testutil::post(&conn, &profile, "1", "a", "t");
        let d = crate::drafts::repo::create(&conn, &profile, &o, None).unwrap();
        crate::drafts::repo::set_status(&conn, &d, "publishing", None).unwrap();
        assert_eq!(recover_interrupted(&conn).unwrap(), 1);
        let v = crate::drafts::repo::get_view(&conn, &d).unwrap();
        assert_eq!(v.status, "failed");
        assert_eq!(v.error_message.as_deref(), Some("Not confirmed yet"));
    }
}
