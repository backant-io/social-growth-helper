use crate::browser::discovery::{self, BrowserInstall};
use crate::browser::service::{VisibleTweet, X_HOME_URL};
use crate::browser::ChromeLaunchConfig;
use crate::error::AppResult;
use crate::lifecycle::{decide_quit, QuitDecision};
use crate::onboarding::{self, OnboardingStatus, RulesNotice, RulesStatus};
use crate::secrets::SecretsStatus;
use crate::state::AppState;
use crate::storage::{self, repo};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

const BROWSER_PATH_KEY: &str = "browser_path_override";

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub data_dir: String,
    pub db_path: String,
    pub schema_version: u32,
    pub platform: String,
    pub arch: String,
}

#[tauri::command]
pub fn get_app_info(state: State<'_, AppState>) -> AppResult<AppInfo> {
    let db = state.db.lock().unwrap();
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir: state.paths.data_dir.display().to_string(),
        db_path: state.paths.db_path.display().to_string(),
        schema_version: storage::user_version(&db)?,
        platform: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
    })
}

#[tauri::command]
pub fn get_onboarding_status(state: State<'_, AppState>) -> AppResult<OnboardingStatus> {
    let db = state.db.lock().unwrap();
    onboarding::status(&db, &state.profile_id)
}

#[tauri::command]
pub fn get_rules_notice() -> RulesNotice {
    onboarding::notice()
}

#[tauri::command]
pub fn acknowledge_rules(state: State<'_, AppState>, version: u32, confirmed: bool) -> AppResult<RulesStatus> {
    let db = state.db.lock().unwrap();
    onboarding::acknowledge(&db, version, confirmed)
}

#[tauri::command]
pub fn complete_tour(state: State<'_, AppState>) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    onboarding::mark_tour_completed(&db)
}

#[tauri::command]
pub fn get_preferences(state: State<'_, AppState>) -> AppResult<repo::Preferences> {
    let db = state.db.lock().unwrap();
    Ok(repo::get_preferences(&db, &state.profile_id)?.unwrap_or_default())
}

#[tauri::command]
pub fn save_preferences(state: State<'_, AppState>, preferences: repo::Preferences) -> AppResult<repo::Preferences> {
    let db = state.db.lock().unwrap();
    repo::save_preferences(&db, &state.profile_id, preferences)
}

#[tauri::command]
pub fn secrets_status(state: State<'_, AppState>) -> SecretsStatus {
    state.secrets.status()
}

fn configured_browser_path(state: &AppState) -> AppResult<Option<PathBuf>> {
    let db = state.db.lock().unwrap();
    Ok(repo::get_setting(&db, BROWSER_PATH_KEY)?.map(PathBuf::from))
}

#[derive(Serialize)]
pub struct BrowserInfo {
    pub install: Option<BrowserInstall>,
    pub problem: Option<String>,
    pub override_path: Option<String>,
    pub running: bool,
}

#[tauri::command]
pub async fn browser_info(state: State<'_, AppState>) -> AppResult<BrowserInfo> {
    let override_path = configured_browser_path(&state)?;
    let (install, problem) = match discovery::discover_system(override_path.as_deref()) {
        Ok(i) => (Some(i), None),
        Err(e) => (None, Some(e.to_string())),
    };
    Ok(BrowserInfo {
        install,
        problem,
        override_path: override_path.map(|p| p.display().to_string()),
        running: state.browser.is_running().await,
    })
}

#[tauri::command]
pub fn set_browser_path(state: State<'_, AppState>, path: Option<String>) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    match path.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => {
            discovery::discover_system(Some(Path::new(p)))?;
            repo::set_setting(&db, BROWSER_PATH_KEY, p)
        }
        None => repo::delete_setting(&db, BROWSER_PATH_KEY),
    }
}

/// Opens the dedicated X Chrome window. Blocked until the rules of engagement are acknowledged, because
/// the next step is logging in and working inside X.
#[tauri::command]
pub async fn browser_launch_x(state: State<'_, AppState>) -> AppResult<bool> {
    {
        let db = state.db.lock().unwrap();
        onboarding::require_acknowledged(&db)?;
    }
    let override_path = configured_browser_path(&state)?;
    let install = discovery::discover_system(override_path.as_deref())?;
    let cfg = ChromeLaunchConfig {
        binary_path: install.path,
        profile_dir: state.paths.browser_profile_dir.clone(),
        start_url: X_HOME_URL.to_string(),
        width: 560,
        height: 900,
    };
    Ok(state.browser.launch(cfg).await?)
}

#[tauri::command]
pub async fn browser_close(state: State<'_, AppState>) -> AppResult<()> {
    Ok(state.browser.close().await?)
}

#[tauri::command]
pub async fn browser_current_url(state: State<'_, AppState>) -> AppResult<String> {
    Ok(state.browser.current_url().await?)
}

/// Dev action from the Phase 1 exit criteria: read tweets already rendered in the feed. Read-only.
#[tauri::command]
pub async fn browser_read_visible_tweets(state: State<'_, AppState>) -> AppResult<Vec<VisibleTweet>> {
    Ok(state.browser.read_visible_tweets().await?)
}

#[tauri::command]
pub fn active_jobs(state: State<'_, AppState>) -> Vec<String> {
    state.jobs.active()
}

/// Frontend confirmed "stop and quit" (or nothing was running): stop jobs, close X Chrome, exit.
#[tauri::command]
pub fn quit_app(app: AppHandle, state: State<'_, AppState>) {
    state.jobs.request_stop_all();
    begin_shutdown(&app);
}

pub fn request_quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    match decide_quit(&state.jobs) {
        QuitDecision::Proceed => begin_shutdown(app),
        decision => {
            let _ = app.emit("quit-guard", decision);
        }
    }
}

pub fn begin_shutdown(app: &AppHandle) {
    use std::sync::atomic::Ordering;
    let state = app.state::<AppState>();
    if state.quit_approved.swap(true, Ordering::SeqCst) {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        handle.state::<AppState>().browser.shutdown().await;
        handle.exit(0);
    });
}

