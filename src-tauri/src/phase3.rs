//! Shared runtime for Phase 3 features: kill switch, completion provider slot, scout run slot, publish
//! queue, the Tauri event sink with OS notifications, tray, and screenshot retention.

use crate::browser::health::BrowserState;
use crate::completion::{CompletionProvider, UnconfiguredProvider};
use crate::control::{EventSink, KillSwitch};
use crate::error::{AppError, AppResult};
use crate::publish::queue::PublishQueue;
use crate::state::AppState;
use crate::storage::repo::{get_setting, set_setting};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tauri::{AppHandle, Emitter, Manager, State};

pub const KILL_KEY: &str = "kill_switch";
pub const SHOT_RETENTION_DAYS: u64 = 30;

pub struct RunningScout {
    pub session_id: String,
    pub stop: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
}

pub struct Phase3 {
    pub kill: Arc<KillSwitch>,
    completion: RwLock<Arc<dyn CompletionProvider>>,
    pub scout: Mutex<Option<RunningScout>>,
    pub publish: PublishQueue,
}

impl Phase3 {
    pub fn new(kill_engaged: bool) -> Self {
        Self {
            kill: Arc::new(KillSwitch::new(kill_engaged)),
            completion: RwLock::new(Arc::new(UnconfiguredProvider)),
            scout: Mutex::new(None),
            publish: PublishQueue::new(),
        }
    }

    pub fn completion(&self) -> Arc<dyn CompletionProvider> {
        self.completion.read().unwrap().clone()
    }

    /// Phase 2 installs its `LlmRouter` here.
    pub fn set_completion(&self, p: Arc<dyn CompletionProvider>) {
        *self.completion.write().unwrap() = p;
    }
}

pub fn load_kill_state(conn: &rusqlite::Connection) -> bool {
    get_setting(conn, KILL_KEY).ok().flatten().as_deref() == Some("1")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotificationSettings {
    pub enabled: bool,
    pub scout_done: bool,
    pub publish_done: bool,
    pub publish_failed: bool,
    pub attention: bool,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { enabled: true, scout_done: true, publish_done: true, publish_failed: true, attention: true }
    }
}

const NOTIFY_KEY: &str = "notification_settings";

pub fn get_notification_settings(conn: &rusqlite::Connection) -> AppResult<NotificationSettings> {
    Ok(get_setting(conn, NOTIFY_KEY)?.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default())
}

pub fn should_notify(s: &NotificationSettings, kind: &str) -> bool {
    s.enabled
        && match kind {
            "scout_done" => s.scout_done,
            "publish_done" => s.publish_done,
            "publish_failed" => s.publish_failed,
            "attention" => s.attention,
            _ => false,
        }
}

/// Forwards job events to the webview. `notify` events additionally raise an OS notification when the user
/// has not turned that kind off.
pub struct TauriSink(pub AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: &str, payload: Value) {
        if event == "notify" {
            let kind = payload.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            let allowed = {
                let state = self.0.state::<AppState>();
                let db = state.db.lock().unwrap();
                get_notification_settings(&db).map(|s| should_notify(&s, kind)).unwrap_or(false)
            };
            if allowed {
                use tauri_plugin_notification::NotificationExt;
                let title = payload.get("title").and_then(|t| t.as_str()).unwrap_or("Social Growth Helper");
                let body = payload.get("body").and_then(|t| t.as_str()).unwrap_or("");
                let _ = self.0.notification().builder().title(title).body(body).show();
            }
        }
        if event == "browser:halt" {
            let state = self.0.state::<AppState>();
            let kind = payload.get("state").and_then(|s| s.as_str()).and_then(BrowserState::parse);
            let source = payload.get("source").and_then(|s| s.as_str()).unwrap_or("");
            if let Some(kind) = kind {
                state.browser_health.record_halt(kind, source);
            }
        }
        if event == "scout:finished" {
            let s = &payload["summary"];
            let body = format!("{} posts scanned, {} added to the queue.", s["scanned"], s["queued"]);
            self.emit("notify", serde_json::json!({ "kind": "scout_done", "title": "Scout finished", "body": body }));
        }
        let _ = self.0.emit(event, payload);
        if event.starts_with("scout:") || event == "publish:update" {
            crate::tray::refresh_all(&self.0);
        }
    }
}

pub fn set_kill(app: &AppHandle, engaged: bool) -> AppResult<()> {
    let state = app.state::<AppState>();
    let p3 = app.state::<Phase3>();
    {
        let db = state.db.lock().unwrap();
        set_setting(&db, KILL_KEY, if engaged { "1" } else { "0" })?;
    }
    if engaged {
        p3.kill.engage();
        if let Some(s) = p3.scout.lock().unwrap().as_ref() {
            s.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    } else {
        p3.kill.release();
    }
    let _ = app.emit("kill:changed", engaged);
    crate::tray::refresh_all(app);
    Ok(())
}

#[tauri::command]
pub fn get_kill_switch(p3: State<'_, Phase3>) -> bool {
    p3.kill.is_engaged()
}

#[tauri::command]
pub fn set_kill_switch(app: AppHandle, engaged: bool) -> AppResult<bool> {
    set_kill(&app, engaged)?;
    Ok(engaged)
}

#[tauri::command]
pub fn get_notifications(state: State<'_, AppState>) -> AppResult<NotificationSettings> {
    get_notification_settings(&state.db.lock().unwrap())
}

#[tauri::command]
pub fn set_notifications(state: State<'_, AppState>, settings: NotificationSettings) -> AppResult<NotificationSettings> {
    set_setting(&state.db.lock().unwrap(), NOTIFY_KEY, &serde_json::to_string(&settings).map_err(|e| AppError::Other(e.to_string()))?)?;
    Ok(settings)
}

#[derive(Serialize)]
pub struct CompletionStatus {
    pub scoring: bool,
    pub drafting: bool,
    pub browser_navigation: bool,
}

#[tauri::command]
pub fn completion_status(p3: State<'_, Phase3>) -> CompletionStatus {
    use crate::completion::TaskSlot;
    let c = p3.completion();
    CompletionStatus {
        scoring: c.is_configured(TaskSlot::PostScoring),
        drafting: c.is_configured(TaskSlot::ReplyDrafting),
        browser_navigation: c.is_configured(TaskSlot::BrowserNavigation),
    }
}

const RETENTION_KEY: &str = "retention_days";
const WATCHLIST_KEY: &str = "scout_watchlist";
pub const MAX_WATCHLIST: usize = 50;

pub fn retention_days(conn: &rusqlite::Connection) -> u64 {
    get_setting(conn, RETENTION_KEY).ok().flatten().and_then(|v| v.parse().ok()).filter(|d| (1..=365).contains(d)).unwrap_or(SHOT_RETENTION_DAYS)
}

/// Removes audit screenshots and post thumbnails past the retention window (SEC-08).
pub fn purge_media(data_dir: &Path, days: u64) -> usize {
    ["audit-shots", "thumbs"].iter().map(|d| purge_old_files(&data_dir.join(d), days)).sum()
}

#[tauri::command]
pub fn retention_get(state: State<'_, AppState>) -> u64 {
    retention_days(&state.db.lock().unwrap())
}

#[tauri::command]
pub fn retention_set(state: State<'_, AppState>, days: u64) -> AppResult<u64> {
    if !(1..=365).contains(&days) {
        return Err(AppError::Validation("retention must be between 1 and 365 days".into()));
    }
    set_setting(&state.db.lock().unwrap(), RETENTION_KEY, &days.to_string())?;
    purge_media(&state.paths.data_dir, days);
    Ok(days)
}

pub fn get_watchlist(conn: &rusqlite::Connection) -> Vec<String> {
    get_setting(conn, WATCHLIST_KEY).ok().flatten().and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// Normalizes handles: strips `@`, keeps valid X handles (1-15 of letters, digits, underscore), unique.
pub fn clean_handles(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for h in raw {
        let h = h.trim().trim_start_matches('@').to_string();
        let ok = (1..=15).contains(&h.len()) && h.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if ok && !out.iter().any(|o| o.eq_ignore_ascii_case(&h)) {
            out.push(h);
        }
    }
    out.truncate(MAX_WATCHLIST);
    out
}

#[tauri::command]
pub fn watchlist_get(state: State<'_, AppState>) -> Vec<String> {
    get_watchlist(&state.db.lock().unwrap())
}

#[tauri::command]
pub fn watchlist_set(state: State<'_, AppState>, handles: Vec<String>) -> AppResult<Vec<String>> {
    let clean = clean_handles(&handles);
    set_setting(&state.db.lock().unwrap(), WATCHLIST_KEY, &serde_json::to_string(&clean).map_err(|e| AppError::Other(e.to_string()))?)?;
    Ok(clean)
}

#[tauri::command]
pub fn activity_list(state: State<'_, AppState>, limit: Option<i64>) -> AppResult<Vec<crate::activity::ActivityEntry>> {
    crate::activity::list(&state.db.lock().unwrap(), &state.profile_id, limit.unwrap_or(200))
}

/// Deletes audit screenshots older than `days` (SEC retention). Returns how many files were removed.
pub fn purge_old_files(dir: &Path, days: u64) -> usize {
    let Ok(rd) = std::fs::read_dir(dir) else { return 0 };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
    rd.filter_map(|e| e.ok())
        .filter(|e| e.metadata().and_then(|m| m.modified()).map(|t| t < cutoff).unwrap_or(false))
        .filter(|e| std::fs::remove_file(e.path()).is_ok())
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn notification_toggles_apply_per_kind() {
        let mut s = NotificationSettings::default();
        assert!(should_notify(&s, "publish_failed"));
        s.publish_failed = false;
        assert!(!should_notify(&s, "publish_failed"));
        assert!(!should_notify(&s, "unknown"));
        s.enabled = false;
        assert!(!should_notify(&s, "scout_done"));
    }

    #[test]
    fn retention_defaults_and_bounds_and_watchlist_is_cleaned() {
        let (conn, _) = crate::drafts::repo::testutil::db();
        assert_eq!(retention_days(&conn), 30);
        set_setting(&conn, RETENTION_KEY, "7").unwrap();
        assert_eq!(retention_days(&conn), 7);
        set_setting(&conn, RETENTION_KEY, "9999").unwrap();
        assert_eq!(retention_days(&conn), 30, "out of range falls back to the default");
        let raw: Vec<String> = ["@naval", "naval", " paulg ", "bad handle", "", "waytoolonghandle_123"].iter().map(|s| s.to_string()).collect();
        assert_eq!(clean_handles(&raw), vec!["naval", "paulg"]);
    }

    #[test]
    fn purge_media_covers_screenshots_and_thumbnails() {
        let d = tempdir().unwrap();
        for sub in ["audit-shots", "thumbs"] {
            std::fs::create_dir_all(d.path().join(sub)).unwrap();
            let f = d.path().join(sub).join("a.png");
            std::fs::write(&f, b"x").unwrap();
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(10 * 86_400)).unwrap();
        }
        assert_eq!(purge_media(d.path(), 30), 0);
        assert_eq!(purge_media(d.path(), 7), 2);
    }

    #[test]
    fn purge_removes_only_old_files() {
        let d = tempdir().unwrap();
        let old = d.path().join("old.png");
        let new = d.path().join("new.png");
        std::fs::write(&old, b"x").unwrap();
        std::fs::write(&new, b"x").unwrap();
        let f = std::fs::File::options().write(true).open(&old).unwrap();
        f.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 86_400)).unwrap();
        assert_eq!(purge_old_files(d.path(), SHOT_RETENTION_DAYS), 1);
        assert!(!old.exists() && new.exists());
    }

    #[test]
    fn kill_state_defaults_off_and_completion_is_swappable() {
        let (conn, _) = crate::drafts::repo::testutil::db();
        assert!(!load_kill_state(&conn));
        set_setting(&conn, KILL_KEY, "1").unwrap();
        assert!(load_kill_state(&conn));
        let p = Phase3::new(false);
        assert!(!p.completion().is_configured(crate::completion::TaskSlot::PostScoring));
        p.set_completion(Arc::new(crate::completion::ScriptedProvider::new(vec![])));
        assert!(p.completion().is_configured(crate::completion::TaskSlot::PostScoring));
    }
}
