use super::repo::{self, QueueItem, SessionParams, SessionView};
use super::session::{run_session, ScoutEnv};
use crate::error::{AppError, AppResult};
use crate::onboarding::require_acknowledged;
use crate::phase3::{Phase3, RunningScout, TauriSink};
use crate::state::AppState;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

#[derive(Serialize)]
pub struct ScoutStatus {
    pub running: bool,
    pub session_id: Option<String>,
    pub threshold: f64,
    pub last_params: Option<SessionParams>,
    pub recent: Vec<SessionView>,
}

fn now_unix() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[tauri::command]
pub fn scout_status(state: State<'_, AppState>, p3: State<'_, Phase3>) -> AppResult<ScoutStatus> {
    let db = state.db.lock().unwrap();
    let running = p3.scout.lock().unwrap().as_ref().map(|s| s.session_id.clone());
    Ok(ScoutStatus { running: running.is_some(), session_id: running, threshold: repo::get_threshold(&db)?, last_params: repo::last_params(&db)?, recent: repo::recent_sessions(&db, &state.profile_id, 5)? })
}

#[tauri::command]
pub async fn scout_start(app: AppHandle, params: SessionParams) -> AppResult<String> {
    params.validate()?;
    let state = app.state::<AppState>();
    let p3 = app.state::<Phase3>();
    {
        let db = state.db.lock().unwrap();
        require_acknowledged(&db)?;
    }
    if p3.kill.is_engaged() {
        return Err(AppError::Validation("Everything is paused (kill switch on). Resume it before scouting.".into()));
    }
    if !state.browser.is_running().await {
        return Err(AppError::Validation("Open the X window first (sidebar, then Open X).".into()));
    }
    if params.feed_source != "for_you" && crate::phase3::get_watchlist(&state.db.lock().unwrap()).is_empty() {
        return Err(AppError::Validation("Add accounts to your watchlist in Settings first.".into()));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let session_id = {
        let mut slot = p3.scout.lock().unwrap();
        if slot.is_some() {
            return Err(AppError::Validation("A scout session is already running.".into()));
        }
        let db = state.db.lock().unwrap();
        let id = repo::create_session(&db, &state.profile_id, &params)?;
        let _ = repo::save_last_params(&db, &params);
        state.browser_health.abort();
        *slot = Some(RunningScout { session_id: id.clone(), stop: stop.clone(), pause: pause.clone() });
        id
    };
    let handle = app.clone();
    let sid = session_id.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let p3 = handle.state::<Phase3>();
        let sink = TauriSink(handle.clone());
        let _job = state.jobs.register("Scout session");
        let env = ScoutEnv {
            db: &state.db,
            browser: &state.browser,
            completion: p3.completion(),
            kill: &p3.kill,
            sink: &sink,
            profile_id: &state.profile_id,
            stop: &stop,
            pause: &pause,
            thumbs_dir: Some(state.paths.data_dir.join("thumbs")),
            idle_poll_ms: 1000,
            pause_ms: (1800, 3600),
        };
        run_session(&env, &sid, &params).await;
        state.browser_health.attach_scout_params(params.clone());
        *p3.scout.lock().unwrap() = None;
        crate::tray::refresh_all(&handle);
    });
    crate::tray::refresh_all(&app);
    Ok(session_id)
}

#[tauri::command]
pub fn scout_stop(p3: State<'_, Phase3>) {
    if let Some(s) = p3.scout.lock().unwrap().as_ref() {
        s.stop.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Holds or continues the running session without ending it (SCT-14, tray).
pub fn set_scout_paused(p3: &Phase3, paused: bool) -> bool {
    match p3.scout.lock().unwrap().as_ref() {
        Some(s) => {
            s.pause.store(paused, std::sync::atomic::Ordering::SeqCst);
            true
        }
        None => false,
    }
}

#[tauri::command]
pub fn scout_pause(app: AppHandle, p3: State<'_, Phase3>, paused: bool) -> bool {
    let changed = set_scout_paused(&p3, paused);
    crate::tray::refresh_all(&app);
    changed
}

#[tauri::command]
pub fn scout_queue(state: State<'_, AppState>) -> AppResult<Vec<QueueItem>> {
    let db = state.db.lock().unwrap();
    let threshold = repo::get_threshold(&db)?;
    repo::queue(&db, &state.profile_id, threshold, now_unix(), 100)
}

#[tauri::command]
pub fn scout_dismiss(app: AppHandle, state: State<'_, AppState>, evaluation_id: String, reason: String) -> AppResult<()> {
    repo::dismiss(&state.db.lock().unwrap(), &evaluation_id, &reason)?;
    crate::tray::refresh_all(&app);
    Ok(())
}

#[tauri::command]
pub fn scout_set_threshold(app: AppHandle, state: State<'_, AppState>, threshold: f64) -> AppResult<f64> {
    let v = repo::set_threshold(&state.db.lock().unwrap(), threshold)?;
    crate::tray::refresh_all(&app);
    Ok(v)
}
