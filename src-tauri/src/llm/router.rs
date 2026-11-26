//! Central LLM Router (LLM-01, LLM-08, LLM-11, LLM-12, LLM-13, LLM-14, LLM-17, LLM-21).

use super::catalog;
use super::providers::{
    anthropic_api::AnthropicApiProvider, claude_cli::ClaudeCliProvider, grok_web::GrokWebTabProvider,
    xai_api::XaiApiProvider, xai_oauth::XaiOAuthProvider,
};
use super::repo::{self, ProviderAccount, TaskModelRoute, UsageEntry};
use super::schema::{extract_json_value, validate_json_schema};
use super::{
    JsonSchemaSpec, LlmError, LlmErrorKind, LlmMessage, LlmProvider, LlmRawResponse, LlmRequest, LlmResponse,
    ProviderStatus, ProviderType, TaskType,
};
use crate::browser::service::BrowserService;
use crate::secrets::SecretsService;
use rusqlite::Connection;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone)]
pub struct RouteOverride {
    pub provider_account_id: String,
    pub model_id: String,
    pub reasoning_effort: Option<String>,
    pub temperature: Option<f32>,
}

pub struct LlmRouter {
    providers: Mutex<HashMap<String, Arc<dyn LlmProvider>>>,
}

impl Default for LlmRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl LlmRouter {
    pub fn new() -> Self {
        Self {
            providers: Mutex::new(HashMap::new()),
        }
    }

    pub fn register_provider(&self, provider: Arc<dyn LlmProvider>) {
        self.providers
            .lock()
            .unwrap()
            .insert(provider.account_id().to_string(), provider);
    }

    pub fn clear_providers(&self) {
        self.providers.lock().unwrap().clear();
    }

    pub fn get_provider(&self, account_id: &str) -> Option<Arc<dyn LlmProvider>> {
        self.providers.lock().unwrap().get(account_id).cloned()
    }

    pub fn build_provider_for_account(
        acct: &ProviderAccount,
        secrets: Arc<SecretsService>,
        browser: Arc<BrowserService>,
    ) -> Arc<dyn LlmProvider> {
        match acct.provider_type {
            ProviderType::ClaudeCli => Arc::new(ClaudeCliProvider::new(
                acct.id.clone(),
                acct.config.cli_binary_path.as_deref().map(PathBuf::from),
            )),
            ProviderType::AnthropicApi => Arc::new(AnthropicApiProvider::new(
                acct.id.clone(),
                acct.keychain_ref.clone().unwrap_or_else(|| format!("provider:{}", acct.id)),
                secrets,
            )),
            ProviderType::XaiOAuth => Arc::new(XaiOAuthProvider::new(
                acct.id.clone(),
                acct.keychain_ref.clone().unwrap_or_else(|| format!("provider:{}", acct.id)),
                acct.config.endpoint_override.clone(),
                secrets,
            )),
            ProviderType::XaiApi => Arc::new(XaiApiProvider::new(
                acct.id.clone(),
                acct.keychain_ref.clone().unwrap_or_else(|| format!("provider:{}", acct.id)),
                secrets,
            )),
            ProviderType::GrokWeb => Arc::new(GrokWebTabProvider::new(acct.id.clone(), browser)),
        }
    }

    /// Rebuilds the in-memory provider registry from SQLite `provider_accounts`.
    pub fn sync_from_db(
        &self,
        conn: &Connection,
        secrets: Arc<SecretsService>,
        browser: Arc<BrowserService>,
    ) -> Result<(), LlmError> {
        let accounts = repo::list_provider_accounts(conn)
            .map_err(|e| LlmError::new(LlmErrorKind::Other, e.to_string()))?;
        let mut map = self.providers.lock().unwrap();
        map.clear();
        for acct in accounts {
            let p = Self::build_provider_for_account(&acct, secrets.clone(), browser.clone());
            map.insert(acct.id, p);
        }
        Ok(())
    }

    pub async fn health_check_account(&self, account_id: &str) -> Result<ProviderStatus, LlmError> {
        let provider = self.get_provider(account_id).ok_or_else(|| {
            LlmError::new(LlmErrorKind::InvalidConfig, "provider account is not registered")
        })?;
        provider.health_check().await
    }

    /// Executes an LLM request for `req.task_type` (or an explicit `override_route` for side-by-side comparison).
    pub async fn complete(
        &self,
        db: &Mutex<Connection>,
        mut req: LlmRequest,
        override_route: Option<RouteOverride>,
    ) -> Result<LlmResponse, LlmError> {
        let (route, accounts_by_id, calls_last_hour) = {
            let conn = db.lock().unwrap();
            let route = repo::get_task_route(&conn, req.task_type)
                .map_err(|e| LlmError::for_task(LlmErrorKind::Other, req.task_type, e.to_string()))?;
            let accounts = repo::list_provider_accounts(&conn)
                .map_err(|e| LlmError::for_task(LlmErrorKind::Other, req.task_type, e.to_string()))?;
            let calls_last_hour = repo::calls_in_last_hour(&conn, req.task_type).unwrap_or(0);
            let map: HashMap<String, ProviderAccount> =
                accounts.into_iter().map(|a| (a.id.clone(), a)).collect();
            (route, map, calls_last_hour)
        };

        // 1. Per-task hourly rate guard (LLM-12).
        if let Some(cap) = route.max_calls_per_hour {
            if calls_last_hour >= cap {
                return Err(LlmError::for_task(
                    LlmErrorKind::RateGuardBlocked,
                    req.task_type,
                    format!(
                        "Rate guard blocked {}: {calls_last_hour}/{cap} calls used in the last hour.",
                        req.task_type.label()
                    ),
                ));
            }
        }

        // 2. Resolve primary target or enforce unset-route guard (LLM-21).
        let (primary_acc_id, primary_model_id, effort, temp) = match override_route {
            Some(ov) => (
                ov.provider_account_id,
                ov.model_id,
                ov.reasoning_effort.or(route.reasoning_effort.clone()),
                ov.temperature.or(route.temperature),
            ),
            None => match (&route.provider_account_id, &route.model_id) {
                (Some(a), Some(m)) => (
                    a.clone(),
                    m.clone(),
                    route.reasoning_effort.clone(),
                    route.temperature,
                ),
                _ => {
                    return Err(LlmError::for_task(
                        LlmErrorKind::UnsetRoute,
                        req.task_type,
                        format!(
                            "No model is chosen for {}. Pick a provider and model to run this task.",
                            req.task_type.label()
                        ),
                    ));
                }
            },
        };

        req.model_id = primary_model_id.clone();
        if req.reasoning_effort.is_none() {
            req.reasoning_effort = effort;
        }
        if req.temperature.is_none() {
            req.temperature = temp;
        }

        let primary_res = self
            .execute_on_provider(db, &accounts_by_id, &primary_acc_id, &req, false)
            .await;

        match primary_res {
            Ok((raw, structured, acct)) => Ok(LlmResponse {
                text: raw.text,
                structured_json: structured,
                model_id: req.model_id,
                provider_type: acct.provider_type,
                provider_account_id: acct.id,
                provider_label: acct.label,
                billing_lane: acct.billing_lane,
                usage: raw.usage,
                latency_ms: raw.latency_ms,
                used_fallback: false,
                fallback_reason: None,
                prompt_version: req.prompt_version,
            }),
            Err(primary_err) => {
                // 3. Explicit fallback routing only (LLM-11) — never silent switch.
                if primary_err.is_retryable_with_fallback() {
                    if let (Some(fb_acc_id), Some(fb_model_id)) =
                        (route.fallback_account_id.clone(), route.fallback_model_id.clone())
                    {
                        let mut fb_req = req.clone();
                        fb_req.model_id = fb_model_id;
                        match self
                            .execute_on_provider(db, &accounts_by_id, &fb_acc_id, &fb_req, true)
                            .await
                        {
                            Ok((raw, structured, fb_acct)) => {
                                return Ok(LlmResponse {
                                    text: raw.text,
                                    structured_json: structured,
                                    model_id: fb_req.model_id,
                                    provider_type: fb_acct.provider_type,
                                    provider_account_id: fb_acct.id,
                                    provider_label: fb_acct.label,
                                    billing_lane: fb_acct.billing_lane,
                                    usage: raw.usage,
                                    latency_ms: raw.latency_ms,
                                    used_fallback: true,
                                    fallback_reason: Some(primary_err.message),
                                    prompt_version: fb_req.prompt_version,
                                });
                            }
                            Err(fb_err) => {
                                return Err(LlmError::for_task(
                                    fb_err.kind,
                                    req.task_type,
                                    format!(
                                        "Primary failed ({}); explicit fallback also failed ({})",
                                        primary_err.message, fb_err.message
                                    ),
                                ));
                            }
                        }
                    }
                }
                Err(LlmError::for_task(
                    primary_err.kind,
                    req.task_type,
                    primary_err.message,
                ))
            }
        }
    }

    async fn execute_on_provider(
        &self,
        db: &Mutex<Connection>,
        accounts_by_id: &HashMap<String, ProviderAccount>,
        account_id: &str,
        req: &LlmRequest,
        used_fallback: bool,
    ) -> Result<(LlmRawResponse, Option<Value>, ProviderAccount), LlmError> {
        let acct = accounts_by_id.get(account_id).cloned().ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::InvalidConfig,
                format!("configured provider account {account_id} no longer exists"),
            )
        })?;
        let provider = self.get_provider(account_id).ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::InvalidConfig,
                format!("provider {} is not initialized", acct.label),
            )
        })?;

        // Vision capability check (LLM-14).
        if req.has_images() {
            let model_caps = catalog::infer_capabilities(acct.provider_type, &req.model_id);
            if !provider.capabilities().vision || !model_caps.vision {
                return Err(LlmError::new(
                    LlmErrorKind::UnsupportedCapability,
                    format!(
                        "{} ({}) does not support vision inputs required by this request.",
                        acct.label, req.model_id
                    ),
                ));
            }
        }

        let res = self.call_with_schema_retry(provider.as_ref(), req).await;
        match &res {
            Ok((raw, _)) => {
                let conn = db.lock().unwrap();
                let _ = repo::record_usage(
                    &conn,
                    &UsageEntry {
                        task_type: req.task_type,
                        provider_account_id: Some(acct.id.clone()),
                        provider_type: acct.provider_type,
                        model_id: req.model_id.clone(),
                        prompt_version: req.prompt_version,
                        input_tokens: raw.usage.input_tokens,
                        output_tokens: raw.usage.output_tokens,
                        latency_ms: raw.latency_ms,
                        used_fallback,
                        success: true,
                        error_kind: None,
                    },
                );
            }
            Err(err) => {
                let conn = db.lock().unwrap();
                let _ = repo::record_usage(
                    &conn,
                    &UsageEntry {
                        task_type: req.task_type,
                        provider_account_id: Some(acct.id.clone()),
                        provider_type: acct.provider_type,
                        model_id: req.model_id.clone(),
                        prompt_version: req.prompt_version,
                        input_tokens: 0,
                        output_tokens: 0,
                        latency_ms: 0,
                        used_fallback,
                        success: false,
                        error_kind: Some(err.error_code().to_string()),
                    },
                );
            }
        }

        res.map(|(raw, structured)| (raw, structured, acct))
    }

    async fn call_with_schema_retry(
        &self,
        provider: &dyn LlmProvider,
        req: &LlmRequest,
    ) -> Result<(LlmRawResponse, Option<Value>), LlmError> {
        let first = provider.complete(req).await?;
        let Some(spec) = &req.json_schema else {
            return Ok((first, None));
        };

        if let Some(val) = Self::validate_raw_against_schema(&first, spec).ok() {
            return Ok((first, Some(val)));
        }

        // One bounded retry with exact validation error feedback (LLM-13).
        let err_reason = Self::validate_raw_against_schema(&first, spec)
            .err()
            .unwrap_or_else(|| "invalid JSON".into());
        let mut retry_req = req.clone();
        retry_req
            .messages
            .push(LlmMessage::assistant_text(&first.text));
        retry_req.messages.push(LlmMessage::user_text(format!(
            "Your previous response failed JSON Schema validation ({err_reason}). Return ONLY valid JSON matching schema `{}`.",
            spec.name
        )));

        let second = provider.complete(&retry_req).await?;
        let val = Self::validate_raw_against_schema(&second, spec).map_err(|e| {
            LlmError::new(
                LlmErrorKind::SchemaValidation,
                format!("Structured JSON validation failed after retry: {e}"),
            )
        })?;
        let combined_usage = super::LlmUsage {
            input_tokens: first.usage.input_tokens + second.usage.input_tokens,
            output_tokens: first.usage.output_tokens + second.usage.output_tokens,
        };
        Ok((
            LlmRawResponse {
                text: second.text,
                structured_json: Some(val.clone()),
                usage: combined_usage,
                latency_ms: first.latency_ms + second.latency_ms,
            },
            Some(val),
        ))
    }

    fn validate_raw_against_schema(raw: &LlmRawResponse, spec: &JsonSchemaSpec) -> Result<Value, String> {
        let value = match &raw.structured_json {
            Some(v) => v.clone(),
            None => extract_json_value(&raw.text)?,
        };
        validate_json_schema(&spec.schema, &value).map_err(|errs| errs.join("; "))?;
        Ok(value)
    }

    /// Clean JSON completion helper for downstream feature adapters (e.g. Phase 5 `formats::llm::StructuredLlm`).
    /// Routes by task slot string (`"format_extraction"`, `"ogc_generation"`, etc.), enforces unset-route
    /// and rate-limit guards, extracts clean JSON if fence-wrapped, and returns the serialized JSON string.
    pub async fn complete_json_for_task(
        &self,
        db: &Mutex<Connection>,
        task_name: &str,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, LlmError> {
        let task_type = TaskType::parse(task_name).ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::InvalidConfig,
                format!("unknown LLM task slot '{task_name}'"),
            )
        })?;
        let req = LlmRequest {
            task_type,
            model_id: String::new(),
            system_prompt: (!system_prompt.trim().is_empty()).then(|| system_prompt.to_string()),
            messages: vec![LlmMessage::user_text(user_prompt)],
            temperature: None,
            reasoning_effort: None,
            max_tokens: None,
            json_schema: None,
            prompt_version: None,
        };
        let resp = self.complete(db, req, None).await?;
        if let Some(v) = resp.structured_json {
            return Ok(v.to_string());
        }
        Ok(extract_json_value(&resp.text)
            .map(|v| v.to_string())
            .unwrap_or(resp.text))
    }
}

pub fn unset_route_tasks(routes: &[TaskModelRoute]) -> Vec<TaskType> {
    routes
        .iter()
        .filter(|r| r.provider_account_id.is_none() || r.model_id.is_none())
        .map(|r| r.task_type)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::mock::MockLlmProvider;
    use crate::llm::repo::{ProviderAccountConfig, TaskModelRoutePatch};
    use crate::llm::{BillingLane, LlmContentPart, LlmRole, ProviderCapabilities};
    use crate::storage::open;
    use serde_json::json;
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, Mutex<Connection>, LlmRouter) {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        (dir, Mutex::new(conn), LlmRouter::new())
    }

    fn add_account(db: &Mutex<Connection>, id: &str, pt: ProviderType) {
        let conn = db.lock().unwrap();
        repo::upsert_provider_account(
            &conn,
            &ProviderAccount {
                id: id.into(),
                provider_type: pt,
                label: format!("{id} label"),
                keychain_ref: None,
                billing_lane: pt.billing_lane(),
                status: "connected".into(),
                last_verified_at: None,
                last_error: None,
                config: ProviderAccountConfig::default(),
            },
        )
        .unwrap();
    }

    fn basic_req(task: TaskType) -> LlmRequest {
        LlmRequest {
            task_type: task,
            model_id: String::new(),
            system_prompt: None,
            messages: vec![LlmMessage::user_text("hi")],
            temperature: None,
            reasoning_effort: None,
            max_tokens: None,
            json_schema: None,
            prompt_version: Some(1),
        }
    }

    #[tokio::test]
    async fn unset_route_guard_blocks_cleanly_without_silent_fallback() {
        let (_d, db, router) = setup();
        let err = router
            .complete(&db, basic_req(TaskType::BrowserNavigation), None)
            .await
            .unwrap_err();
        assert_eq!(err.kind, LlmErrorKind::UnsetRoute);
        assert!(err.message.contains("Browser Navigation"));
    }

    #[tokio::test]
    async fn malformed_json_retries_once_and_validates() {
        let (_d, db, router) = setup();
        add_account(&db, "p1", ProviderType::ClaudeCli);
        {
            let conn = db.lock().unwrap();
            repo::save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::PostScoring,
                    provider_account_id: Some("p1".into()),
                    model_id: Some("sonnet".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: None,
                    fallback_model_id: None,
                    max_calls_per_hour: None,
                },
            )
            .unwrap();
        }

        let mock = Arc::new(MockLlmProvider::new(
            "p1",
            ProviderType::ClaudeCli,
            ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
        ));
        mock.push_ok("not json");
        mock.push_ok(r#"{"score": 88}"#);
        router.register_provider(mock.clone());

        let mut req = basic_req(TaskType::PostScoring);
        req.json_schema = Some(JsonSchemaSpec {
            name: "score".into(),
            schema: json!({
                "type": "object",
                "required": ["score"],
                "properties": { "score": { "type": "number" } }
            }),
        });
        let res = router.complete(&db, req, None).await.unwrap();
        assert_eq!(res.structured_json.unwrap()["score"], 88);
        assert_eq!(mock.recorded_calls().len(), 2, "retried exactly once");

        // Now verify double-invalid fails cleanly with SchemaValidation.
        mock.push_ok("bad 1");
        mock.push_ok("bad 2");
        let mut req2 = basic_req(TaskType::PostScoring);
        req2.json_schema = Some(JsonSchemaSpec {
            name: "score".into(),
            schema: json!({ "type": "object", "required": ["score"] }),
        });
        let err = router.complete(&db, req2, None).await.unwrap_err();
        assert_eq!(err.kind, LlmErrorKind::SchemaValidation);
    }

    #[tokio::test]
    async fn fallback_only_fires_when_explicitly_configured_and_records_badge() {
        let (_d, db, router) = setup();
        add_account(&db, "sub", ProviderType::XaiOAuth);
        add_account(&db, "api", ProviderType::XaiApi);

        // Without fallback configured, offline error surfaces directly (never silently switches to API).
        {
            let conn = db.lock().unwrap();
            repo::save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::ReplyDrafting,
                    provider_account_id: Some("sub".into()),
                    model_id: Some("grok-4.6".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: None,
                    fallback_model_id: None,
                    max_calls_per_hour: None,
                },
            )
            .unwrap();
        }
        let sub_mock = Arc::new(MockLlmProvider::new(
            "sub",
            ProviderType::XaiOAuth,
            ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
        ));
        sub_mock.push_err(LlmError::new(LlmErrorKind::RateLimited, "429"));
        let api_mock = Arc::new(MockLlmProvider::new(
            "api",
            ProviderType::XaiApi,
            ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true },
        ));
        api_mock.push_ok("from api");
        router.register_provider(sub_mock.clone());
        router.register_provider(api_mock.clone());

        let err = router.complete(&db, basic_req(TaskType::ReplyDrafting), None).await.unwrap_err();
        assert_eq!(err.kind, LlmErrorKind::RateLimited);
        assert_eq!(api_mock.recorded_calls().len(), 0);

        // With explicit fallback configured, fallback fires and response marks used_fallback=true.
        {
            let conn = db.lock().unwrap();
            repo::save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::ReplyDrafting,
                    provider_account_id: Some("sub".into()),
                    model_id: Some("grok-4.6".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: Some("api".into()),
                    fallback_model_id: Some("grok-4.6".into()),
                    max_calls_per_hour: None,
                },
            )
            .unwrap();
        }
        sub_mock.push_err(LlmError::new(LlmErrorKind::RateLimited, "429 quota"));
        let res = router.complete(&db, basic_req(TaskType::ReplyDrafting), None).await.unwrap();
        assert!(res.used_fallback);
        assert_eq!(res.billing_lane, BillingLane::Api);
        assert!(res.fallback_reason.unwrap().contains("429"));
    }

    #[tokio::test]
    async fn rate_guard_and_vision_capability_are_enforced() {
        let (_d, db, router) = setup();
        add_account(&db, "web", ProviderType::GrokWeb);
        {
            let conn = db.lock().unwrap();
            repo::save_task_route(
                &conn,
                TaskModelRoutePatch {
                    task_type: TaskType::DashboardBuilder,
                    provider_account_id: Some("web".into()),
                    model_id: Some("grok-web".into()),
                    reasoning_effort: None,
                    temperature: None,
                    fallback_account_id: None,
                    fallback_model_id: None,
                    max_calls_per_hour: Some(1),
                },
            )
            .unwrap();
        }
        let web_mock = Arc::new(MockLlmProvider::new(
            "web",
            ProviderType::GrokWeb,
            ProviderCapabilities { vision: false, json_schema: false, streaming: false, thinking: true },
        ));
        web_mock.push_ok("first");
        router.register_provider(web_mock);

        // Vision request rejected by GrokWeb capability flag.
        let mut vis = basic_req(TaskType::DashboardBuilder);
        vis.messages = vec![LlmMessage {
            role: LlmRole::User,
            parts: vec![LlmContentPart::ImageBase64 {
                media_type: "image/png".into(),
                data_base64: "AA==".into(),
            }],
        }];
        let vis_err = router.complete(&db, vis, None).await.unwrap_err();
        assert_eq!(vis_err.kind, LlmErrorKind::UnsupportedCapability);

        // First call succeeds, second call hits max_calls_per_hour = 1.
        assert!(router.complete(&db, basic_req(TaskType::DashboardBuilder), None).await.is_ok());
        let rate_err = router.complete(&db, basic_req(TaskType::DashboardBuilder), None).await.unwrap_err();
        assert_eq!(rate_err.kind, LlmErrorKind::RateGuardBlocked);
    }

    #[tokio::test]
    async fn complete_json_for_task_supports_format_extraction_and_ogc_generation() {
        let (_dir, db, router) = setup();
        add_account(&db, "p1", ProviderType::AnthropicApi);
        {
            let conn = db.lock().unwrap();
            for task in [TaskType::FormatExtraction, TaskType::OgcGeneration] {
                repo::save_task_route(
                    &conn,
                    TaskModelRoutePatch {
                        task_type: task,
                        provider_account_id: Some("p1".into()),
                        model_id: Some("m1".into()),
                        reasoning_effort: None,
                        temperature: None,
                        fallback_account_id: None,
                        fallback_model_id: None,
                        max_calls_per_hour: None,
                    },
                )
                .unwrap();
            }
        }
        let mock = Arc::new(MockLlmProvider::new(
            "p1",
            ProviderType::AnthropicApi,
            ProviderCapabilities { vision: true, json_schema: true, streaming: false, thinking: false },
        ));
        mock.push_ok("```json\n{\"name\":\"Contrarian Metric\"}\n```");
        mock.push_ok("{\"drafts\":[{\"text\":\"Shipped it.\"}]}");
        router.register_provider(mock);

        let f_out = router
            .complete_json_for_task(&db, "format_extraction", "Extract", "Post text")
            .await
            .unwrap();
        assert_eq!(f_out, "{\"name\":\"Contrarian Metric\"}");

        let o_out = router
            .complete_json_for_task(&db, "ogc_generation", "Draft", "Seed")
            .await
            .unwrap();
        assert!(o_out.contains("Shipped it."));
    }
}
