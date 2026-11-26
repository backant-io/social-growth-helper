//! Adapts the Phase 2 [`LlmRouter`] to Phase 5's [`StructuredLlm`] seam. Unset routes surface as
//! [`LlmError::NoModel`] so the UI can point the user at model routing instead of failing opaquely.

use super::llm::{JsonRequest, LlmError, StructuredLlm};
use crate::llm::router::LlmRouter;
use crate::llm::LlmErrorKind;
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
        self.router
            .complete_json_for_task(&self.db, req.task, &req.system, &req.user)
            .await
            .map_err(|e| match e.kind {
                LlmErrorKind::UnsetRoute => LlmError::NoModel(req.task),
                _ => LlmError::Failed(e.message),
            })
    }
}
