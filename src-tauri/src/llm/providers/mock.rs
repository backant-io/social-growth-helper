//! Scriptable mock provider for router, retry, capability, and fallback tests (LLM-01).

use super::super::{
    LlmError, LlmProvider, LlmRawResponse, LlmRequest, LlmUsage, ProviderCapabilities, ProviderHealth,
    ProviderStatus, ProviderType,
};
use crate::util::now_rfc3339;
use async_trait::async_trait;
use std::collections::VecDeque;
use std::sync::Mutex;

pub struct MockLlmProvider {
    pub account_id: String,
    pub provider_type: ProviderType,
    pub caps: ProviderCapabilities,
    pub responses: Mutex<VecDeque<Result<LlmRawResponse, LlmError>>>,
    pub calls: Mutex<Vec<LlmRequest>>,
    pub health: Mutex<ProviderHealth>,
}

impl MockLlmProvider {
    pub fn new(account_id: impl Into<String>, provider_type: ProviderType, caps: ProviderCapabilities) -> Self {
        Self {
            account_id: account_id.into(),
            provider_type,
            caps,
            responses: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            health: Mutex::new(ProviderHealth::Connected),
        }
    }

    pub fn push_ok(&self, text: impl Into<String>) {
        self.responses.lock().unwrap().push_back(Ok(LlmRawResponse {
            text: text.into(),
            structured_json: None,
            usage: LlmUsage { input_tokens: 50, output_tokens: 25 },
            latency_ms: 42,
        }));
    }

    pub fn push_err(&self, err: LlmError) {
        self.responses.lock().unwrap().push_back(Err(err));
    }

    pub fn recorded_calls(&self) -> Vec<LlmRequest> {
        self.calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        self.provider_type
    }

    fn capabilities(&self) -> ProviderCapabilities {
        self.caps
    }

    async fn health_check(&self) -> Result<ProviderStatus, LlmError> {
        let h = *self.health.lock().unwrap();
        Ok(ProviderStatus {
            health: h,
            message: format!("mock {:?}", h),
            checked_at: now_rfc3339(),
            details: None,
        })
    }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError> {
        self.calls.lock().unwrap().push(req.clone());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| {
                Ok(LlmRawResponse {
                    text: "mock-ok".into(),
                    structured_json: None,
                    usage: LlmUsage { input_tokens: 10, output_tokens: 5 },
                    latency_ms: 10,
                })
            })
    }
}
