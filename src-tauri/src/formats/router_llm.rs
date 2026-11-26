//! Adapts the Phase 2 [`LlmRouter`] to Phase 5's [`StructuredLlm`] seam. Unset routes surface as
//! [`LlmError::NoModel`] so the UI can point the user at model routing instead of failing opaquely.

use super::llm::{JsonRequest, LlmError, StructuredLlm};
use crate::llm::prompts::resolve_prompt;
use crate::llm::router::LlmRouter;
use crate::llm::{LlmErrorKind, TaskType};
use async_trait::async_trait;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub struct RouterLlm {
    router: Arc<LlmRouter>,
    db: Arc<Mutex<Connection>>,
}

impl RouterLlm {
    pub fn new(router: Arc<LlmRouter>, db: Arc<Mutex<Connection>>) -> Self {
        Self { router, db }
    }
}

#[async_trait]
impl StructuredLlm for RouterLlm {
    async fn complete_json(&self, req: JsonRequest) -> Result<String, LlmError> {
        let (system, user, version) = match (&req.slots, TaskType::parse(req.task)) {
            (Some(slots), Some(task)) => match resolve_prompt(&self.db.lock().unwrap(), task, &req.system, &req.user, slots) {
                Ok(p) => (p.system, p.user, Some(p.version)),
                Err(_) => (req.system.clone(), req.user.clone(), None),
            },
            _ => (req.system.clone(), req.user.clone(), None),
        };
        self.router
            .complete_json_for_task_versioned(&self.db, req.task, &system, &user, version)
            .await
            .map_err(|e| match e.kind {
                LlmErrorKind::UnsetRoute => LlmError::NoModel(req.task),
                _ => LlmError::Failed(e.message),
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::prompts::{reset_prompt_to_default, save_prompt_override};
    use crate::llm::providers::mock::MockLlmProvider;
    use crate::llm::repo::{save_task_route, upsert_provider_account, ProviderAccount, ProviderAccountConfig, TaskModelRoutePatch};
    use crate::llm::{LlmContentPart, ProviderCapabilities, ProviderType};
    use crate::completion::PromptSlots;

    #[tokio::test]
    async fn creator_calls_use_the_stored_prompt_and_log_its_version() {
        let dir = tempfile::tempdir().unwrap();
        let (conn, _) = crate::storage::open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let db = Arc::new(Mutex::new(conn));
        let pt = ProviderType::AnthropicApi;
        upsert_provider_account(
            &db.lock().unwrap(),
            &ProviderAccount {
                id: "p1".into(),
                provider_type: pt,
                label: "p1".into(),
                keychain_ref: None,
                billing_lane: pt.billing_lane(),
                status: "connected".into(),
                last_verified_at: None,
                last_error: None,
                config: ProviderAccountConfig::default(),
            },
        )
        .unwrap();
        save_task_route(
            &db.lock().unwrap(),
            TaskModelRoutePatch {
                task_type: TaskType::OgcGeneration,
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
        let router = Arc::new(LlmRouter::new());
        let mock = Arc::new(MockLlmProvider::new("p1", pt, ProviderCapabilities { vision: false, json_schema: true, streaming: false, thinking: false }));
        for _ in 0..3 {
            mock.push_ok("{\"variants\":[]}");
        }
        router.register_provider(mock.clone());
        let llm = RouterLlm::new(router, db.clone());
        let req = || JsonRequest {
            task: "ogc_generation",
            system: "built sys".into(),
            user: "built usr".into(),
            slots: Some(PromptSlots { vars: vec![("topic_seed", "cats".into())], required: vec!["topic_seed"], guard: "GUARD".into(), extra: "LIMIT".into() }),
        };
        llm.complete_json(req()).await.unwrap();
        save_prompt_override(&db.lock().unwrap(), TaskType::OgcGeneration, "my voice", "Topic: {{topic_seed}}").unwrap();
        llm.complete_json(req()).await.unwrap();
        reset_prompt_to_default(&db.lock().unwrap(), TaskType::OgcGeneration).unwrap();
        llm.complete_json(req()).await.unwrap();

        let calls = mock.recorded_calls();
        assert_eq!(calls[0].system_prompt.as_deref(), Some("built sys"));
        assert_eq!(calls[1].system_prompt.as_deref(), Some("my voice\n\nGUARD"));
        let text = calls[1].messages[0].parts.iter().find_map(|p| if let LlmContentPart::Text { text } = p { Some(text.as_str()) } else { None });
        assert_eq!(text, Some("Topic: cats\n\nLIMIT"));
        assert_eq!(calls[2].system_prompt.as_deref(), Some("built sys"));
        let conn = db.lock().unwrap();
        let mut st = conn.prepare("SELECT prompt_version FROM llm_usage_logs WHERE task_type='ogc_generation' ORDER BY rowid").unwrap();
        let logged: Vec<Option<u32>> = st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect();
        assert_eq!(logged, vec![Some(1), Some(2), Some(1)]);
    }
}
