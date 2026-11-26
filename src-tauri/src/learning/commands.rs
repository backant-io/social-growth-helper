//! Tauri commands for the learning loop and the Data panel. Thin wrappers: the logic and its tests live in
//! the sibling modules. Every command works on the single active profile bound to `AppState`.

use super::patch::Patch;
use super::reflection::{self, Mode, ReflectionSummary, Signals};
use super::store::{self, Insight};
use super::transfer::{self, ExportSummary, ImportSummary, Manifest};
use super::versions::{self, ChangelogEntry, ExampleInfo, VersionInfo};
use super::wipe::{self, WipeReport};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;

fn work_dir(state: &AppState) -> PathBuf {
    state.paths.data_dir.join("tmp")
}

#[derive(Serialize)]
pub struct LearningOverview {
    pub signals: Signals,
    pub proposed: Vec<Insight>,
    pub decided: Vec<Insight>,
}

#[tauri::command]
pub fn learning_overview(state: State<'_, AppState>) -> AppResult<LearningOverview> {
    let db = state.db.lock().unwrap();
    let all = store::list(&db, &state.profile_id, &[])?;
    let (proposed, decided) = all.into_iter().partition(|i| i.status == "proposed");
    Ok(LearningOverview { signals: reflection::compute_signals(&db, &state.profile_id)?, proposed, decided })
}

#[tauri::command]
pub fn learning_proposed_count(state: State<'_, AppState>) -> AppResult<i64> {
    let db = state.db.lock().unwrap();
    store::proposed_count(&db, &state.profile_id)
}

#[tauri::command]
pub fn learning_reflect(state: State<'_, AppState>, mode: Mode) -> AppResult<ReflectionSummary> {
    let db = state.db.lock().unwrap();
    reflection::run(&db, &state.profile_id, mode, vec![], None)
}

#[tauri::command]
pub fn learning_reflect_if_due(state: State<'_, AppState>) -> AppResult<Option<ReflectionSummary>> {
    let db = state.db.lock().unwrap();
    let now = crate::learning::outcomes::parse_ts(&crate::util::now_rfc3339()).unwrap_or(0);
    reflection::run_if_due(&db, &state.profile_id, now)
}

#[tauri::command]
pub fn learning_accept(state: State<'_, AppState>, insight_id: String, edited_patch: Option<Patch>) -> AppResult<Insight> {
    let db = state.db.lock().unwrap();
    store::accept(&db, &state.profile_id, &insight_id, edited_patch)?;
    store::get(&db, &state.profile_id, &insight_id)
}

#[tauri::command]
pub fn learning_reject(state: State<'_, AppState>, insight_id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    store::reject(&db, &state.profile_id, &insight_id)
}

#[tauri::command]
pub fn learning_versions(state: State<'_, AppState>, kind: String) -> AppResult<Vec<VersionInfo>> {
    let db = state.db.lock().unwrap();
    match kind.as_str() {
        "rubric" => versions::rubric_versions(&db, &state.profile_id),
        _ => Err(AppError::Validation("only the scout rubric has versions here; style versions are on the Style page".into())),
    }
}

#[tauri::command]
pub fn learning_rollback(state: State<'_, AppState>, kind: String, version: i64) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    versions::rollback(&db, &state.profile_id, &kind, version)
}

#[tauri::command]
pub fn learning_changelog(state: State<'_, AppState>) -> AppResult<Vec<ChangelogEntry>> {
    let db = state.db.lock().unwrap();
    versions::changelog(&db, &state.profile_id, 50)
}

#[tauri::command]
pub fn learning_examples(state: State<'_, AppState>) -> AppResult<Vec<ExampleInfo>> {
    let db = state.db.lock().unwrap();
    versions::examples(&db, &state.profile_id)
}

#[tauri::command]
pub fn learning_remove_example(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    versions::remove_promoted_example(&db, &state.profile_id, &id)
}

#[tauri::command]
pub fn learning_set_min_sample(state: State<'_, AppState>, value: usize) -> AppResult<usize> {
    let db = state.db.lock().unwrap();
    store::set_min_sample(&db, value)
}

#[tauri::command]
pub fn data_default_export_path() -> String {
    transfer::default_export_path().display().to_string()
}

#[tauri::command]
pub fn data_export(state: State<'_, AppState>, path: Option<String>) -> AppResult<ExportSummary> {
    let dest = path.as_deref().map(str::trim).filter(|p| !p.is_empty()).map(PathBuf::from).unwrap_or_else(transfer::default_export_path);
    let db = state.db.lock().unwrap();
    transfer::export_to(&db, &state.profile_id, &work_dir(&state), &dest)
}

#[tauri::command]
pub fn data_inspect_export(path: String) -> AppResult<Manifest> {
    transfer::read_manifest(&PathBuf::from(path.trim()))
}

fn refuse_while_busy(state: &AppState, what: &str) -> AppResult<()> {
    let jobs = state.jobs.active();
    if jobs.is_empty() {
        Ok(())
    } else {
        Err(AppError::Validation(format!("stop running work first ({}) before {what}", jobs.join(", "))))
    }
}

#[tauri::command]
pub fn data_import(state: State<'_, AppState>, path: String) -> AppResult<ImportSummary> {
    refuse_while_busy(&state, "importing")?;
    let db = state.db.lock().unwrap();
    transfer::import_from(&db, &state.profile_id, &work_dir(&state), &state.paths.backup_dir, &PathBuf::from(path.trim()), &state.secrets)
}

#[tauri::command]
pub async fn data_wipe(state: State<'_, AppState>, confirmation: String, ignore_keychain_errors: Option<bool>) -> AppResult<WipeReport> {
    refuse_while_busy(&state, "wiping")?;
    if confirmation.trim() != wipe::CONFIRMATION {
        return Err(AppError::Validation(format!("type {} to confirm", wipe::CONFIRMATION)));
    }
    let _ = state.browser.close().await;
    let db = state.db.lock().unwrap();
    wipe::wipe(&db, &state.paths, &state.secrets, &confirmation, ignore_keychain_errors.unwrap_or(false))
}
