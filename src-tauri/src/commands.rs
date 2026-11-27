use crate::browser::compat::{self, VersionCheck};
use crate::browser::health::BrowserStatus;
use crate::browser::discovery::{self, BrowserInstall};
use crate::browser::display::{self, DisplayServer};
use crate::browser::service::{VisibleTweet, X_HOME_URL};
use crate::browser::{ChromeLaunchConfig, WindowBounds};
use crate::diagnostics::{self, DiagnosticsInput};
use crate::error::{AppError, AppResult};
use crate::layout::{self, LayoutPlan, WindowLayout, WorkArea};
use crate::lifecycle::{decide_quit, QuitDecision};
use crate::llm::catalog::{self, ModelInfo};
use crate::llm::prompts::{self, PromptTemplate};
use crate::llm::providers::anthropic_api::AnthropicApiProvider;
use crate::llm::providers::claude_cli::{self, ClaudeCliDetection, PERSONAL_USE_NOTICE};
use crate::llm::providers::xai_api::XaiApiProvider;
use crate::llm::providers::xai_oauth::{self, DevicePollOutcome, XaiDeviceChallenge};
use crate::llm::repo::{
    self as llm_repo, ProviderAccount, ProviderAccountConfig, TaskModelRoute, TaskModelRoutePatch,
    UsageDashboard,
};
use crate::llm::{BillingLane, ProviderHealth, ProviderStatus, ProviderType, TaskType};
use crate::onboarding::{self, OnboardingStatus, RulesNotice, RulesStatus};
use crate::secrets::SecretsStatus;
use crate::state::AppState;
use crate::storage::{self, repo};
use crate::style::bench::{self, StyleBenchInput, StyleBenchResult};
use crate::style::repo as style_repo;
use crate::style::{
    ExampleKind, ImportCandidate, RetrievedExample, StyleExample, StyleExampleInput, StyleGuide,
    StyleGuideInput,
};
use crate::util::now_rfc3339;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, State, WebviewWindow};

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
    pub version_check: Option<VersionCheck>,
    pub display_server: DisplayServer,
    pub placement_hint: Option<String>,
}

#[tauri::command]
pub async fn browser_info(state: State<'_, AppState>) -> AppResult<BrowserInfo> {
    let override_path = configured_browser_path(&state)?;
    let (install, problem) = match discovery::discover_system(override_path.as_deref()) {
        Ok(i) => (Some(i), None),
        Err(e) => (None, Some(e.to_string())),
    };
    let version_check = match &install {
        Some(i) => {
            let path = i.path.clone();
            tokio::task::spawn_blocking(move || compat::check_binary(&path)).await.ok()
        }
        None => None,
    };
    let display_server = display::detect_system();
    Ok(BrowserInfo {
        install,
        problem,
        override_path: override_path.map(|p| p.display().to_string()),
        running: state.browser.is_running().await,
        version_check,
        display_server,
        placement_hint: display_server.placement_hint().map(str::to_string),
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

/// Ensures the visible X Chrome window (used for login and Scout browsing) is open.
pub async fn ensure_x_browser_open(app: &AppHandle, state: &AppState) -> AppResult<bool> {
    {
        let db = state.db.lock().unwrap();
        onboarding::require_acknowledged(&db)?;
    }
    if state.browser.is_running().await {
        state.browser_health.set_expected_open(true);
        return Ok(false);
    }
    let override_path = configured_browser_path(state)?;
    let install = discovery::discover_system(override_path.as_deref())?;
    let cfg = ChromeLaunchConfig {
        binary_path: install.path,
        profile_dir: state.paths.browser_profile_dir.clone(),
        start_url: X_HOME_URL.to_string(),
        width: 560,
        height: 900,
        invisible: false,
    };
    let launched = state.browser.launch(cfg).await?;
    state.browser_health.set_expected_open(true);
    if launched {
        if let Err(e) = arrange_windows(app, state, true).await {
            log::warn!("restoring window layout: {e}");
        }
    }
    Ok(launched)
}

fn copy_dir_skipping_locks(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with("Singleton") || name_str == "LOCK" || name_str.ends_with(".lock") {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            copy_dir_skipping_locks(&from, &to)?;
        } else {
            let _ = std::fs::copy(&from, &to);
        }
    }
    Ok(())
}

fn file_mtime(p: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(p).ok()?.modified().ok()
}

fn sync_profile_state_if_newer(src_profile: &Path, dst_profile: &Path) -> bool {
    if !src_profile.exists() {
        return false;
    }
    let src_cookies = src_profile.join("Default").join("Cookies");
    let dst_cookies = dst_profile.join("Default").join("Cookies");
    let needs_seed = !dst_profile.exists() || !dst_cookies.exists();
    let src_newer = matches!((file_mtime(&src_cookies), file_mtime(&dst_cookies)), (Some(s), Some(d)) if s > d);
    if !needs_seed && !src_newer {
        return false;
    }
    let _ = std::fs::create_dir_all(dst_profile.join("Default"));
    for root_file in ["Local State"] {
        let s = src_profile.join(root_file);
        if s.is_file() {
            let _ = std::fs::copy(&s, dst_profile.join(root_file));
        }
    }
    for def_file in ["Cookies", "Cookies-journal", "Preferences"] {
        let s = src_profile.join("Default").join(def_file);
        if s.is_file() {
            let _ = std::fs::copy(&s, dst_profile.join("Default").join(def_file));
        }
    }
    for def_dir in ["Local Storage", "Session Storage"] {
        let _ = copy_dir_skipping_locks(&src_profile.join("Default").join(def_dir), &dst_profile.join("Default").join(def_dir));
    }
    true
}

/// Ensures the separate invisible reply/publish Chrome instance is running and shares cookies/session
/// state with the visible X browser profile (`x-default`).
pub async fn ensure_publish_browser_ready(state: &AppState) -> AppResult<()> {
    {
        let db = state.db.lock().unwrap();
        onboarding::require_acknowledged(&db)?;
    }
    let override_path = configured_browser_path(state)?;
    let install = discovery::discover_system(override_path.as_deref())?;
    let scout_running = state.browser.is_running().await;

    if scout_running {
        let cookies = state.browser.export_cookies().await.unwrap_or_default();
        if !state.publish_browser.is_running().await {
            sync_profile_state_if_newer(&state.paths.browser_profile_dir, &state.paths.publish_browser_profile_dir);
            let cfg = ChromeLaunchConfig {
                binary_path: install.path,
                profile_dir: state.paths.publish_browser_profile_dir.clone(),
                start_url: "about:blank".to_string(),
                width: 1280,
                height: 900,
                invisible: true,
            };
            state.publish_browser.launch(cfg).await?;
        }
        if !cookies.is_empty() {
            state.publish_browser.import_cookies(&cookies).await?;
        }
    } else {
        let src_cookies = state.paths.browser_profile_dir.join("Default").join("Cookies");
        let dst_cookies = state.paths.publish_browser_profile_dir.join("Default").join("Cookies");
        let src_newer = !dst_cookies.exists()
            || matches!((file_mtime(&src_cookies), file_mtime(&dst_cookies)), (Some(s), Some(d)) if s > d);
        if src_newer && state.publish_browser.is_running().await {
            let _ = state.publish_browser.close().await;
        }
        sync_profile_state_if_newer(&state.paths.browser_profile_dir, &state.paths.publish_browser_profile_dir);
        if !state.publish_browser.is_running().await {
            let cfg = ChromeLaunchConfig {
                binary_path: install.path,
                profile_dir: state.paths.publish_browser_profile_dir.clone(),
                start_url: X_HOME_URL.to_string(),
                width: 1280,
                height: 900,
                invisible: true,
            };
            state.publish_browser.launch(cfg).await?;
        }
    }
    Ok(())
}

/// Opens the dedicated X Chrome window. Blocked until the rules of engagement are acknowledged, because
/// the next step is logging in and working inside X.
#[tauri::command]
pub async fn browser_launch_x(app: AppHandle, state: State<'_, AppState>) -> AppResult<bool> {
    ensure_x_browser_open(&app, &state).await
}

const MAIN_WINDOW: &str = "main";

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window(MAIN_WINDOW)
}

fn logical(v: i32, scale: f64) -> i32 {
    (f64::from(v) / scale).round() as i32
}

fn current_work_area(win: &WebviewWindow) -> Option<WorkArea> {
    let monitor = win.current_monitor().ok()??;
    let (s, r) = (monitor.scale_factor(), monitor.work_area());
    Some(WorkArea {
        x: logical(r.position.x, s),
        y: logical(r.position.y, s),
        width: logical(r.size.width as i32, s).max(0) as u32,
        height: logical(r.size.height as i32, s).max(0) as u32,
    })
}

fn current_app_bounds(win: &WebviewWindow) -> Option<WindowBounds> {
    let s = win.scale_factor().ok()?;
    let (pos, size) = (win.outer_position().ok()?, win.inner_size().ok()?);
    Some(WindowBounds {
        left: logical(pos.x, s),
        top: logical(pos.y, s),
        width: logical(size.width as i32, s).max(0) as u32,
        height: logical(size.height as i32, s).max(0) as u32,
    })
}

fn place_app_window(win: &WebviewWindow, b: &WindowBounds) {
    let _ = win.set_position(LogicalPosition::new(b.left, b.top));
    let _ = win.set_size(LogicalSize::new(b.width, b.height));
}

#[derive(Serialize)]
pub struct LayoutStatus {
    pub display_server: DisplayServer,
    pub placement_supported: bool,
    pub hint: Option<String>,
    pub saved: bool,
}

fn layout_status(state: &AppState) -> AppResult<LayoutStatus> {
    let display_server = display::detect_system();
    let db = state.db.lock().unwrap();
    Ok(LayoutStatus {
        display_server,
        placement_supported: display_server.supports_window_placement(),
        hint: display_server.placement_hint().map(str::to_string),
        saved: layout::load(&db)?.is_some(),
    })
}

/// Applies the saved (or default side-by-side) layout. On Wayland and unknown display servers it does nothing.
async fn arrange_windows(app: &AppHandle, state: &AppState, include_app: bool) -> AppResult<()> {
    let Some(win) = main_window(app) else { return Ok(()) };
    let Some(area) = current_work_area(&win) else { return Ok(()) };
    let saved = layout::load(&state.db.lock().unwrap())?;
    let LayoutPlan::Apply(l) = layout::plan(display::detect_system(), saved, &area) else { return Ok(()) };
    if include_app {
        place_app_window(&win, &l.app);
    }
    if state.browser.is_running().await {
        state.browser.set_bounds(l.browser).await?;
    }
    Ok(())
}

/// Startup: put only the app window back where the user left it. The X window follows when it is opened.
pub fn restore_app_window_layout(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Some(win) = main_window(app) else { return };
    let (Some(area), Ok(Some(saved))) = (current_work_area(&win), layout::load(&state.db.lock().unwrap())) else { return };
    if let LayoutPlan::Apply(l) = layout::plan(display::detect_system(), Some(saved), &area) {
        place_app_window(&win, &l.app);
    }
}

#[tauri::command]
pub fn window_layout_status(state: State<'_, AppState>) -> AppResult<LayoutStatus> {
    layout_status(&state)
}

/// Remembers where the app window and the X window are right now.
#[tauri::command]
pub async fn save_window_layout(app: AppHandle, state: State<'_, AppState>) -> AppResult<LayoutStatus> {
    if display::detect_system().supports_window_placement() {
        let win = main_window(&app).ok_or_else(|| AppError::Other("main window not found".into()))?;
        let area = current_work_area(&win).ok_or_else(|| AppError::Other("could not read the screen size".into()))?;
        let app_bounds = current_app_bounds(&win).ok_or_else(|| AppError::Other("could not read the window position".into()))?;
        let browser_bounds = match state.browser.get_bounds().await {
            Ok(b) => b,
            Err(_) => {
                let saved = layout::load(&state.db.lock().unwrap())?;
                layout::resolve(saved, &area).browser
            }
        };
        layout::save(&state.db.lock().unwrap(), &WindowLayout { app: app_bounds, browser: browser_bounds })?;
    }
    layout_status(&state)
}

#[tauri::command]
pub async fn apply_window_layout(app: AppHandle, state: State<'_, AppState>) -> AppResult<LayoutStatus> {
    arrange_windows(&app, &state, true).await?;
    layout_status(&state)
}

#[tauri::command]
pub fn reset_window_layout(state: State<'_, AppState>) -> AppResult<LayoutStatus> {
    layout::clear(&state.db.lock().unwrap())?;
    layout_status(&state)
}

/// Plain-text report for bug reports (SHL-14): environment facts and the recent log, redacted.
#[tauri::command]
pub async fn get_diagnostics(state: State<'_, AppState>) -> AppResult<String> {
    let override_path = configured_browser_path(&state)?;
    let install = discovery::discover_system(override_path.as_deref());
    let (browser, browser_check) = match &install {
        Ok(i) => {
            let path = i.path.clone();
            let check = tokio::task::spawn_blocking(move || compat::check_binary(&path)).await.ok();
            let summary = match &check {
                Some(c) => c.warning.clone().unwrap_or_else(|| format!("ok ({})", c.version.clone().unwrap_or_default())),
                None => "not checked".into(),
            };
            (format!("{:?} at {}", i.kind, i.path.display()), summary)
        }
        Err(e) => (format!("not found: {e}"), "n/a".into()),
    };
    let schema_version = storage::user_version(&state.db.lock().unwrap()).ok();
    let secrets = state.secrets.status();
    let report = diagnostics::build(&DiagnosticsInput {
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        display_server: &format!("{:?}", display::detect_system()).to_lowercase(),
        browser,
        browser_check,
        secrets_backend: format!("{} (available: {})", secrets.backend, secrets.available),
        schema_version,
        data_dir: &state.paths.data_dir,
        log_dir: &state.paths.log_dir,
        log_tail: crate::logging::tail(&state.paths.log_dir, diagnostics::LOG_TAIL_LINES),
        home: dirs::home_dir().as_deref(),
    });
    Ok(report)
}

#[tauri::command]
pub async fn browser_close(state: State<'_, AppState>) -> AppResult<()> {
    state.browser_health.set_expected_open(false);
    Ok(state.browser.close().await?)
}

/// What the persistent banner shows: closed, crashed, logged out, challenge, suspended, locked or ready,
/// plus the job (if any) that this stopped.
#[tauri::command]
pub async fn browser_status(state: State<'_, AppState>) -> AppResult<BrowserStatus> {
    Ok(state.browser_health.status(&state.browser).await)
}

/// Raises the X window so the user can solve a check or log in (challenge takeover).
#[tauri::command]
pub async fn browser_focus_x(state: State<'_, AppState>) -> AppResult<()> {
    Ok(state.browser.bring_to_front().await?)
}

/// The user fixed X by hand. Verifies the page and, if it is clear, releases the stopped job and starts a
/// stopped Scout session again with its original settings.
#[tauri::command]
pub async fn browser_resume(app: AppHandle) -> AppResult<BrowserStatus> {
    let state = app.state::<AppState>();
    let outcome = state.browser_health.resume(&state.browser).await;
    if let Some(params) = outcome.restart_scout {
        crate::scout::commands::scout_start(app.clone(), params).await?;
    }
    Ok(outcome.status)
}

/// Gives up on the job a halt stopped. A running Scout session is stopped; X itself is left as it is.
#[tauri::command]
pub async fn browser_abort(app: AppHandle) -> AppResult<BrowserStatus> {
    let state = app.state::<AppState>();
    state.browser_health.abort();
    crate::scout::commands::scout_stop(app.state::<crate::phase3::Phase3>());
    Ok(state.browser_health.status(&state.browser).await)
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
        let st = handle.state::<AppState>();
        st.browser.shutdown().await;
        st.publish_browser.shutdown().await;
        handle.exit(0);
    });
}

// ---------------------------------------------------------------------------
// Phase 2: LLM Providers, Catalogs, Task Routing, Prompts & Usage (LLM-01..21)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ProviderAccountView {
    pub id: String,
    pub provider_type: ProviderType,
    pub label: String,
    pub billing_lane: BillingLane,
    pub status: String,
    pub last_verified_at: Option<String>,
    pub last_error: Option<String>,
    pub has_secret: bool,
    pub config: ProviderAccountConfig,
    pub models: Vec<ModelInfo>,
    pub personal_use_notice: Option<String>,
}

fn health_status_str(h: ProviderHealth) -> &'static str {
    match h {
        ProviderHealth::Connected => "connected",
        ProviderHealth::NeedsLogin => "needs_login",
        ProviderHealth::Expired => "expired",
        ProviderHealth::Offline => "offline",
        ProviderHealth::Error => "error",
        ProviderHealth::Disconnected => "disconnected",
    }
}

fn to_account_view(state: &AppState, acct: ProviderAccount, remote_models: &[String]) -> ProviderAccountView {
    let has_secret = acct
        .keychain_ref
        .as_deref()
        .and_then(|k| state.secrets.load(k).ok().flatten())
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let cached = state.remote_models.lock().unwrap().get(&acct.id).cloned().unwrap_or_default();
    let remote = if remote_models.is_empty() { cached.as_slice() } else { remote_models };
    let models = catalog::merge_catalog(acct.provider_type, remote, &acct.config.custom_models);
    let personal_use_notice = (acct.provider_type == ProviderType::ClaudeCli)
        .then(|| PERSONAL_USE_NOTICE.to_string());
    ProviderAccountView {
        id: acct.id,
        provider_type: acct.provider_type,
        label: acct.label,
        billing_lane: acct.billing_lane,
        status: acct.status,
        last_verified_at: acct.last_verified_at,
        last_error: acct.last_error,
        has_secret,
        config: acct.config,
        models,
        personal_use_notice,
    }
}

fn sync_router_from_db(state: &AppState) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    state
        .llm_router
        .sync_from_db(&db, state.secrets.clone(), state.browser.clone())
        .map_err(AppError::Llm)
}

#[tauri::command]
pub fn llm_list_providers(state: State<'_, AppState>) -> AppResult<Vec<ProviderAccountView>> {
    let accounts = {
        let db = state.db.lock().unwrap();
        llm_repo::list_provider_accounts(&db)?
    };
    Ok(accounts
        .into_iter()
        .map(|a| to_account_view(&state, a, &[]))
        .collect())
}

#[derive(Serialize)]
pub struct ClaudeCliDetectionResponse {
    pub detection: ClaudeCliDetection,
    pub personal_use_notice: String,
}

#[tauri::command]
pub async fn llm_detect_claude_cli(override_path: Option<String>) -> ClaudeCliDetectionResponse {
    let clean = override_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let detection = claude_cli::detect_claude_cli(clean.as_deref()).await;
    ClaudeCliDetectionResponse {
        detection,
        personal_use_notice: PERSONAL_USE_NOTICE.to_string(),
    }
}

#[tauri::command]
pub async fn llm_connect_claude_cli(
    state: State<'_, AppState>,
    label: Option<String>,
    binary_path: Option<String>,
) -> AppResult<ProviderAccountView> {
    let clean_path = binary_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let id = "claude_cli".to_string();
    let existing = {
        let db = state.db.lock().unwrap();
        llm_repo::get_provider_account(&db, &id)?
    };
    let mut cfg = existing.as_ref().map(|a| a.config.clone()).unwrap_or_default();
    cfg.cli_binary_path = clean_path;

    let acct = ProviderAccount {
        id: id.clone(),
        provider_type: ProviderType::ClaudeCli,
        label: label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Claude Subscription CLI")
            .to_string(),
        keychain_ref: None,
        billing_lane: BillingLane::Subscription,
        status: "disconnected".into(),
        last_verified_at: existing.as_ref().and_then(|a| a.last_verified_at.clone()),
        last_error: None,
        config: cfg,
    };
    {
        let db = state.db.lock().unwrap();
        llm_repo::upsert_provider_account(&db, &acct)?;
    }
    sync_router_from_db(&state)?;
    let status = state
        .llm_router
        .health_check_account(&id)
        .await
        .unwrap_or_else(|e| ProviderStatus {
            health: ProviderHealth::Error,
            message: e.message,
            checked_at: now_rfc3339(),
            details: None,
        });
    let updated = {
        let db = state.db.lock().unwrap();
        let st = health_status_str(status.health);
        let verified = (status.health == ProviderHealth::Connected).then(|| status.checked_at.clone());
        let err = (status.health != ProviderHealth::Connected).then(|| status.message.clone());
        llm_repo::update_provider_status(&db, &id, st, verified.as_deref(), err.as_deref())?;
        llm_repo::get_provider_account(&db, &id)?
            .ok_or_else(|| AppError::Other("provider disappeared".into()))?
    };
    Ok(to_account_view(&state, updated, &[]))
}

#[tauri::command]
pub async fn llm_connect_grok_web(
    state: State<'_, AppState>,
    label: Option<String>,
) -> AppResult<ProviderAccountView> {
    let id = "grok_web".to_string();
    let existing = {
        let db = state.db.lock().unwrap();
        llm_repo::get_provider_account(&db, &id)?
    };
    let acct = ProviderAccount {
        id: id.clone(),
        provider_type: ProviderType::GrokWeb,
        label: label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("Grok X Tab (Experimental)")
            .to_string(),
        keychain_ref: None,
        billing_lane: BillingLane::Subscription,
        status: "disconnected".into(),
        last_verified_at: existing.as_ref().and_then(|a| a.last_verified_at.clone()),
        last_error: None,
        config: existing.map(|a| a.config).unwrap_or_default(),
    };
    {
        let db = state.db.lock().unwrap();
        llm_repo::upsert_provider_account(&db, &acct)?;
    }
    sync_router_from_db(&state)?;
    let status = state
        .llm_router
        .health_check_account(&id)
        .await
        .unwrap_or_else(|e| ProviderStatus {
            health: ProviderHealth::Disconnected,
            message: e.message,
            checked_at: now_rfc3339(),
            details: None,
        });
    let updated = {
        let db = state.db.lock().unwrap();
        let st = health_status_str(status.health);
        let verified = (status.health == ProviderHealth::Connected).then(|| status.checked_at.clone());
        let err = (status.health != ProviderHealth::Connected).then(|| status.message.clone());
        llm_repo::update_provider_status(&db, &id, st, verified.as_deref(), err.as_deref())?;
        llm_repo::get_provider_account(&db, &id)?
            .ok_or_else(|| AppError::Other("provider disappeared".into()))?
    };
    Ok(to_account_view(&state, updated, &[]))
}

#[tauri::command]
pub async fn llm_save_api_key_provider(
    state: State<'_, AppState>,
    provider_type: ProviderType,
    label: Option<String>,
    api_key: String,
) -> AppResult<ProviderAccountView> {
    if !matches!(provider_type, ProviderType::AnthropicApi | ProviderType::XaiApi) {
        return Err(AppError::Validation(
            "API key storage is only valid for Anthropic API or xAI API".into(),
        ));
    }
    let trimmed = api_key.trim();
    if trimmed.len() < 8 {
        return Err(AppError::Validation("API key looks too short".into()));
    }
    let id = provider_type.as_str().to_string();
    let keychain_ref = format!("provider:{id}");
    state.secrets.store(&keychain_ref, trimmed)?;

    let existing = {
        let db = state.db.lock().unwrap();
        llm_repo::get_provider_account(&db, &id)?
    };
    let default_label = match provider_type {
        ProviderType::AnthropicApi => "Anthropic API",
        ProviderType::XaiApi => "xAI API",
        _ => "API Provider",
    };
    let acct = ProviderAccount {
        id: id.clone(),
        provider_type,
        label: label
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(default_label)
            .to_string(),
        keychain_ref: Some(keychain_ref),
        billing_lane: BillingLane::Api,
        status: "disconnected".into(),
        last_verified_at: existing.as_ref().and_then(|a| a.last_verified_at.clone()),
        last_error: None,
        config: existing.map(|a| a.config).unwrap_or_default(),
    };
    {
        let db = state.db.lock().unwrap();
        llm_repo::upsert_provider_account(&db, &acct)?;
    }
    sync_router_from_db(&state)?;
    let status = state
        .llm_router
        .health_check_account(&id)
        .await
        .unwrap_or_else(|e| ProviderStatus {
            health: ProviderHealth::Error,
            message: e.message,
            checked_at: now_rfc3339(),
            details: None,
        });
    let updated = {
        let db = state.db.lock().unwrap();
        let st = health_status_str(status.health);
        let verified = (status.health == ProviderHealth::Connected).then(|| status.checked_at.clone());
        let err = (status.health != ProviderHealth::Connected).then(|| status.message.clone());
        llm_repo::update_provider_status(&db, &id, st, verified.as_deref(), err.as_deref())?;
        llm_repo::get_provider_account(&db, &id)?
            .ok_or_else(|| AppError::Other("provider disappeared".into()))?
    };
    Ok(to_account_view(&state, updated, &[]))
}

#[tauri::command]
pub async fn llm_xai_oauth_start(state: State<'_, AppState>) -> AppResult<XaiDeviceChallenge> {
    xai_oauth::start_device_flow(&state.http)
        .await
        .map_err(AppError::Llm)
}

#[derive(Serialize)]
pub struct XaiOAuthPollResult {
    pub outcome: DevicePollOutcome,
    pub account: Option<ProviderAccountView>,
}

#[tauri::command]
pub async fn llm_xai_oauth_poll(
    state: State<'_, AppState>,
    token_endpoint: String,
    device_code: String,
    endpoint_base: Option<String>,
    label: Option<String>,
) -> AppResult<XaiOAuthPollResult> {
    let base = endpoint_base
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(xai_oauth::DEFAULT_INFERENCE_BASE)
        .to_string();
    let (outcome, maybe_tokens) =
        xai_oauth::poll_device_token(&state.http, &token_endpoint, &device_code, &base)
            .await
            .map_err(AppError::Llm)?;

    if let Some(tokens) = maybe_tokens {
        let id = "xai_oauth".to_string();
        let keychain_ref = format!("provider:{id}");
        let serialized = serde_json::to_string(&tokens)
            .map_err(|e| AppError::Other(format!("failed to serialize OAuth tokens: {e}")))?;
        state.secrets.store(&keychain_ref, &serialized)?;

        let existing = {
            let db = state.db.lock().unwrap();
            llm_repo::get_provider_account(&db, &id)?
        };
        let mut cfg = existing.as_ref().map(|a| a.config.clone()).unwrap_or_default();
        cfg.endpoint_override = Some(base);
        let acct = ProviderAccount {
            id: id.clone(),
            provider_type: ProviderType::XaiOAuth,
            label: label
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("SuperGrok / xAI OAuth")
                .to_string(),
            keychain_ref: Some(keychain_ref),
            billing_lane: BillingLane::Subscription,
            status: "connected".into(),
            last_verified_at: Some(now_rfc3339()),
            last_error: None,
            config: cfg,
        };
        {
            let db = state.db.lock().unwrap();
            llm_repo::upsert_provider_account(&db, &acct)?;
        }
        sync_router_from_db(&state)?;
        return Ok(XaiOAuthPollResult {
            outcome,
            account: Some(to_account_view(&state, acct, &[])),
        });
    }

    Ok(XaiOAuthPollResult {
        outcome,
        account: None,
    })
}

#[tauri::command]
pub async fn llm_health_check_provider(
    state: State<'_, AppState>,
    account_id: String,
) -> AppResult<ProviderAccountView> {
    let status = state
        .llm_router
        .health_check_account(&account_id)
        .await
        .map_err(AppError::Llm)?;
    let updated = {
        let db = state.db.lock().unwrap();
        let st = health_status_str(status.health);
        let verified = (status.health == ProviderHealth::Connected).then(|| status.checked_at.clone());
        let err = (status.health != ProviderHealth::Connected).then(|| status.message.clone());
        llm_repo::update_provider_status(&db, &account_id, st, verified.as_deref(), err.as_deref())?;
        llm_repo::get_provider_account(&db, &account_id)?
            .ok_or_else(|| AppError::Validation("provider account not found".into()))?
    };
    Ok(to_account_view(&state, updated, &[]))
}

#[tauri::command]
pub async fn llm_refresh_provider_models(
    state: State<'_, AppState>,
    account_id: String,
) -> AppResult<Vec<ModelInfo>> {
    let acct = {
        let db = state.db.lock().unwrap();
        llm_repo::get_provider_account(&db, &account_id)?
            .ok_or_else(|| AppError::Validation("provider account not found".into()))?
    };
    let remote = match acct.provider_type {
        ProviderType::XaiApi => {
            let kref = acct
                .keychain_ref
                .clone()
                .unwrap_or_else(|| format!("provider:{}", acct.id));
            let p = XaiApiProvider::new(acct.id.clone(), kref, state.secrets.clone());
            p.fetch_model_ids().await.map_err(AppError::Llm)?
        }
        ProviderType::AnthropicApi => {
            let kref = acct
                .keychain_ref
                .clone()
                .unwrap_or_else(|| format!("provider:{}", acct.id));
            let p = AnthropicApiProvider::new(acct.id.clone(), kref, state.secrets.clone());
            p.fetch_model_ids().await.map_err(AppError::Llm)?
        }
        _ => Vec::new(),
    };
    if !remote.is_empty() {
        state.remote_models.lock().unwrap().insert(acct.id.clone(), remote.clone());
    }
    Ok(catalog::merge_catalog(
        acct.provider_type,
        &remote,
        &acct.config.custom_models,
    ))
}

#[tauri::command]
pub fn llm_add_custom_model(
    state: State<'_, AppState>,
    account_id: String,
    model_id: String,
) -> AppResult<ProviderAccountView> {
    let updated = {
        let db = state.db.lock().unwrap();
        llm_repo::add_custom_model(&db, &account_id, &model_id)?
    };
    Ok(to_account_view(&state, updated, &[]))
}

#[tauri::command]
pub fn llm_disconnect_provider(state: State<'_, AppState>, account_id: String) -> AppResult<()> {
    let maybe_acct = {
        let db = state.db.lock().unwrap();
        let a = llm_repo::get_provider_account(&db, &account_id)?;
        llm_repo::delete_provider_account(&db, &account_id)?;
        a
    };
    if let Some(acct) = maybe_acct {
        if let Some(kref) = acct.keychain_ref {
            let _ = state.secrets.remove(&kref);
        }
    }
    sync_router_from_db(&state)?;
    Ok(())
}

#[tauri::command]
pub fn llm_list_task_routes(state: State<'_, AppState>) -> AppResult<Vec<TaskModelRoute>> {
    let db = state.db.lock().unwrap();
    llm_repo::list_task_routes(&db)
}

#[tauri::command]
pub fn llm_save_task_route(
    state: State<'_, AppState>,
    patch: TaskModelRoutePatch,
) -> AppResult<TaskModelRoute> {
    let db = state.db.lock().unwrap();
    llm_repo::save_task_route(&db, patch)
}

#[tauri::command]
pub fn llm_usage_dashboard(
    state: State<'_, AppState>,
    days: Option<u32>,
) -> AppResult<UsageDashboard> {
    let db = state.db.lock().unwrap();
    llm_repo::usage_dashboard(&db, days.unwrap_or(7).clamp(1, 90))
}

#[tauri::command]
pub fn llm_list_prompts(state: State<'_, AppState>) -> AppResult<Vec<PromptTemplate>> {
    let db = state.db.lock().unwrap();
    prompts::list_active_prompts(&db)
}

#[tauri::command]
pub fn llm_save_prompt_override(
    state: State<'_, AppState>,
    task_type: TaskType,
    system_prompt: String,
    user_template: String,
) -> AppResult<PromptTemplate> {
    let db = state.db.lock().unwrap();
    prompts::save_prompt_override(&db, task_type, &system_prompt, &user_template)
}

#[tauri::command]
pub fn llm_reset_prompt(
    state: State<'_, AppState>,
    task_type: TaskType,
) -> AppResult<PromptTemplate> {
    let db = state.db.lock().unwrap();
    prompts::reset_prompt_to_default(&db, task_type)
}

// ---------------------------------------------------------------------------
// Phase 2: Style Studio & Style Test Bench (STY-01..06, STY-09)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn style_get_active_guide(state: State<'_, AppState>) -> AppResult<StyleGuide> {
    let db = state.db.lock().unwrap();
    style_repo::get_active_style_guide(&db, &state.profile_id)
}

#[tauri::command]
pub fn style_list_guide_versions(state: State<'_, AppState>) -> AppResult<Vec<StyleGuide>> {
    let db = state.db.lock().unwrap();
    style_repo::list_style_guide_versions(&db, &state.profile_id)
}

#[tauri::command]
pub fn style_save_guide_version(
    state: State<'_, AppState>,
    input: StyleGuideInput,
) -> AppResult<StyleGuide> {
    let db = state.db.lock().unwrap();
    style_repo::save_style_guide_version(&db, &state.profile_id, input)
}

#[tauri::command]
pub fn style_activate_guide_version(
    state: State<'_, AppState>,
    version: u32,
) -> AppResult<StyleGuide> {
    let db = state.db.lock().unwrap();
    style_repo::activate_style_guide_version(&db, &state.profile_id, version)
}

#[tauri::command]
pub fn style_list_examples(
    state: State<'_, AppState>,
    kind: Option<ExampleKind>,
) -> AppResult<Vec<StyleExample>> {
    let db = state.db.lock().unwrap();
    style_repo::list_style_examples(&db, &state.profile_id, kind)
}

#[tauri::command]
pub fn style_search_examples(
    state: State<'_, AppState>,
    kind: Option<ExampleKind>,
    query: String,
    limit: Option<usize>,
) -> AppResult<Vec<RetrievedExample>> {
    let db = state.db.lock().unwrap();
    style_repo::search_style_examples(&db, &state.profile_id, kind, &query, limit.unwrap_or(20))
}

#[tauri::command]
pub fn style_upsert_example(
    state: State<'_, AppState>,
    input: StyleExampleInput,
) -> AppResult<StyleExample> {
    let db = state.db.lock().unwrap();
    style_repo::upsert_style_example(&db, &state.profile_id, input)
}

#[tauri::command]
pub fn style_delete_example(state: State<'_, AppState>, example_id: String) -> AppResult<()> {
    let db = state.db.lock().unwrap();
    style_repo::delete_style_example(&db, &state.profile_id, &example_id)
}

#[tauri::command]
pub fn style_parse_pasted_examples(
    raw_text: String,
    kind: ExampleKind,
) -> Vec<ImportCandidate> {
    bench::parse_pasted_examples(&raw_text, kind)
}

#[tauri::command]
pub async fn style_import_examples_from_browser(
    state: State<'_, AppState>,
    handle: Option<String>,
    kind: ExampleKind,
) -> AppResult<Vec<ImportCandidate>> {
    bench::import_examples_from_browser(&state.browser, handle.as_deref(), kind).await
}

#[tauri::command]
pub async fn style_run_bench(
    state: State<'_, AppState>,
    input: StyleBenchInput,
) -> AppResult<StyleBenchResult> {
    {
        let db = state.db.lock().unwrap();
        onboarding::require_acknowledged(&db)?;
    }
    bench::run_style_bench(&state.db, &state.llm_router, &state.profile_id, input).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn sync_profile_state_seeds_cookies_and_skips_singleton_locks() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("x-default");
        let dst = dir.path().join("x-publish");
        std::fs::create_dir_all(src.join("Default").join("Local Storage")).unwrap();
        std::fs::write(src.join("Local State"), b"{}").unwrap();
        std::fs::write(src.join("Default").join("Cookies"), b"cookie-v1").unwrap();
        std::fs::write(src.join("Default").join("Preferences"), b"{\"a\":1}").unwrap();
        std::fs::write(src.join("Default").join("Local Storage").join("0001.ldb"), b"data").unwrap();
        std::fs::write(src.join("Default").join("Local Storage").join("LOCK"), b"lock").unwrap();
        std::fs::write(src.join("Default").join("Local Storage").join("SingletonSocket"), b"sock").unwrap();

        assert!(sync_profile_state_if_newer(&src, &dst));
        assert_eq!(std::fs::read(dst.join("Default").join("Cookies")).unwrap(), b"cookie-v1");
        assert_eq!(std::fs::read(dst.join("Local State")).unwrap(), b"{}");
        assert!(dst.join("Default").join("Local Storage").join("0001.ldb").exists());
        assert!(!dst.join("Default").join("Local Storage").join("LOCK").exists());
        assert!(!dst.join("Default").join("Local Storage").join("SingletonSocket").exists());

        // Second sync without newer Cookies should be a no-op.
        assert!(!sync_profile_state_if_newer(&src, &dst));
    }
}

