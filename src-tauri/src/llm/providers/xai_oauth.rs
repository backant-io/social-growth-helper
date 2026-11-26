//! SuperGrok / X Premium RFC 8628 Device-Code OAuth + OpenAI-compatible inference (LLM-04, SPK-02).
//!
//! Tokens live exclusively in OS Keychain (`SecretsService`). SQLite stores only `keychain_ref`.

use super::super::{
    classify_reqwest_error, LlmContentPart, LlmError, LlmErrorKind, LlmProvider, LlmRawResponse, LlmRequest,
    LlmRole, LlmUsage, ProviderCapabilities, ProviderHealth, ProviderStatus, ProviderType,
};
use crate::secrets::SecretsService;
use crate::util::{now_epoch_secs, now_rfc3339};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const OIDC_DISCOVERY_URL: &str = "https://auth.x.ai/.well-known/openid-configuration";
pub const DEFAULT_DEVICE_ENDPOINT: &str = "https://auth.x.ai/oauth2/device/code";
pub const DEFAULT_TOKEN_ENDPOINT: &str = "https://auth.x.ai/oauth2/token";
pub const PUBLIC_CLIENT_ID: &str = "b1a00492-073a-47ea-816f-4c329264a828";
pub const OAUTH_SCOPE: &str = "openid profile email offline_access grok-cli:access api:access";
pub const DEFAULT_INFERENCE_BASE: &str = "https://api.x.ai/v1";
pub const CLI_PROXY_INFERENCE_BASE: &str = "https://cli-chat-proxy.grok.com/v1";
const REFRESH_SKEW_SECS: i64 = 300; // 5-minute skew per architecture plan

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XaiDeviceChallenge {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
    pub token_endpoint: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredOAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at_epoch: i64,
    pub token_endpoint: String,
    pub endpoint_base: String,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DevicePollOutcome {
    Pending { slow_down: bool },
    Authorized,
    Expired,
    Denied,
}

pub async fn start_device_flow(client: &Client) -> Result<XaiDeviceChallenge, LlmError> {
    let (device_endpoint, token_endpoint) = discover_endpoints(client).await;

    let res = client
        .post(&device_endpoint)
        .timeout(Duration::from_secs(10))
        .form(&[
            ("client_id", PUBLIC_CLIENT_ID),
            ("scope", OAUTH_SCOPE),
            ("referrer", "social-growth-helper"),
        ])
        .send()
        .await
        .map_err(|e| classify_reqwest_error("auth.x.ai", &e))?;

    let status = res.status();
    let body: Value = res
        .json()
        .await
        .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("bad device-code JSON: {e}")))?;
    if !status.is_success() {
        return Err(LlmError::new(
            LlmErrorKind::ProviderDown,
            format!("xAI device-code request failed ({status}): {body}"),
        ));
    }

    let device_code = body["device_code"]
        .as_str()
        .ok_or_else(|| LlmError::new(LlmErrorKind::ProviderDown, "missing device_code"))?
        .to_string();
    let user_code = body["user_code"]
        .as_str()
        .ok_or_else(|| LlmError::new(LlmErrorKind::ProviderDown, "missing user_code"))?
        .to_string();
    let verification_uri = body["verification_uri"]
        .as_str()
        .unwrap_or("https://accounts.x.ai/oauth2/device")
        .to_string();

    Ok(XaiDeviceChallenge {
        device_code,
        user_code,
        verification_uri,
        verification_uri_complete: body["verification_uri_complete"].as_str().map(ToString::to_string),
        expires_in: body["expires_in"].as_u64().unwrap_or(600),
        interval: body["interval"].as_u64().unwrap_or(5),
        token_endpoint,
    })
}

async fn discover_endpoints(client: &Client) -> (String, String) {
    if let Ok(res) = client.get(OIDC_DISCOVERY_URL).timeout(Duration::from_secs(6)).send().await {
        if res.status().is_success() {
            if let Ok(v) = res.json::<Value>().await {
                let d = v["device_authorization_endpoint"]
                    .as_str()
                    .unwrap_or(DEFAULT_DEVICE_ENDPOINT)
                    .to_string();
                let t = v["token_endpoint"]
                    .as_str()
                    .unwrap_or(DEFAULT_TOKEN_ENDPOINT)
                    .to_string();
                return (d, t);
            }
        }
    }
    (DEFAULT_DEVICE_ENDPOINT.into(), DEFAULT_TOKEN_ENDPOINT.into())
}

pub async fn poll_device_token(
    client: &Client,
    token_endpoint: &str,
    device_code: &str,
    endpoint_base: &str,
) -> Result<(DevicePollOutcome, Option<StoredOAuthTokens>), LlmError> {
    let res = client
        .post(token_endpoint)
        .timeout(Duration::from_secs(10))
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", PUBLIC_CLIENT_ID),
            ("device_code", device_code),
        ])
        .send()
        .await
        .map_err(|e| classify_reqwest_error("auth.x.ai", &e))?;

    let status = res.status();
    let body: Value = res
        .json()
        .await
        .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("bad token poll JSON: {e}")))?;

    if status.is_success() && body["access_token"].is_string() {
        let tokens = parse_token_response(&body, token_endpoint, endpoint_base, None)?;
        return Ok((DevicePollOutcome::Authorized, Some(tokens)));
    }

    match body["error"].as_str() {
        Some("authorization_pending") => Ok((DevicePollOutcome::Pending { slow_down: false }, None)),
        Some("slow_down") => Ok((DevicePollOutcome::Pending { slow_down: true }, None)),
        Some("expired_token") => Ok((DevicePollOutcome::Expired, None)),
        Some("access_denied") => Ok((DevicePollOutcome::Denied, None)),
        other => Err(LlmError::new(
            LlmErrorKind::ProviderDown,
            format!("xAI OAuth token error ({status}): {}", other.unwrap_or("unknown")),
        )),
    }
}

pub fn parse_token_response(
    body: &Value,
    token_endpoint: &str,
    endpoint_base: &str,
    previous_refresh: Option<&str>,
) -> Result<StoredOAuthTokens, LlmError> {
    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| LlmError::new(LlmErrorKind::AuthExpired, "token response missing access_token"))?
        .to_string();
    let refresh_token = body["refresh_token"]
        .as_str()
        .map(ToString::to_string)
        .or_else(|| previous_refresh.map(ToString::to_string));
    let expires_in = body["expires_in"].as_i64().unwrap_or(3600);
    Ok(StoredOAuthTokens {
        access_token,
        refresh_token,
        expires_at_epoch: now_epoch_secs() + expires_in,
        token_endpoint: token_endpoint.to_string(),
        endpoint_base: endpoint_base.to_string(),
        scope: body["scope"].as_str().map(ToString::to_string),
    })
}

pub async fn refresh_tokens(client: &Client, current: &StoredOAuthTokens) -> Result<StoredOAuthTokens, LlmError> {
    let refresh = current.refresh_token.as_deref().ok_or_else(|| {
        LlmError::new(
            LlmErrorKind::AuthExpired,
            "xAI OAuth token expired and no refresh_token is available. Reconnect SuperGrok OAuth.",
        )
    })?;

    let endpoint = if current.token_endpoint.is_empty() {
        DEFAULT_TOKEN_ENDPOINT
    } else {
        current.token_endpoint.as_str()
    };

    let res = client
        .post(endpoint)
        .timeout(Duration::from_secs(10))
        .form(&[
            ("grant_type", "refresh_token"),
            ("client_id", PUBLIC_CLIENT_ID),
            ("refresh_token", refresh),
        ])
        .send()
        .await
        .map_err(|e| classify_reqwest_error("auth.x.ai", &e))?;

    let status = res.status();
    let body: Value = res
        .json()
        .await
        .map_err(|e| LlmError::new(LlmErrorKind::AuthExpired, format!("refresh JSON parse error: {e}")))?;

    if !status.is_success() {
        return Err(LlmError::new(
            LlmErrorKind::AuthExpired,
            format!("xAI refresh failed ({status}); reconnect SuperGrok OAuth."),
        ));
    }

    parse_token_response(&body, endpoint, &current.endpoint_base, Some(refresh))
}

pub fn token_needs_refresh(tokens: &StoredOAuthTokens, now_epoch: i64) -> bool {
    now_epoch + REFRESH_SKEW_SECS >= tokens.expires_at_epoch
}

pub struct XaiOAuthProvider {
    account_id: String,
    keychain_ref: String,
    endpoint_override: Option<String>,
    secrets: Arc<SecretsService>,
    client: Client,
}

impl XaiOAuthProvider {
    pub fn new(
        account_id: impl Into<String>,
        keychain_ref: impl Into<String>,
        endpoint_override: Option<String>,
        secrets: Arc<SecretsService>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            account_id: account_id.into(),
            keychain_ref: keychain_ref.into(),
            endpoint_override,
            secrets,
            client,
        }
    }

    fn load_tokens(&self) -> Result<StoredOAuthTokens, LlmError> {
        let raw = self
            .secrets
            .load(&self.keychain_ref)
            .map_err(|e| LlmError::new(LlmErrorKind::InvalidConfig, e.to_string()))?
            .ok_or_else(|| {
                LlmError::new(
                    LlmErrorKind::AuthExpired,
                    "No xAI OAuth token found in Keychain. Connect SuperGrok OAuth in Settings.",
                )
            })?;
        serde_json::from_str(&raw)
            .map_err(|e| LlmError::new(LlmErrorKind::InvalidConfig, format!("corrupt xAI OAuth token data: {e}")))
    }

    async fn valid_tokens(&self) -> Result<StoredOAuthTokens, LlmError> {
        let mut tokens = self.load_tokens()?;
        if token_needs_refresh(&tokens, now_epoch_secs()) {
            tokens = refresh_tokens(&self.client, &tokens).await?;
            let serialized = serde_json::to_string(&tokens)
                .map_err(|e| LlmError::new(LlmErrorKind::Other, e.to_string()))?;
            self.secrets
                .store(&self.keychain_ref, &serialized)
                .map_err(|e| LlmError::new(LlmErrorKind::InvalidConfig, e.to_string()))?;
        }
        Ok(tokens)
    }

    fn effective_base<'a>(&'a self, tokens: &'a StoredOAuthTokens) -> &'a str {
        self.endpoint_override
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                if tokens.endpoint_base.trim().is_empty() {
                    DEFAULT_INFERENCE_BASE
                } else {
                    tokens.endpoint_base.as_str()
                }
            })
    }
}

pub fn build_openai_chat_payload(req: &LlmRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    if let Some(sys) = req.system_prompt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        messages.push(json!({ "role": "system", "content": sys }));
    }
    for msg in &req.messages {
        let role = match msg.role {
            LlmRole::System => "system",
            LlmRole::User => "user",
            LlmRole::Assistant => "assistant",
        };
        if !msg.has_images() {
            messages.push(json!({ "role": role, "content": msg.text_content() }));
        } else {
            let parts: Vec<Value> = msg
                .parts
                .iter()
                .map(|p| match p {
                    LlmContentPart::Text { text } => json!({ "type": "text", "text": text }),
                    LlmContentPart::ImageBase64 { media_type, data_base64 } => json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{media_type};base64,{data_base64}") }
                    }),
                })
                .collect();
            messages.push(json!({ "role": role, "content": parts }));
        }
    }

    let mut body = json!({
        "model": req.model_id,
        "messages": messages,
    });

    if let Some(temp) = req.temperature {
        body["temperature"] = json!(temp);
    }
    if let Some(max_t) = req.max_tokens {
        body["max_tokens"] = json!(max_t);
    }
    if let Some(effort) = req.reasoning_effort.as_deref().filter(|e| !e.trim().is_empty()) {
        body["reasoning_effort"] = json!(effort);
    }
    if let Some(spec) = &req.json_schema {
        body["response_format"] = json!({
            "type": "json_schema",
            "json_schema": {
                "name": spec.name,
                "strict": true,
                "schema": spec.schema,
            }
        });
    }
    body
}

pub fn parse_openai_chat_response(body: &Value, latency_ms: u64) -> Result<LlmRawResponse, LlmError> {
    let text = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    if text.is_empty() && body.get("error").is_some() {
        return Err(LlmError::new(
            LlmErrorKind::ProviderDown,
            format!("xAI error: {}", body["error"]),
        ));
    }
    let usage = LlmUsage {
        input_tokens: body["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
        output_tokens: body["usage"]["completion_tokens"].as_u64().unwrap_or(0),
    };
    Ok(LlmRawResponse {
        text,
        structured_json: None,
        usage,
        latency_ms,
    })
}

#[async_trait]
impl LlmProvider for XaiOAuthProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::XaiOAuth
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
        let tokens = match self.valid_tokens().await {
            Ok(t) => t,
            Err(e) => {
                let health = match e.kind {
                    LlmErrorKind::Offline => ProviderHealth::Offline,
                    LlmErrorKind::AuthExpired => ProviderHealth::Expired,
                    _ => ProviderHealth::Error,
                };
                return Ok(ProviderStatus {
                    health,
                    message: e.message,
                    checked_at,
                    details: None,
                });
            }
        };

        let base = self.effective_base(&tokens).trim_end_matches('/');
        let url = format!("{base}/models");
        match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .bearer_auth(&tokens.access_token)
            .send()
            .await
        {
            Ok(res) if res.status().is_success() => Ok(ProviderStatus {
                health: ProviderHealth::Connected,
                message: format!("SuperGrok OAuth connected ({base})"),
                checked_at,
                details: None,
            }),
            Ok(res) if res.status().as_u16() == 401 || res.status().as_u16() == 403 => Ok(ProviderStatus {
                health: ProviderHealth::Expired,
                message: format!("xAI OAuth token rejected (HTTP {}). Reconnect SuperGrok OAuth.", res.status()),
                checked_at,
                details: Some(base.to_string()),
            }),
            Ok(res) => Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("xAI endpoint {base} returned HTTP {}", res.status()),
                checked_at,
                details: Some(base.to_string()),
            }),
            Err(e) => {
                let err = classify_reqwest_error(base, &e);
                Ok(ProviderStatus {
                    health: if err.kind == LlmErrorKind::Offline {
                        ProviderHealth::Offline
                    } else {
                        ProviderHealth::Error
                    },
                    message: err.message,
                    checked_at,
                    details: Some(base.to_string()),
                })
            }
        }
    }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError> {
        let tokens = self.valid_tokens().await?;
        let base = self.effective_base(&tokens).trim_end_matches('/');
        let url = format!("{base}/chat/completions");
        let payload = build_openai_chat_payload(req);
        let conv_id = format!("sgh-{}", crate::util::new_id());
        let t0 = Instant::now();

        let res = self
            .client
            .post(&url)
            .bearer_auth(&tokens.access_token)
            .header("x-grok-conv-id", conv_id)
            .json(&payload)
            .send()
            .await
            .map_err(|e| classify_reqwest_error(base, &e))?;

        let status = res.status();
        let body: Value = res
            .json()
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("bad xAI response JSON: {e}")))?;

        if status.as_u16() == 401 || status.as_u16() == 403 {
            return Err(LlmError::new(
                LlmErrorKind::AuthExpired,
                format!("xAI OAuth unauthorized (HTTP {status}). Reconnect or refresh token."),
            ));
        }
        if status.as_u16() == 429 {
            return Err(LlmError::new(LlmErrorKind::RateLimited, "xAI rate limit reached (429)."));
        }
        if !status.is_success() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                format!("xAI OAuth HTTP {status}: {body}"),
            ));
        }

        parse_openai_chat_response(&body, t0.elapsed().as_millis() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{JsonSchemaSpec, LlmMessage, TaskType};

    #[test]
    fn refresh_skew_triggers_five_minutes_before_expiry() {
        let tok = StoredOAuthTokens {
            access_token: "a".into(),
            refresh_token: Some("r".into()),
            expires_at_epoch: 10_000,
            token_endpoint: DEFAULT_TOKEN_ENDPOINT.into(),
            endpoint_base: DEFAULT_INFERENCE_BASE.into(),
            scope: None,
        };
        assert!(!token_needs_refresh(&tok, 9_699));
        assert!(token_needs_refresh(&tok, 9_700));
        assert!(token_needs_refresh(&tok, 10_500));
    }

    #[test]
    fn openai_payload_encodes_json_schema_and_vision() {
        let req = LlmRequest {
            task_type: TaskType::BrowserNavigation,
            model_id: "grok-4.6".into(),
            system_prompt: Some("Locate button".into()),
            messages: vec![LlmMessage {
                role: LlmRole::User,
                parts: vec![
                    LlmContentPart::Text { text: "Where is Like?".into() },
                    LlmContentPart::ImageBase64 {
                        media_type: "image/png".into(),
                        data_base64: "QUJD".into(),
                    },
                ],
            }],
            temperature: Some(0.2),
            reasoning_effort: Some("low".into()),
            max_tokens: Some(120),
            json_schema: Some(JsonSchemaSpec {
                name: "coords".into(),
                schema: json!({"type":"object"}),
            }),
            prompt_version: Some(1),
        };
        let p = build_openai_chat_payload(&req);
        assert_eq!(p["model"], "grok-4.6");
        assert_eq!(p["response_format"]["json_schema"]["strict"], true);
        assert_eq!(p["messages"][1]["content"][1]["image_url"]["url"], "data:image/png;base64,QUJD");
    }
}
