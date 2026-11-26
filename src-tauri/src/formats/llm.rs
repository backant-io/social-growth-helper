//! The only seam between Phase 5 and the model router. [`router_llm::RouterLlm`] implements
//! [`StructuredLlm`] over the Phase 2 router; [`UnconfiguredLlm`] stays for tests and fails clearly.

use async_trait::async_trait;

pub const TASK_FORMAT_EXTRACTION: &str = "format_extraction";
pub const TASK_OGC_GENERATION: &str = "ogc_generation";

#[derive(Debug, Clone)]
pub struct JsonRequest {
    /// Routing slot: `format_extraction` or `ogc_generation`.
    pub task: &'static str,
    pub system: String,
    pub user: String,
    /// When set, an active prompt override for the task replaces `system` and `user` (LLM-19).
    pub slots: Option<crate::completion::PromptSlots>,
}

#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("No model is chosen for \"{0}\". Pick one in Settings > Model routing once the model router is connected; until then you can still build and edit formats by hand.")]
    NoModel(&'static str),
    #[error("model call failed: {0}")]
    Failed(String),
}

#[async_trait]
pub trait StructuredLlm: Send + Sync {
    /// Returns the model's raw text, which should contain one JSON object. Callers validate it.
    async fn complete_json(&self, req: JsonRequest) -> Result<String, LlmError>;
}

pub struct UnconfiguredLlm;

#[async_trait]
impl StructuredLlm for UnconfiguredLlm {
    async fn complete_json(&self, req: JsonRequest) -> Result<String, LlmError> {
        Err(LlmError::NoModel(req.task))
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// Replays canned replies and records every request, so prompts can be asserted on.
    #[derive(Clone, Default)]
    pub struct ScriptedLlm {
        pub replies: Arc<Mutex<VecDeque<Result<String, String>>>>,
        pub requests: Arc<Mutex<Vec<JsonRequest>>>,
    }

    impl ScriptedLlm {
        pub fn with(replies: Vec<Result<&str, &str>>) -> Self {
            let s = Self::default();
            *s.replies.lock().unwrap() = replies.into_iter().map(|r| r.map(String::from).map_err(String::from)).collect();
            s
        }
    }

    #[async_trait]
    impl StructuredLlm for ScriptedLlm {
        async fn complete_json(&self, req: JsonRequest) -> Result<String, LlmError> {
            self.requests.lock().unwrap().push(req);
            match self.replies.lock().unwrap().pop_front() {
                Some(Ok(s)) => Ok(s),
                Some(Err(e)) => Err(LlmError::Failed(e)),
                None => Err(LlmError::Failed("script exhausted".into())),
            }
        }
    }
}
