//! Plugs the Phase 2 [`LlmRouter`] into the Phase 3 [`CompletionProvider`] seam: scoring, drafting and the
//! vision fallback all run through the user's task routes, rate guards and fallbacks (LLM-15, LLM-20).

use super::catalog::merge_catalog;
use super::prompts::get_active_prompt;
use super::repo::{get_task_route, list_provider_accounts};
use super::router::{LlmRouter, RouteOverride};
use super::{LlmContentPart, LlmErrorKind, LlmMessage, LlmRequest, LlmRole, TaskType};
use crate::completion::{Completion, CompletionError, CompletionProvider, CompletionRequest, TaskSlot};
use rusqlite::Connection;
use serde::Serialize;
use std::sync::{Arc, Mutex};

pub struct RouterCompletion {
    router: Arc<LlmRouter>,
    db: Arc<Mutex<Connection>>,
}

impl RouterCompletion {
    pub fn new(router: Arc<LlmRouter>, db: Arc<Mutex<Connection>>) -> Self {
        Self { router, db }
    }
}

fn task_type(slot: TaskSlot) -> TaskType {
    match slot {
        TaskSlot::PostScoring => TaskType::PostScoring,
        TaskSlot::ReplyDrafting => TaskType::ReplyDrafting,
        TaskSlot::BrowserNavigation => TaskType::BrowserNavigation,
    }
}

fn media_type(b64: &str) -> &'static str {
    if b64.starts_with("iVBOR") {
        "image/png"
    } else {
        "image/jpeg"
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ModelOption {
    /// `account_id::model_id`, accepted as `model_hint`.
    pub value: String,
    pub label: String,
}

/// Every model of every connected account, for the "redraft with another model" picker.
pub fn model_options(conn: &Connection) -> Vec<ModelOption> {
    let accounts = list_provider_accounts(conn).unwrap_or_default();
    accounts
        .iter()
        .flat_map(|a| {
            merge_catalog(a.provider_type, &[], &a.config.custom_models)
                .into_iter()
                .map(move |m| ModelOption { value: format!("{}::{}", a.id, m.id), label: format!("{} - {}", a.label, m.label) })
        })
        .collect()
}

/// `account::model` exactly, or a loose word ("claude", "grok", a model id) matched against labels and ids.
pub fn resolve_hint(conn: &Connection, hint: &str) -> Option<RouteOverride> {
    let hint = hint.trim();
    let accounts = list_provider_accounts(conn).ok()?;
    let make = |account: &str, model: &str| RouteOverride { provider_account_id: account.to_string(), model_id: model.to_string(), reasoning_effort: None, temperature: None };
    if let Some((acct, model)) = hint.split_once("::") {
        return accounts.iter().any(|a| a.id == acct).then(|| make(acct, model));
    }
    let h = hint.to_lowercase();
    for a in &accounts {
        let models = merge_catalog(a.provider_type, &[], &a.config.custom_models);
        if let Some(m) = models.iter().find(|m| m.id.to_lowercase() == h) {
            return Some(make(&a.id, &m.id));
        }
    }
    for a in &accounts {
        let models = merge_catalog(a.provider_type, &[], &a.config.custom_models);
        let by_label = a.label.to_lowercase().contains(&h) || a.provider_type.as_str().contains(&h);
        if let Some(m) = models.iter().find(|m| by_label || m.id.to_lowercase().contains(&h)) {
            return Some(make(&a.id, &m.id));
        }
    }
    None
}

#[async_trait::async_trait]
impl CompletionProvider for RouterCompletion {
    fn is_configured(&self, task: TaskSlot) -> bool {
        let conn = self.db.lock().unwrap();
        get_task_route(&conn, task_type(task)).map(|r| r.provider_account_id.is_some() && r.model_id.is_some()).unwrap_or(false)
    }

    async fn complete(&self, req: CompletionRequest) -> Result<Completion, CompletionError> {
        let task = task_type(req.task);
        let prompt_version = get_active_prompt(&self.db.lock().unwrap(), task).ok().map(|p| p.version);
        let override_route = match req.model_hint.as_deref().filter(|h| !h.trim().is_empty()) {
            Some(h) => {
                let conn = self.db.lock().unwrap();
                Some(resolve_hint(&conn, h).ok_or_else(|| CompletionError::Failed(format!("no connected model matches \"{h}\"")))?)
            }
            None => None,
        };
        let mut parts = vec![LlmContentPart::Text { text: req.user.clone() }];
        parts.extend(req.images.iter().map(|b| LlmContentPart::ImageBase64 { media_type: media_type(b).into(), data_base64: b.clone() }));
        let llm = LlmRequest {
            task_type: task,
            model_id: String::new(),
            system_prompt: Some(req.system.clone()),
            messages: vec![LlmMessage { role: LlmRole::User, parts }],
            temperature: None,
            reasoning_effort: None,
            max_tokens: None,
            json_schema: None,
            prompt_version,
        };
        let resp = self.router.complete(&self.db, llm, override_route).await.map_err(|e| match e.kind {
            LlmErrorKind::UnsetRoute => CompletionError::NotConfigured(req.task),
            _ => CompletionError::Failed(e.to_string()),
        })?;
        let text = match (&resp.structured_json, resp.text.trim().is_empty()) {
            (Some(v), true) => v.to_string(),
            _ => resp.text,
        };
        Ok(Completion { text, model: resp.model_id })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::providers::mock::MockLlmProvider;
    use crate::llm::repo::TaskModelRoutePatch;

    #[test]
    fn media_type_is_sniffed_from_base64_magic() {
        assert_eq!(media_type("iVBORw0KGgo"), "image/png");
        assert_eq!(media_type("/9j/4AAQ"), "image/jpeg");
    }

    #[test]
    fn slots_map_to_task_routes() {
        assert_eq!(task_type(TaskSlot::BrowserNavigation).as_str(), TaskSlot::BrowserNavigation.id());
        assert_eq!(task_type(TaskSlot::PostScoring).as_str(), TaskSlot::PostScoring.id());
        assert_eq!(task_type(TaskSlot::ReplyDrafting).as_str(), TaskSlot::ReplyDrafting.id());
    }

    use crate::llm::repo::{upsert_provider_account, ProviderAccount, ProviderAccountConfig};
    use crate::llm::{ProviderCapabilities, ProviderType};

    fn rig() -> (tempfile::TempDir, RouterCompletion, Arc<MockLlmProvider>, Arc<MockLlmProvider>) {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _) = crate::storage::open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let router = Arc::new(LlmRouter::new());
        let caps = ProviderCapabilities { vision: true, json_schema: true, streaming: true, thinking: true };
        let mut mocks = vec![];
        for (id, pt) in [("cli", ProviderType::ClaudeCli), ("xai", ProviderType::XaiApi)] {
            upsert_provider_account(
                &db.lock().unwrap(),
                &ProviderAccount {
                    id: id.into(),
                    provider_type: pt,
                    label: format!("{id} account"),
                    keychain_ref: None,
                    billing_lane: pt.billing_lane(),
                    status: "connected".into(),
                    last_verified_at: None,
                    last_error: None,
                    config: ProviderAccountConfig::default(),
                },
            )
            .unwrap();
            let m = Arc::new(MockLlmProvider::new(id, pt, caps.clone()));
            router.register_provider(m.clone());
            mocks.push(m);
        }
        let b = RouterCompletion::new(router, db);
        (dir, b, mocks.remove(0), mocks.remove(0))
    }

    fn route(b: &RouterCompletion, task: TaskType, account: &str, model: &str) {
        crate::llm::repo::save_task_route(
            &b.db.lock().unwrap(),
            TaskModelRoutePatch {
                task_type: task,
                provider_account_id: Some(account.into()),
                model_id: Some(model.into()),
                reasoning_effort: None,
                temperature: None,
                fallback_account_id: None,
                fallback_model_id: None,
                max_calls_per_hour: None,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn unset_slot_is_reported_and_set_slot_sends_images_to_the_routed_model() {
        let (_d, b, cli, _xai) = rig();
        assert!(!b.is_configured(TaskSlot::BrowserNavigation));
        let req = || CompletionRequest::new(TaskSlot::BrowserNavigation, "sys".into(), "look".into(), false).with_images(vec!["iVBORw0".into()]);
        assert!(matches!(b.complete(req()).await.unwrap_err(), CompletionError::NotConfigured(TaskSlot::BrowserNavigation)));
        route(&b, TaskType::BrowserNavigation, "cli", "sonnet");
        assert!(b.is_configured(TaskSlot::BrowserNavigation));
        cli.push_ok("{\"action\":\"give_up\"}");
        let c = b.complete(req()).await.unwrap();
        assert!(c.text.contains("give_up"));
        let sent = &cli.recorded_calls()[0];
        assert!(sent.has_images());
        assert_eq!(sent.system_prompt.as_deref(), Some("sys"));
    }

    #[tokio::test]
    async fn production_calls_carry_the_active_prompt_version() {
        let (_d, b, cli, _xai) = rig();
        route(&b, TaskType::ReplyDrafting, "cli", "sonnet");
        cli.push_ok("{\"variants\":[]}");
        cli.push_ok("{\"variants\":[]}");
        let req = || CompletionRequest::new(TaskSlot::ReplyDrafting, "s".into(), "u".into(), true);
        b.complete(req()).await.unwrap();
        crate::llm::prompts::save_prompt_override(&b.db.lock().unwrap(), TaskType::ReplyDrafting, "sys v2", "usr v2").unwrap();
        b.complete(req()).await.unwrap();
        let calls = cli.recorded_calls();
        assert_eq!(calls[0].prompt_version, Some(1));
        assert_eq!(calls[1].prompt_version, Some(2));
        let logged: Vec<Option<u32>> = {
            let conn = b.db.lock().unwrap();
            let mut st = conn.prepare("SELECT prompt_version FROM llm_usage_logs WHERE task_type='reply_drafting' ORDER BY rowid").unwrap();
            st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(logged, vec![Some(1), Some(2)]);
    }

    #[tokio::test]
    async fn model_hint_overrides_the_route_for_one_call() {
        let (_d, b, cli, xai) = rig();
        route(&b, TaskType::ReplyDrafting, "cli", "sonnet");
        xai.push_ok("{\"variants\":[]}");
        let hint = Some("xai_api".to_string());
        let c = b.complete(CompletionRequest::new(TaskSlot::ReplyDrafting, "s".into(), "u".into(), true).with_model_hint(hint)).await.unwrap();
        assert!(!c.model.is_empty());
        assert_eq!(xai.recorded_calls().len(), 1);
        assert_eq!(cli.recorded_calls().len(), 0, "the routed model was not used");
        let err = b.complete(CompletionRequest::new(TaskSlot::ReplyDrafting, "s".into(), "u".into(), true).with_model_hint(Some("nonexistent-llm".into()))).await.unwrap_err();
        assert!(err.to_string().contains("no connected model"));
    }

    #[test]
    fn options_list_every_account_model_and_hints_resolve_exactly() {
        let (_d, b, _c, _x) = rig();
        let conn = b.db.lock().unwrap();
        let opts = model_options(&conn);
        assert!(opts.iter().any(|o| o.value.starts_with("cli::")) && opts.iter().any(|o| o.value.starts_with("xai::")));
        let first = &opts[0];
        let r = resolve_hint(&conn, &first.value).unwrap();
        assert_eq!(format!("{}::{}", r.provider_account_id, r.model_id), first.value);
        assert!(resolve_hint(&conn, "missing::x").is_none());
    }
}
