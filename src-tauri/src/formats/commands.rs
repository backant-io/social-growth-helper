//! Tauri commands for the format library, candidates and the creator watchlist.

use super::blueprint::FormatBlueprint;
use super::detect::{PostMetrics, ViralThresholds};
use super::pipeline::{self, BatchSummary, Detection, PostInput, PostSource, Processed};
use super::repo::{self, Candidate, FormatRecord, FormatSort, WatchEntry};
use super::scan;
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::{Deserialize, Serialize};
use tauri::State;

const MAX_PENDING_PER_RUN: usize = 10;

#[tauri::command]
pub fn formats_list(state: State<'_, AppState>, sort: Option<FormatSort>, include_archived: Option<bool>) -> AppResult<Vec<FormatRecord>> {
    let sort = sort.unwrap_or(FormatSort::Working);
    let db = state.db.lock().unwrap();
    repo::list_formats(&db, &state.profile_id, include_archived.unwrap_or(false), sort)
}

#[tauri::command]
pub fn formats_create(state: State<'_, AppState>, blueprint: FormatBlueprint) -> AppResult<FormatRecord> {
    let bp = blueprint.validated(None).map_err(AppError::Validation)?;
    let db = state.db.lock().unwrap();
    repo::insert_format(&db, &state.profile_id, &bp, "manual", None, None)
}

#[tauri::command]
pub fn formats_update(state: State<'_, AppState>, id: String, blueprint: FormatBlueprint) -> AppResult<FormatRecord> {
    let bp = blueprint.validated(None).map_err(AppError::Validation)?;
    let db = state.db.lock().unwrap();
    repo::update_format(&db, &state.profile_id, &id, &bp)
}

#[tauri::command]
pub fn formats_set_status(state: State<'_, AppState>, id: String, status: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    repo::set_format_status(&db, &state.profile_id, &id, &status)
}

#[tauri::command]
pub fn formats_candidates(state: State<'_, AppState>, status: Option<String>) -> AppResult<Vec<Candidate>> {
    let db = state.db.lock().unwrap();
    repo::list_candidates(&db, &state.profile_id, status.as_deref().unwrap_or("pending"), 100)
}

#[tauri::command]
pub fn formats_dismiss_candidate(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    repo::set_candidate_status(&db, &state.profile_id, &id, "dismissed", None, None)
}

#[tauri::command]
pub async fn formats_extract(state: State<'_, AppState>, candidate_id: String) -> AppResult<Processed> {
    pipeline::process_candidate(&state.db, &state.profile_id, state.llm.as_ref(), &candidate_id).await
}

#[tauri::command]
pub async fn formats_extract_pending(state: State<'_, AppState>) -> AppResult<BatchSummary> {
    let _job = state.jobs.register("Studying viral posts");
    pipeline::process_pending(&state.db, &state.profile_id, state.llm.as_ref(), MAX_PENDING_PER_RUN).await
}

#[derive(Deserialize)]
pub struct ManualPost {
    pub url: String,
    pub text: String,
    #[serde(default)]
    pub likes: u64,
    #[serde(default)]
    pub reposts: u64,
    #[serde(default)]
    pub replies: u64,
    #[serde(default)]
    pub bookmarks: u64,
    pub views: Option<u64>,
    pub author_followers: Option<u64>,
}

/// Accepts `https://x.com/<handle>/status/<id>` (also twitter.com, mobile.*, with query strings).
pub fn parse_post_url(url: &str) -> AppResult<(String, String)> {
    let bad = || AppError::Validation("paste a post link like https://x.com/name/status/123456789".into());
    let rest = url.trim().strip_prefix("https://").or_else(|| url.trim().strip_prefix("http://")).ok_or_else(bad)?;
    let (host, path) = rest.split_once('/').ok_or_else(bad)?;
    let host = host.to_lowercase();
    if !matches!(host.as_str(), "x.com" | "www.x.com" | "mobile.x.com" | "twitter.com" | "www.twitter.com" | "mobile.twitter.com") {
        return Err(bad());
    }
    let mut parts = path.split(['?', '#']).next().unwrap_or("").split('/').filter(|p| !p.is_empty());
    let handle = parts.next().ok_or_else(bad)?;
    if handle.eq_ignore_ascii_case("i") {
        return Err(AppError::Validation("that link does not include the author's name; use the full link from the post's share menu".into()));
    }
    if parts.next() != Some("status") {
        return Err(bad());
    }
    let id = parts.next().ok_or_else(bad)?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
        return Err(bad());
    }
    Ok((repo::normalize_handle(handle)?, id.to_string()))
}

/// Manual add (OGC-01 escape hatch): bypasses the viral thresholds because the user judged the post worth studying.
#[tauri::command]
pub fn formats_add_post(state: State<'_, AppState>, post: ManualPost) -> AppResult<Detection> {
    let (handle, id) = parse_post_url(&post.url)?;
    let input = PostInput {
        platform_post_id: id,
        author_handle: handle,
        author_followers: post.author_followers,
        text: post.text,
        metrics: PostMetrics { likes: post.likes, reposts: post.reposts, replies: post.replies, bookmarks: post.bookmarks, views: post.views },
        posted_at: None,
    };
    let db = state.db.lock().unwrap();
    let thresholds = repo::get_thresholds(&db)?;
    pipeline::consider_post(&db, &state.profile_id, &input, PostSource::Manual, &thresholds, true)
}

#[tauri::command]
pub fn formats_get_thresholds(state: State<'_, AppState>) -> AppResult<ViralThresholds> {
    let db = state.db.lock().unwrap();
    repo::get_thresholds(&db)
}

#[tauri::command]
pub fn formats_set_thresholds(state: State<'_, AppState>, thresholds: ViralThresholds) -> AppResult<ViralThresholds> {
    let db = state.db.lock().unwrap();
    repo::set_thresholds(&db, &thresholds)?;
    repo::get_thresholds(&db)
}

#[tauri::command]
pub fn formats_watchlist(state: State<'_, AppState>) -> AppResult<Vec<WatchEntry>> {
    let db = state.db.lock().unwrap();
    repo::list_watch(&db, &state.profile_id)
}

#[tauri::command]
pub fn formats_watch_add(state: State<'_, AppState>, handle: String, followers: Option<u64>) -> AppResult<WatchEntry> {
    let db = state.db.lock().unwrap();
    repo::add_watch(&db, &state.profile_id, &handle, followers)
}

#[tauri::command]
pub fn formats_watch_remove(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    repo::remove_watch(&db, &state.profile_id, &id)
}

#[derive(Debug, Serialize)]
pub struct ScanSummary {
    pub handle: String,
    pub posts_read: usize,
    pub flagged: usize,
    pub new_candidates: usize,
}

/// Reads a watched creator's recent posts in the X window (read-only) and flags the ones that over-perform.
/// One creator per click: it is a user-started scan, never a background loop.
#[tauri::command]
pub async fn formats_watch_scan(state: State<'_, AppState>, id: String) -> AppResult<ScanSummary> {
    let (entry, thresholds) = {
        let db = state.db.lock().unwrap();
        crate::onboarding::require_acknowledged(&db)?;
        let entry = repo::list_watch(&db, &state.profile_id)?
            .into_iter()
            .find(|w| w.id == id)
            .ok_or_else(|| AppError::Validation("creator not found on the watchlist".into()))?;
        (entry, repo::get_thresholds(&db)?)
    };
    let _job = state.jobs.register("Scanning a watched creator");
    let posts = scan::scan_profile(&state.browser, &entry.handle).await?;

    let db = state.db.lock().unwrap();
    let mut summary = ScanSummary { handle: entry.handle.clone(), posts_read: posts.len(), flagged: 0, new_candidates: 0 };
    for p in posts {
        let input = PostInput {
            platform_post_id: p.id,
            author_handle: p.handle,
            author_followers: entry.followers,
            text: p.text,
            metrics: p.metrics,
            posted_at: p.posted_at,
        };
        if let Ok(d) = pipeline::consider_post(&db, &state.profile_id, &input, PostSource::Watchlist, &thresholds, false) {
            if d.verdict.flagged {
                summary.flagged += 1;
            }
            if d.candidate.is_some() {
                summary.new_candidates += 1;
            }
        }
    }
    repo::touch_watch_scanned(&db, &state.profile_id, &entry.id)?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_post_links() {
        for (u, h, i) in [
            ("https://x.com/alice/status/1234567890", "alice", "1234567890"),
            ("https://twitter.com/Bob_1/status/42?s=20&t=abc", "Bob_1", "42"),
            ("http://mobile.x.com/carol/status/7/", "carol", "7"),
            ("  https://www.x.com/dave/status/9#frag ", "dave", "9"),
        ] {
            assert_eq!(parse_post_url(u).unwrap(), (h.to_string(), i.to_string()), "{u}");
        }
    }

    #[test]
    fn rejects_other_links() {
        for u in [
            "",
            "alice/status/1",
            "https://example.com/alice/status/1",
            "https://x.com/alice",
            "https://x.com/alice/likes/1",
            "https://x.com/alice/status/abc",
            "https://x.com/i/status/1",
            "https://x.com.evil.com/alice/status/1",
        ] {
            assert!(parse_post_url(u).is_err(), "{u}");
        }
    }
}
