//! Direct Anthropic API-key provider (LLM-06). Escape hatch for subscription quota limits.

use super::super::{
    classify_reqwest_error, LlmContentPart, LlmError, LlmErrorKind, LlmProvider, LlmRawResponse, LlmRequest,
    LlmRole, LlmUsage, ProviderCapabilities, ProviderHealth, ProviderStatus, ProviderType,
};
use crate::secrets::SecretsService;
use crate::util::now_rfc3339;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const ANTHROPIC_API_BASE: &str = "https://api.anthropic.com/v1";
const ANTHROPIC_VERSION: &str = "2023-06-01";

pub struct AnthropicApiProvider {
    account_id: String,
    keychain_ref: String,
    secrets: Arc<SecretsService>,
    client: Client,
}

impl AnthropicApiProvider {
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
                    "No Anthropic API key found in Keychain. Save your key in Settings.",
                )
            })
    }

    pub async fn fetch_model_ids(&self) -> Result<Vec<String>, LlmError> {
        let key = self.load_key()?;
        let res = self
            .client
            .get(format!("{ANTHROPIC_API_BASE}/models"))
            .timeout(Duration::from_secs(10))
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await
            .map_err(|e| classify_reqwest_error("api.anthropic.com", &e))?;
        if !res.status().is_success() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                format!("Anthropic /models returned HTTP {}", res.status()),
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

pub fn build_anthropic_payload(req: &LlmRequest) -> Value {
    let mut sys = req.system_prompt.clone().unwrap_or_default();
    if let Some(spec) = &req.json_schema {
        if !sys.is_empty() {
            sys.push_str("\n\n");
        }
        sys.push_str(&format!(
            "Output ONLY valid JSON matching schema `{}`:\n{}",
            spec.name, spec.schema
        ));
    }

    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|msg| {
            let role = match msg.role {
                LlmRole::Assistant => "assistant",
                LlmRole::User | LlmRole::System => "user",
            };
            let blocks: Vec<Value> = msg
                .parts
                .iter()
                .map(|part| match part {
                    LlmContentPart::Text { text } => json!({ "type": "text", "text": text }),
                    LlmContentPart::ImageBase64 { media_type, data_base64 } => json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": data_base64,
                        }
                    }),
                })
                .collect();
            json!({ "role": role, "content": blocks })
        })
        .collect();

    let mut body = json!({
        "model": req.model_id,
        "max_tokens": req.max_tokens.unwrap_or(1024),
        "messages": messages,
    });
    if !sys.trim().is_empty() {
        body["system"] = json!(sys);
    }
    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    body
}

pub fn parse_anthropic_response(body: &Value, latency_ms: u64) -> Result<LlmRawResponse, LlmError> {
    let text = body["content"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|b| (b["type"] == "text").then(|| b["text"].as_str()).flatten())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let usage = LlmUsage {
        input_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0),
    };
    Ok(LlmRawResponse {
        text,
        structured_json: None,
        usage,
        latency_ms,
    })
}

#[async_trait]
impl LlmProvider for AnthropicApiProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::AnthropicApi
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
            .get(format!("{ANTHROPIC_API_BASE}/models"))
            .timeout(Duration::from_secs(10))
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => Ok(ProviderStatus {
                health: ProviderHealth::Connected,
                message: "Anthropic API key verified".into(),
                checked_at,
                details: None,
            }),
            Ok(res) if res.status().as_u16() == 401 || res.status().as_u16() == 403 => Ok(ProviderStatus {
                health: ProviderHealth::Expired,
                message: "Anthropic API key was rejected (HTTP 401/403)".into(),
                checked_at,
                details: None,
            }),
            Ok(res) => Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("api.anthropic.com returned HTTP {}", res.status()),
                checked_at,
                details: None,
            }),
            Err(e) => {
                let err = classify_reqwest_error("api.anthropic.com", &e);
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
        let payload = build_anthropic_payload(req);
        let t0 = Instant::now();

        let res = self
            .client
            .post(format!("{ANTHROPIC_API_BASE}/messages"))
            .header("x-api-key", &key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .json(&payload)
            .send()
            .await
            .map_err(|e| classify_reqwest_error("api.anthropic.com", &e))?;

        let status = res.status();
        let body: Value = res
            .json()
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("bad Anthropic JSON: {e}")))?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(LlmError::new(
                LlmErrorKind::AuthExpired,
                "Anthropic API key rejected (HTTP 401/403).",
            ));
        }
        if status.as_u16() == 429 {
            return Err(LlmError::new(LlmErrorKind::RateLimited, "Anthropic API rate limit reached (429)."));
        }
        if !status.is_success() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                format!("Anthropic API HTTP {status}: {body}"),
            ));
        }

        parse_anthropic_response(&body, t0.elapsed().as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{JsonSchemaSpec, LlmMessage, TaskType};

    #[test]
    fn builds_anthropic_messages_with_base64_image_and_schema() {
        let req = LlmRequest {
            task_type: TaskType::ReplyDrafting,
            model_id: "claude-sonnet-4-5".into(),
            system_prompt: Some("Be concise".into()),
            messages: vec![LlmMessage {
                role: LlmRole::User,
                parts: vec![
                    LlmContentPart::Text { text: "Describe".into() },
                    LlmContentPart::ImageBase64 {
                        media_type: "image/png".into(),
                        data_base64: "AAAA".into(),
                    },
                ],
            }],
            temperature: Some(0.5),
            reasoning_effort: None,
            max_tokens: Some(512),
            json_schema: Some(JsonSchemaSpec {
                name: "reply".into(),
                schema: json!({"type":"object"}),
            }),
            prompt_version: Some(1),
        };
        let p = build_anthropic_payload(&req);
        assert_eq!(p["model"], "claude-sonnet-4-5");
        assert!(p["system"].as_str().unwrap().contains("JSON"));
        assert_eq!(p["messages"][0]["content"][1]["source"]["data"], "AAAA");
    }
}
