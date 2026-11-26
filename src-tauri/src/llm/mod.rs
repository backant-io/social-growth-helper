//! LLM Router, multi-lane providers, task routing, schema validation, and usage accounting (LLM-01..21).

pub mod catalog;
pub mod phase3_bridge;
pub mod prompts;
pub mod providers;
pub mod repo;
pub mod router;
pub mod schema;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    BrowserNavigation,
    PostScoring,
    ReplyDrafting,
    OgcGeneration,
    FormatExtraction,
    LearningAnalysis,
    DashboardBuilder,
}

impl TaskType {
    pub const ALL: [TaskType; 7] = [
        TaskType::BrowserNavigation,
        TaskType::PostScoring,
        TaskType::ReplyDrafting,
        TaskType::OgcGeneration,
        TaskType::FormatExtraction,
        TaskType::LearningAnalysis,
        TaskType::DashboardBuilder,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            TaskType::BrowserNavigation => "browser_navigation",
            TaskType::PostScoring => "post_scoring",
            TaskType::ReplyDrafting => "reply_drafting",
            TaskType::OgcGeneration => "ogc_generation",
            TaskType::FormatExtraction => "format_extraction",
            TaskType::LearningAnalysis => "learning_analysis",
            TaskType::DashboardBuilder => "dashboard_builder",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "browser_navigation" => Some(Self::BrowserNavigation),
            "post_scoring" => Some(Self::PostScoring),
            "reply_drafting" => Some(Self::ReplyDrafting),
            "ogc_generation" => Some(Self::OgcGeneration),
            "format_extraction" => Some(Self::FormatExtraction),
            "learning_analysis" => Some(Self::LearningAnalysis),
            "dashboard_builder" => Some(Self::DashboardBuilder),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskType::BrowserNavigation => "Browser Navigation",
            TaskType::PostScoring => "Post Scoring",
            TaskType::ReplyDrafting => "Reply Drafting",
            TaskType::OgcGeneration => "OGC Generation",
            TaskType::FormatExtraction => "Format Extraction",
            TaskType::LearningAnalysis => "Learning Analysis",
            TaskType::DashboardBuilder => "Dashboard Builder",
        }
    }

    pub fn requires_vision(self) -> bool {
        matches!(self, TaskType::BrowserNavigation)
    }
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    ClaudeCli,
    AnthropicApi,
    #[serde(rename = "xai_oauth")]
    XaiOAuth,
    XaiApi,
    GrokWeb,
}

impl ProviderType {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderType::ClaudeCli => "claude_cli",
            ProviderType::AnthropicApi => "anthropic_api",
            ProviderType::XaiOAuth => "xai_oauth",
            ProviderType::XaiApi => "xai_api",
            ProviderType::GrokWeb => "grok_web",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "claude_cli" => Some(Self::ClaudeCli),
            "anthropic_api" => Some(Self::AnthropicApi),
            "xai_oauth" => Some(Self::XaiOAuth),
            "xai_api" => Some(Self::XaiApi),
            "grok_web" => Some(Self::GrokWeb),
            _ => None,
        }
    }

    pub fn billing_lane(self) -> BillingLane {
        match self {
            ProviderType::ClaudeCli | ProviderType::XaiOAuth | ProviderType::GrokWeb => BillingLane::Subscription,
            ProviderType::AnthropicApi | ProviderType::XaiApi => BillingLane::Api,
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            ProviderType::ClaudeCli => "Claude Code CLI",
            ProviderType::AnthropicApi => "Anthropic API Key",
            ProviderType::XaiOAuth => "SuperGrok / xAI OAuth",
            ProviderType::XaiApi => "xAI API Key",
            ProviderType::GrokWeb => "Grok X Tab (Fallback)",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingLane {
    Subscription,
    Api,
}

impl BillingLane {
    pub fn as_str(self) -> &'static str {
        match self {
            BillingLane::Subscription => "subscription",
            BillingLane::Api => "api",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProviderCapabilities {
    pub vision: bool,
    pub json_schema: bool,
    pub streaming: bool,
    pub thinking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmContentPart {
    Text { text: String },
    ImageBase64 { media_type: String, data_base64: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmMessage {
    pub role: LlmRole,
    pub parts: Vec<LlmContentPart>,
}

impl LlmMessage {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self { role: LlmRole::User, parts: vec![LlmContentPart::Text { text: text.into() }] }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self { role: LlmRole::Assistant, parts: vec![LlmContentPart::Text { text: text.into() }] }
    }

    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| match p {
                LlmContentPart::Text { text } => Some(text.as_str()),
                LlmContentPart::ImageBase64 { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn has_images(&self) -> bool {
        self.parts.iter().any(|p| matches!(p, LlmContentPart::ImageBase64 { .. }))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonSchemaSpec {
    pub name: String,
    pub schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub task_type: TaskType,
    pub model_id: String,
    pub system_prompt: Option<String>,
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<f32>,
    pub reasoning_effort: Option<String>,
    pub max_tokens: Option<u32>,
    pub json_schema: Option<JsonSchemaSpec>,
    pub prompt_version: Option<u32>,
}

impl LlmRequest {
    pub fn has_images(&self) -> bool {
        self.messages.iter().any(LlmMessage::has_images)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LlmUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRawResponse {
    pub text: String,
    pub structured_json: Option<serde_json::Value>,
    pub usage: LlmUsage,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub text: String,
    pub structured_json: Option<serde_json::Value>,
    pub model_id: String,
    pub provider_type: ProviderType,
    pub provider_account_id: String,
    pub provider_label: String,
    pub billing_lane: BillingLane,
    pub usage: LlmUsage,
    pub latency_ms: u64,
    pub used_fallback: bool,
    pub fallback_reason: Option<String>,
    pub prompt_version: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderHealth {
    Connected,
    NeedsLogin,
    Expired,
    Offline,
    Error,
    Disconnected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub health: ProviderHealth,
    pub message: String,
    pub checked_at: String,
    pub details: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmErrorKind {
    Offline,
    ProviderDown,
    AuthExpired,
    NotLoggedIn,
    RateLimited,
    RateGuardBlocked,
    UnsetRoute,
    UnsupportedCapability,
    SchemaValidation,
    InvalidConfig,
    BrowserNotRunning,
    Other,
}

#[derive(Debug, Clone, thiserror::Error, Serialize, Deserialize)]
#[error("{message}")]
pub struct LlmError {
    pub kind: LlmErrorKind,
    pub message: String,
    pub task_type: Option<TaskType>,
}

impl LlmError {
    pub fn new(kind: LlmErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), task_type: None }
    }

    pub fn for_task(kind: LlmErrorKind, task_type: TaskType, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), task_type: Some(task_type) }
    }

    pub fn is_retryable_with_fallback(&self) -> bool {
        matches!(
            self.kind,
            LlmErrorKind::Offline
                | LlmErrorKind::ProviderDown
                | LlmErrorKind::RateLimited
                | LlmErrorKind::AuthExpired
                | LlmErrorKind::NotLoggedIn
                | LlmErrorKind::BrowserNotRunning
        )
    }

    pub fn error_code(&self) -> &'static str {
        match self.kind {
            LlmErrorKind::Offline => "offline",
            LlmErrorKind::ProviderDown => "provider_down",
            LlmErrorKind::AuthExpired => "auth_expired",
            LlmErrorKind::NotLoggedIn => "not_logged_in",
            LlmErrorKind::RateLimited => "rate_limited",
            LlmErrorKind::RateGuardBlocked => "rate_guard_blocked",
            LlmErrorKind::UnsetRoute => "unset_route",
            LlmErrorKind::UnsupportedCapability => "unsupported_capability",
            LlmErrorKind::SchemaValidation => "schema_validation",
            LlmErrorKind::InvalidConfig => "invalid_config",
            LlmErrorKind::BrowserNotRunning => "browser_not_running",
            LlmErrorKind::Other => "other",
        }
    }
}

pub fn classify_reqwest_error(provider: &str, err: &reqwest::Error) -> LlmError {
    if err.is_timeout() {
        return LlmError::new(
            LlmErrorKind::ProviderDown,
            format!("{provider} timed out; check your connection or provider status"),
        );
    }
    if err.is_connect() {
        return LlmError::new(
            LlmErrorKind::Offline,
            format!("cannot reach {provider}; you appear to be offline or the host is unreachable"),
        );
    }
    LlmError::new(LlmErrorKind::ProviderDown, format!("{provider} request failed: {err}"))
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn account_id(&self) -> &str;
    fn provider_type(&self) -> ProviderType;
    fn capabilities(&self) -> ProviderCapabilities;
    async fn health_check(&self) -> Result<ProviderStatus, LlmError>;
    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError>;
}
