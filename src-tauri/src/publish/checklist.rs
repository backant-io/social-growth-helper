//! Repeatable live check of the six engagement actions (DRF-30). Like, bookmark, repost and follow are done
//! and undone on a post the user names, each confirmed from the page state by the same executor the app
//! publishes with. Reply and quote post the user's own words, so the check never sends them: they are
//! reported as manual. The result is stored with the selector registry version so drift shows up fast.

use super::actions::Probe;
use super::executor::JobResult;
use crate::drafts::bundle::ActionKind;
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChecklistItem {
    pub action: String,
    /// `pass`, `fail`, `skipped` (not applicable on this post) or `manual`.
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChecklistReport {
    pub id: String,
    pub registry_version: u32,
    pub created_at: String,
    pub post_url: String,
    pub items: Vec<ChecklistItem>,
}

/// What the check needs from the app. The real one goes through the approval gate and the publish queue.
#[async_trait::async_trait]
pub trait Toggler: Send + Sync {
    async fn probe(&self) -> Result<Option<Probe>, String>;
    async fn toggle(&self, kind: ActionKind, undo: bool) -> JobResult;
}

/// `https://x.com/<handle>/status/<id>` (also twitter.com, with a query string or trailing path).
pub fn parse_post_url(url: &str) -> AppResult<(String, String)> {
    let bad = || AppError::Validation("Paste a post link like https://x.com/name/status/1234567890.".into());
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://")).ok_or_else(bad)?;
    let mut parts = rest.split('/');
    let host = parts.next().unwrap_or("").trim_start_matches("www.").trim_start_matches("mobile.");
    if host != "x.com" && host != "twitter.com" {
        return Err(bad());
    }
    let handle = parts.next().filter(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')).ok_or_else(bad)?;
    if parts.next() != Some("status") {
        return Err(bad());
    }
    let id: String = parts.next().unwrap_or("").chars().take_while(|c| c.is_ascii_digit()).collect();
    if id.is_empty() {
        return Err(bad());
    }
    Ok((handle.to_string(), id))
}

fn state_of(p: &Probe, kind: ActionKind) -> &str {
    match kind {
        ActionKind::Like => &p.like,
        ActionKind::Bookmark => &p.bookmark,
        ActionKind::Repost => &p.repost,
        _ => &p.follow,
    }
}

fn item(action: &str, status: &str, detail: impl Into<String>) -> ChecklistItem {
    ChecklistItem { action: action.into(), status: status.into(), detail: detail.into() }
}

/// Runs the toggles in a fixed order. Each ends in the state it started in.
pub async fn run(t: &dyn Toggler) -> Vec<ChecklistItem> {
    let mut items = vec![];
    for kind in [ActionKind::Like, ActionKind::Bookmark, ActionKind::Repost, ActionKind::Follow] {
        let name = kind.as_str();
        let probe = match t.probe().await {
            Ok(Some(p)) => p,
            Ok(None) => {
                items.push(item(name, "fail", "The X window is closed or the post was not found."));
                break;
            }
            Err(e) => {
                items.push(item(name, "fail", e));
                break;
            }
        };
        let start = state_of(&probe, kind).to_string();
        match start.as_str() {
            "done" | "todo" => {}
            "missing" | "unknown" if kind == ActionKind::Follow => {
                items.push(item(name, "skipped", "No follow button on this post (your own post, or already followed from the menu only)."));
                continue;
            }
            _ => {
                items.push(item(name, "fail", format!("The {name} button was not found on this post. Its selector may have changed.")));
                continue;
            }
        }
        let first_undo = start == "done";
        let a = t.toggle(kind, first_undo).await;
        if !a.ok {
            items.push(item(name, "fail", a.message.unwrap_or_else(|| "The first step failed.".into())));
            continue;
        }
        let b = t.toggle(kind, !first_undo).await;
        items.push(if b.ok {
            item(name, "pass", if first_undo { "Removed and restored." } else { "Done and undone." })
        } else {
            item(name, "fail", format!("Could not restore the original state: {}", b.message.unwrap_or_default()))
        });
    }
    for kind in ["reply", "quote"] {
        items.push(item(kind, "manual", "Posts your own words, so it is never sent by this check. Confirm it once with a real reply."));
    }
    items
}

pub fn save(conn: &Connection, profile_id: &str, post_url: &str, items: &[ChecklistItem]) -> AppResult<ChecklistReport> {
    let report = ChecklistReport { id: new_id(), registry_version: crate::selectors::REGISTRY_VERSION, created_at: now_rfc3339(), post_url: post_url.to_string(), items: items.to_vec() };
    conn.execute(
        "INSERT INTO live_check_reports (id, profile_id, registry_version, post_url, items_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![report.id, profile_id, report.registry_version, report.post_url, serde_json::to_string(&report.items).unwrap_or_default(), report.created_at],
    )?;
    let failed = items.iter().filter(|i| i.status == "fail").count();
    let title = if failed == 0 { "Live action check passed".to_string() } else { format!("Live action check: {failed} failed") };
    crate::activity::record(conn, profile_id, "action", &title, Some(post_url), Some(failed == 0), None);
    Ok(report)
}

pub fn list(conn: &Connection, profile_id: &str, limit: i64) -> AppResult<Vec<ChecklistReport>> {
    let mut stmt = conn.prepare("SELECT id, registry_version, post_url, items_json, created_at FROM live_check_reports WHERE profile_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT ?2")?;
    let rows = stmt.query_map(params![profile_id, limit], |r| {
        Ok(ChecklistReport {
            id: r.get(0)?,
            registry_version: r.get(1)?,
            post_url: r.get(2)?,
            items: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default(),
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.filter_map(Result::ok).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_links_are_parsed_strictly() {
        assert_eq!(parse_post_url("https://x.com/naval/status/123?s=20").unwrap(), ("naval".into(), "123".into()));
        assert_eq!(parse_post_url(" https://www.twitter.com/a_b/status/9/photo/1 ").unwrap(), ("a_b".into(), "9".into()));
        for bad in ["", "x.com/naval/status/1", "https://example.com/naval/status/1", "https://x.com/naval", "https://x.com/naval/status/", "https://x.com/na val/status/1", "https://x.com/i/lists/5"] {
            assert!(parse_post_url(bad).is_err(), "{bad}");
        }
    }
}
