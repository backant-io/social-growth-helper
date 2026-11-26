//! Publish ownership gate (DRF-04, DRF-11, DRF-27). [`Approval`] has private fields and no public
//! constructor: the only ways to obtain one are [`approve_bundle`] and [`approve_action`], which the Tauri
//! commands call when the user clicks Publish or an action button. The executor takes `&Approval`, so no
//! other code path (scout, drafts, a model response, a scraped post) can make X change.
//!
//! Reply and Quote carry text and only ever pass through `approve_bundle`, which requires the exact text
//! the user was shown in the confirmation. Single-action approvals refuse text actions.

use crate::drafts::bundle::{text_action, ActionKind};
use crate::drafts::edit::{pre_publish_problems, DEFAULT_MAX_CHARS};
use crate::drafts::repo;
use crate::error::{AppError, AppResult};
use crate::onboarding::require_acknowledged;
use crate::util::new_id;
use rusqlite::Connection;

#[derive(Debug, Clone)]
enum Scope {
    Bundle { draft_id: String, actions: Vec<ActionKind>, text: String, kind: &'static str },
    Single { action: ActionKind, undo: bool },
}

#[derive(Debug, Clone)]
pub struct Approval {
    ref_id: String,
    scope: Scope,
    platform_post_id: String,
    handle: String,
    post_text: String,
}

impl Approval {
    pub fn ref_id(&self) -> &str {
        &self.ref_id
    }
    pub fn platform_post_id(&self) -> &str {
        &self.platform_post_id
    }
    pub fn handle(&self) -> &str {
        &self.handle
    }
    pub fn post_text(&self) -> &str {
        &self.post_text
    }
    pub fn source(&self) -> &'static str {
        match self.scope {
            Scope::Bundle { .. } => "publish",
            Scope::Single { .. } => "action_click",
        }
    }
    pub fn draft_id(&self) -> Option<&str> {
        match &self.scope {
            Scope::Bundle { draft_id, .. } => Some(draft_id),
            Scope::Single { .. } => None,
        }
    }
    pub fn bundle(&self) -> Vec<ActionKind> {
        match &self.scope {
            Scope::Bundle { actions, .. } => actions.clone(),
            Scope::Single { action, .. } => vec![*action],
        }
    }
    pub fn undo(&self) -> bool {
        matches!(self.scope, Scope::Single { undo: true, .. })
    }
    /// The exact approved text (empty for bundles without a text action).
    pub fn text(&self) -> &str {
        match &self.scope {
            Scope::Bundle { text, .. } => text,
            Scope::Single { .. } => "",
        }
    }
    pub fn kind(&self) -> &'static str {
        match &self.scope {
            Scope::Bundle { kind, .. } => kind,
            Scope::Single { .. } => "single_action",
        }
    }
}

fn require_open(kill_engaged: bool) -> AppResult<()> {
    if kill_engaged {
        return Err(AppError::Validation("Everything is paused (kill switch on). Resume it before publishing.".into()));
    }
    Ok(())
}

/// The user clicked Publish and confirmed `confirmed_text` for `draft_id`.
pub fn approve_bundle(conn: &Connection, profile_id: &str, draft_id: &str, confirmed_text: &str, kill_engaged: bool) -> AppResult<Approval> {
    require_open(kill_engaged)?;
    require_acknowledged(conn)?;
    let view = repo::get_view(conn, draft_id)?;
    if !matches!(view.status.as_str(), "draft" | "failed") {
        return Err(AppError::Validation(format!("this draft is {}; it cannot be published again", view.status)));
    }
    let actions = repo::bundle_of(conn, draft_id)?;
    let text_kind = text_action(&actions);
    if text_kind.is_some() && confirmed_text != view.edited_text {
        return Err(AppError::Validation("The text changed after the confirmation opened. Review it and confirm again.".into()));
    }
    let style = repo::load_style(conn, profile_id)?;
    let recent = repo::recent_published_texts(conn, profile_id, 50)?;
    let problems = pre_publish_problems(&view.edited_text, &actions, DEFAULT_MAX_CHARS, &style.banned, &recent);
    if !problems.is_empty() {
        return Err(AppError::Validation(problems.join(" ")));
    }
    for a in &actions {
        match repo::step_status(conn, draft_id, *a)?.as_deref() {
            Some("done") | Some("skipped") | Some("uncertain") => {}
            _ => repo::upsert_step(conn, draft_id, *a, "pending", None)?,
        }
    }
    super::repo::mark_approved(conn, draft_id)?;
    Ok(Approval {
        ref_id: new_id(),
        platform_post_id: view.post.platform_post_id.clone(),
        handle: view.post.handle.clone(),
        post_text: view.post.text.clone(),
        scope: Scope::Bundle { draft_id: draft_id.to_string(), actions, text: if text_kind.is_some() { view.edited_text } else { String::new() }, kind: crate::drafts::bundle::draft_kind(&repo::bundle_of(conn, draft_id)?) },
    })
}

/// The user clicked one of the no-text action buttons (or its reversal) on a post.
pub fn approve_action(conn: &Connection, observed_post_id: &str, action: ActionKind, undo: bool, kill_engaged: bool) -> AppResult<Approval> {
    require_open(kill_engaged)?;
    require_acknowledged(conn)?;
    if action.is_text() {
        return Err(AppError::Validation("Reply and Quote post your own words, so they only go through Publish.".into()));
    }
    let post = repo::post_by_observed(conn, observed_post_id)?;
    Ok(Approval { ref_id: new_id(), platform_post_id: post.platform_post_id, handle: post.handle, post_text: post.text, scope: Scope::Single { action, undo } })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drafts::repo::testutil::*;

    fn ready() -> (Connection, String, String, String) {
        let (conn, profile) = db();
        acknowledge(&conn);
        let o = post(&conn, &profile, "111", "naval", "a post");
        let d = repo::create(&conn, &profile, &o, None).unwrap();
        repo::save_edit(&conn, &d, "my reply").unwrap();
        (conn, profile, o, d)
    }

    #[test]
    fn publish_requires_the_exact_confirmed_text() {
        let (conn, profile, _, d) = ready();
        let err = approve_bundle(&conn, &profile, &d, "my reply (edited later)", false).unwrap_err().to_string();
        assert!(err.contains("changed after"));
        assert_eq!(repo::get_view(&conn, &d).unwrap().status, "draft", "a refused approval changes nothing");
        let a = approve_bundle(&conn, &profile, &d, "my reply", false).unwrap();
        assert_eq!((a.text(), a.source(), a.kind()), ("my reply", "publish", "reply"));
        assert_eq!(repo::get_view(&conn, &d).unwrap().status, "queued_for_publish");
        assert!(repo::get_view(&conn, &d).unwrap().steps.iter().all(|s| s.status == "pending"));
    }

    #[test]
    fn publish_runs_prechecks_and_respects_the_kill_switch_and_acknowledgement() {
        let (conn, profile, _, d) = ready();
        repo::save_edit(&conn, &d, &"x".repeat(300)).unwrap();
        assert!(approve_bundle(&conn, &profile, &d, &"x".repeat(300), false).unwrap_err().to_string().contains("300 characters"));
        repo::save_edit(&conn, &d, "fine").unwrap();
        assert!(approve_bundle(&conn, &profile, &d, "fine", true).unwrap_err().to_string().contains("paused"));
        let (conn2, profile2) = db();
        let o = post(&conn2, &profile2, "1", "a", "t");
        let d2 = repo::create(&conn2, &profile2, &o, None).unwrap();
        repo::save_edit(&conn2, &d2, "hi").unwrap();
        assert!(matches!(approve_bundle(&conn2, &profile2, &d2, "hi", false), Err(AppError::RulesNotAcknowledged)));
    }

    #[test]
    fn single_actions_refuse_text_and_accept_reversals() {
        let (conn, _, o, _) = ready();
        assert!(approve_action(&conn, &o, ActionKind::Reply, false, false).is_err());
        assert!(approve_action(&conn, &o, ActionKind::Quote, false, false).is_err());
        let a = approve_action(&conn, &o, ActionKind::Like, true, false).unwrap();
        assert!(a.undo() && a.draft_id().is_none() && a.text().is_empty());
        assert_eq!(a.source(), "action_click");
    }

    #[test]
    fn retry_keeps_done_steps_and_resets_failed_ones() {
        let (conn, profile, _, d) = ready();
        repo::set_actions(&conn, &d, &["like".into(), "reply".into()]).unwrap();
        repo::upsert_step(&conn, &d, ActionKind::Like, "done", None).unwrap();
        repo::upsert_step(&conn, &d, ActionKind::Reply, "failed", Some("boom")).unwrap();
        repo::set_status(&conn, &d, "failed", Some("boom")).unwrap();
        approve_bundle(&conn, &profile, &d, "my reply", false).unwrap();
        let s = repo::steps(&conn, &d).unwrap();
        assert_eq!((s[0].status.as_str(), s[1].status.as_str()), ("done", "pending"));
    }
}
