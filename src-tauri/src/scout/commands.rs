use super::repo::{self, QueueItem, SessionParams, SessionView};
use super::session::{run_session, ScoutEnv};
use crate::control::{EventSink, StopSignal};
use crate::error::{AppError, AppResult};
use crate::onboarding::require_acknowledged;
use crate::phase3::{Phase3, RunningScout, TauriSink};
use crate::state::AppState;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use tokio::sync::watch;

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
    if matches!(params.feed_source.as_str(), "watchlist" | "both") && crate::phase3::get_watchlist(&state.db.lock().unwrap()).is_empty() {
        return Err(AppError::Validation("Add accounts to your watchlist in Settings first.".into()));
    }
    crate::commands::ensure_x_browser_open(&app, &state).await?;
    let stop = Arc::new(StopSignal::new());
    let pause = Arc::new(AtomicBool::new(false));
    let (done_tx, done_rx) = watch::channel(false);
    let session_id = {
        let mut slot = p3.scout.lock().unwrap();
        if slot.is_some() {
            return Err(AppError::Validation("A scout session is already running.".into()));
        }
        let db = state.db.lock().unwrap();
        let id = repo::create_session(&db, &state.profile_id, &params)?;
        let _ = repo::save_last_params(&db, &params);
        state.browser_health.abort();
        *slot = Some(RunningScout { session_id: id.clone(), stop: stop.clone(), pause: pause.clone(), done: done_rx });
        id
    };
    let handle = app.clone();
    let sid = session_id.clone();
    tauri::async_runtime::spawn(async move {
        let state = handle.state::<AppState>();
        let p3 = handle.state::<Phase3>();
        let guard = Arc::new(ScoutSlotGuard {
            app: handle.clone(),
            session_id: sid.clone(),
            params: params.clone(),
            done_tx,
        });
        let sink = ScoutEventSink { inner: TauriSink(handle.clone()), guard: guard.clone() };
        let relauncher = AppRelauncher(handle.clone());
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
            relaunch: Some(&relauncher),
        };
        run_session(&env, &sid, &params).await;
        if guard.release() {
            crate::tray::refresh_all(&handle);
        }
    });
    crate::tray::refresh_all(&app);
    Ok(session_id)
}

struct ScoutSlotGuard {
    app: AppHandle,
    session_id: String,
    params: SessionParams,
    done_tx: watch::Sender<bool>,
}

impl ScoutSlotGuard {
    fn release(&self) -> bool {
        let p3 = self.app.state::<Phase3>();
        let cleared = clear_running_scout(&p3, &self.session_id);
        if cleared {
            self.app.state::<AppState>().browser_health.attach_scout_params(self.params.clone());
        }
        let _ = self.done_tx.send_replace(true);
        cleared
    }
}

impl Drop for ScoutSlotGuard {
    fn drop(&mut self) {
        if self.release() {
            crate::tray::refresh_all(&self.app);
        }
    }
}

/// Clears the active scout slot before `scout:finished` reaches the webview or tray so a
/// `scout_status` refresh triggered by `scout:finished` always observes the session as idle.
struct ScoutEventSink {
    inner: TauriSink,
    guard: Arc<ScoutSlotGuard>,
}

impl EventSink for ScoutEventSink {
    fn emit(&self, event: &str, payload: serde_json::Value) {
        if event == "scout:finished" {
            self.guard.release();
        }
        self.inner.emit(event, payload);
    }
}

/// Clears `p3.scout` only when it still holds `session_id`, so a finishing task never clears a newer session.
pub fn clear_running_scout(p3: &Phase3, session_id: &str) -> bool {
    let mut slot = p3.scout.lock().unwrap();
    if slot.as_ref().is_some_and(|s| s.session_id == session_id) {
        *slot = None;
        true
    } else {
        false
    }
}

/// Signals the active scout session (if any) to cancel immediately and returns its completion receiver.
pub fn request_scout_stop(p3: &Phase3) -> Option<watch::Receiver<bool>> {
    p3.scout.lock().unwrap().as_ref().map(|s| {
        s.stop.stop();
        s.done.clone()
    })
}

/// Signals the active scout session to stop and waits up to `timeout` for cleanup to finish.
pub async fn wait_for_scout_stop(p3: &Phase3, timeout: Duration) -> bool {
    let Some(mut done) = request_scout_stop(p3) else {
        return false;
    };
    let _ = tokio::time::timeout(timeout, done.wait_for(|&finished| finished)).await;
    true
}

#[tauri::command]
pub async fn scout_stop(app: AppHandle) -> AppResult<bool> {
    let p3 = app.state::<Phase3>();
    let stopped = wait_for_scout_stop(&p3, Duration::from_secs(5)).await;
    crate::tray::refresh_all(&app);
    Ok(stopped)
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

/// Reopens the managed X window for a running session, unless the user closed it on purpose.
struct AppRelauncher(AppHandle);

#[async_trait::async_trait]
impl super::session::BrowserRelauncher for AppRelauncher {
    async fn relaunch(&self) -> Result<(), String> {
        let state = self.0.state::<AppState>();
        if !state.browser_health.expects_open() {
            return Err("the X window was closed on purpose".into());
        }
        crate::commands::ensure_x_browser_open(&self.0, &state).await.map(|_| ()).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn wait_for_scout_stop_signals_and_waits_for_slot_release() {
        let p3 = Arc::new(Phase3::new(false));
        assert!(!wait_for_scout_stop(&p3, Duration::from_millis(50)).await, "stopping when idle returns false");

        let stop = Arc::new(StopSignal::new());
        let (done_tx, done_rx) = watch::channel(false);
        *p3.scout.lock().unwrap() = Some(RunningScout {
            session_id: "s1".into(),
            stop: stop.clone(),
            pause: Arc::new(AtomicBool::new(false)),
            done: done_rx,
        });

        let p3_bg = p3.clone();
        let stop_bg = stop.clone();
        tokio::spawn(async move {
            stop_bg.cancelled().await;
            assert!(clear_running_scout(&p3_bg, "s1"));
            let _ = done_tx.send_replace(true);
        });

        let stopped = wait_for_scout_stop(&p3, Duration::from_secs(2)).await;
        assert!(stopped);
        assert!(stop.is_stopped());
        assert!(p3.scout.lock().unwrap().is_none(), "slot is already cleared when stop returns");
    }

    #[test]
    fn clear_running_scout_only_clears_matching_session() {
        let p3 = Phase3::new(false);
        let (_tx, rx) = watch::channel(false);
        *p3.scout.lock().unwrap() = Some(RunningScout {
            session_id: "s2".into(),
            stop: Arc::new(StopSignal::new()),
            pause: Arc::new(AtomicBool::new(false)),
            done: rx,
        });
        assert!(!clear_running_scout(&p3, "s1"), "stale session must not clear a newer session");
        assert_eq!(p3.scout.lock().unwrap().as_ref().map(|s| s.session_id.as_str()), Some("s2"));
        assert!(clear_running_scout(&p3, "s2"));
        assert!(p3.scout.lock().unwrap().is_none());
    }
}
