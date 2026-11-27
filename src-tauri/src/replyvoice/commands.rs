use super::repo::{self, ReplyFormat};
use super::{fetch, get_custom_prompt, set_custom_prompt};
use crate::error::{AppError, AppResult};
use crate::onboarding::require_acknowledged;
use crate::phase3::Phase3;
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
pub struct ReplyVoice {
    /// The saved prompt, empty when the built-in opening is in use.
    pub prompt: String,
    pub formats: Vec<ReplyFormat>,
}

fn view(state: &AppState) -> AppResult<ReplyVoice> {
    let db = state.db.lock().unwrap();
    Ok(ReplyVoice { prompt: get_custom_prompt(&db)?.unwrap_or_default(), formats: repo::list(&db, &state.profile_id)? })
}

#[tauri::command]
pub fn reply_voice_get(state: State<'_, AppState>) -> AppResult<ReplyVoice> {
    view(&state)
}

#[tauri::command]
pub fn reply_voice_set_prompt(state: State<'_, AppState>, prompt: String) -> AppResult<ReplyVoice> {
    set_custom_prompt(&state.db.lock().unwrap(), &prompt)?;
    view(&state)
}

#[tauri::command]
pub fn reply_format_set_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> AppResult<ReplyVoice> {
    repo::set_enabled(&state.db.lock().unwrap(), &state.profile_id, &id, enabled)?;
    view(&state)
}

#[tauri::command]
pub fn reply_format_delete(state: State<'_, AppState>, id: String) -> AppResult<ReplyVoice> {
    repo::delete(&state.db.lock().unwrap(), &state.profile_id, &id)?;
    view(&state)
}

/// Reads replies under your best scouted posts in the X window (read-only) and stores their structure as formats.
#[tauri::command]
pub async fn reply_formats_fetch(app: AppHandle) -> AppResult<fetch::FetchSummary> {
    let state = app.state::<AppState>();
    let p3 = app.state::<Phase3>();
    require_acknowledged(&state.db.lock().unwrap())?;
    if p3.kill.is_engaged() {
        return Err(AppError::Validation("Everything is paused (kill switch on). Resume it first.".into()));
    }
    if p3.scout.lock().unwrap().is_some() {
        return Err(AppError::Validation("Scout is running. Stop it before fetching formats.".into()));
    }
    crate::commands::ensure_x_browser_open(&app, &state).await?;
    let _job = state.jobs.register("Fetching successful comment formats");
    let provider = p3.completion();
    fetch::fetch_formats(&state.browser, &*provider, &state.db, &state.profile_id).await
}
