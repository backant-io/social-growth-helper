//! Direct xAI API-key provider (LLM-05). Never used unless explicitly configured on a route.

use super::super::{
    classify_reqwest_error, LlmError, LlmErrorKind, LlmProvider, LlmRawResponse, LlmRequest,
    ProviderCapabilities, ProviderHealth, ProviderStatus, ProviderType,
};
use super::xai_oauth::{build_openai_chat_payload, parse_openai_chat_response};
use crate::secrets::SecretsService;
use crate::util::now_rfc3339;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const XAI_API_BASE: &str = "https://api.x.ai/v1";

pub struct XaiApiProvider {
    account_id: String,
    keychain_ref: String,
    secrets: Arc<SecretsService>,
    client: Client,
}

impl XaiApiProvider {
    pub fn new(
        account_id: impl Into<String>,
        keychain_ref: impl Into<String>,
        secrets: Arc<SecretsService>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            account_id: account_id.into(),
            keychain_ref: keychain_ref.into(),
            secrets,
            client,
        }
    }

    fn load_key(&self) -> Result<String, LlmError> {
        self.secrets
            .load(&self.keychain_ref)
            .map_err(|e| LlmError::new(LlmErrorKind::InvalidConfig, e.to_string()))?
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| {
                LlmError::new(
                    LlmErrorKind::AuthExpired,
                    "No xAI API key found in Keychain. Save your key in Settings.",
                )
            })
    }

    pub async fn fetch_model_ids(&self) -> Result<Vec<String>, LlmError> {
        let key = self.load_key()?;
        let res = self
            .client
            .get(format!("{XAI_API_BASE}/models"))
            .timeout(Duration::from_secs(10))
            .bearer_auth(&key)
            .send()
            .await
            .map_err(|e| classify_reqwest_error("api.x.ai", &e))?;
        if !res.status().is_success() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                format!("xAI /models returned HTTP {}", res.status()),
            ));
        }
        let v: Value = res
            .json()
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e.to_string()))?;
        let ids = v["data"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|m| m["id"].as_str().map(ToString::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Ok(ids)
    }
}

#[async_trait]
impl LlmProvider for XaiApiProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::XaiApi
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            vision: true,
            json_schema: true,
            streaming: true,
            thinking: true,
        }
    }

    async fn health_check(&self) -> Result<ProviderStatus, LlmError> {
        let checked_at = now_rfc3339();
        let key = match self.load_key() {
            Ok(k) => k,
            Err(e) => {
                return Ok(ProviderStatus {
                    health: ProviderHealth::NeedsLogin,
                    message: e.message,
                    checked_at,
                    details: None,
                })
            }
        };

        match self
            .client
            .get(format!("{XAI_API_BASE}/models"))
            .timeout(Duration::from_secs(10))
            .bearer_auth(&key)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => Ok(ProviderStatus {
                health: ProviderHealth::Connected,
                message: "xAI API key verified".into(),
                checked_at,
                details: None,
            }),
            Ok(res) if res.status().as_u16() == 401 || res.status().as_u16() == 403 => Ok(ProviderStatus {
                health: ProviderHealth::Expired,
                message: "xAI API key was rejected (HTTP 401/403)".into(),
                checked_at,
                details: None,
            }),
            Ok(res) => Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("api.x.ai returned HTTP {}", res.status()),
                checked_at,
                details: None,
            }),
            Err(e) => {
                let err = classify_reqwest_error("api.x.ai", &e);
                Ok(ProviderStatus {
                    health: if err.kind == LlmErrorKind::Offline {
                        ProviderHealth::Offline
                    } else {
                        ProviderHealth::Error
                    },
                    message: err.message,
                    checked_at,
                    details: None,
                })
            }
        }
    }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError> {
        let key = self.load_key()?;
        let payload = build_openai_chat_payload(req);
        let t0 = Instant::now();

        let res = self
            .client
            .post(format!("{XAI_API_BASE}/chat/completions"))
            .bearer_auth(&key)
            .json(&payload)
            .send()
            .await
            .map_err(|e| classify_reqwest_error("api.x.ai", &e))?;

        let status = res.status();
        let body: Value = res
            .json()
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("bad xAI API JSON: {e}")))?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(LlmError::new(
                LlmErrorKind::AuthExpired,
                "xAI API key rejected (HTTP 401/403).",
            ));
        }
        if status.as_u16() == 429 {
            return Err(LlmError::new(LlmErrorKind::RateLimited, "xAI API rate limit reached (429)."));
        }
        if !status.is_success() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                format!("xAI API HTTP {status}: {body}"),
            ));
        }

        parse_openai_chat_response(&body, t0.elapsed().as_millis() as u64)
    }
}
