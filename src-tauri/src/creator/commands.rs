//! Tauri commands for the OGC composer. Publishing is the only write to X and it needs the user's click.

use super::drafts::{self, OgcDraft};
use super::{CreatorSettings, Generated};
use crate::error::{AppError, AppResult};
use crate::formats::repo;
use crate::phase3::Phase3;
use crate::publish::gate;
use crate::state::AppState;
use super::compose::{self, FormatMatch};
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

#[derive(Serialize)]
pub struct SimilarityView {
    pub score: f64,
    pub threshold: f64,
    pub flagged: bool,
}

#[tauri::command]
pub fn creator_get_settings(state: State<'_, AppState>) -> AppResult<CreatorSettings> {
    let db = state.db.lock().unwrap();
    super::get_settings(&db)
}

#[tauri::command]
pub fn creator_save_settings(state: State<'_, AppState>, settings: CreatorSettings) -> AppResult<CreatorSettings> {
    let db = state.db.lock().unwrap();
    super::save_settings(&db, &settings)
}

/// Suggests formats for a rough thought (OGC-07 format picker). Local word overlap; no model call.
#[tauri::command]
pub fn creator_match_formats(state: State<'_, AppState>, topic: String) -> AppResult<Vec<FormatMatch>> {
    let db = state.db.lock().unwrap();
    let formats = repo::list_formats(&db, &state.profile_id, false, repo::FormatSort::Working)?;
    Ok(compose::match_formats(&formats, &topic, 3))
}

#[tauri::command]
pub async fn creator_generate(state: State<'_, AppState>, format_id: String, topic: String) -> AppResult<Generated> {
    let _job = state.jobs.register("Writing post variants");
    super::generate_variants(&state.db, &state.profile_id, state.llm.as_ref(), &format_id, &topic).await
}

#[tauri::command]
pub fn creator_create_blank(state: State<'_, AppState>, format_id: Option<String>, text: String) -> AppResult<OgcDraft> {
    let db = state.db.lock().unwrap();
    super::create_blank(&db, &state.profile_id, format_id.as_deref(), &text)
}

#[tauri::command]
pub fn creator_drafts(state: State<'_, AppState>) -> AppResult<Vec<OgcDraft>> {
    let db = state.db.lock().unwrap();
    drafts::list_recent(&db, &state.profile_id, 60)
}

#[tauri::command]
pub fn creator_edit(state: State<'_, AppState>, id: String, text: String) -> AppResult<OgcDraft> {
    let db = state.db.lock().unwrap();
    super::edit_draft(&db, &state.profile_id, &id, &text)
}

#[tauri::command]
pub fn creator_set_media(state: State<'_, AppState>, id: String, paths: Vec<String>) -> AppResult<OgcDraft> {
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    let db = state.db.lock().unwrap();
    drafts::set_media(&db, &state.profile_id, &id, &paths)
}

#[tauri::command]
pub fn creator_discard(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    drafts::discard(&db, &state.profile_id, &id)
}

/// The user has checked X and the post is not there, so it may be published again.
#[tauri::command]
pub fn creator_reopen(state: State<'_, AppState>, id: String) -> AppResult<OgcDraft> {
    let db = state.db.lock().unwrap();
    drafts::reopen(&db, &state.profile_id, &id)
}

/// The Publish click. `expected_text` is what the confirmation dialog showed. The approval goes through the same
/// gate and serial worker as replies, so the kill switch and one-publish-at-a-time rules apply.
#[tauri::command]
pub async fn creator_publish(state: State<'_, AppState>, p3: State<'_, Phase3>, id: String, expected_text: String) -> AppResult<OgcDraft> {
    let approval = {
        let db = state.db.lock().unwrap();
        let max = super::get_settings(&db)?.max_chars;
        gate::approve_top_level(&db, &state.profile_id, &id, &expected_text, max, p3.kill.is_engaged())?
    };
    crate::commands::ensure_publish_browser_ready(&state).await?;
    let rx = p3.publish.submit_and_wait(approval);
    rx.await.map_err(|_| AppError::Other("the publish worker stopped".into()))?;
    let db = state.db.lock().unwrap();
    drafts::get(&db, &state.profile_id, &id)?.ok_or_else(|| AppError::Other("draft vanished".into()))
}
