use crate::browser::discovery::{self, BrowserInstall};
use crate::browser::service::{VisibleTweet, X_HOME_URL};
use crate::browser::ChromeLaunchConfig;
use crate::error::{AppError, AppResult};
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
    let models = catalog::merge_catalog(acct.provider_type, remote_models, &acct.config.custom_models);
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
        .llm
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
        .llm
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
        .llm
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
        .llm
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
        .llm
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
    bench::run_style_bench(&state.db, &state.llm, &state.profile_id, input).await
}

