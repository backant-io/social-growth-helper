//! Narrow completion seam between Phase 3 features and whatever LLM layer is plugged in.
//!
//! Scout scoring and draft generation only ever need "send a system and user prompt for task slot X and
//! get text back". Phase 2's `LlmRouter` implements [`CompletionProvider`] (one `impl` that maps
//! [`TaskSlot`] to its `task_model_routes` row and calls the routed provider) and is installed with
//! `Phase3State::set_completion`. Until then [`UnconfiguredProvider`] reports every slot as unset and the
//! features degrade: scout falls back to a local heuristic score and drafts are written by hand.

use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskSlot {
    PostScoring,
    ReplyDrafting,
    BrowserNavigation,
}

impl TaskSlot {
    /// The `task_model_routes.task_type` id of this slot.
    pub fn id(self) -> &'static str {
        match self {
            TaskSlot::PostScoring => "post_scoring",
            TaskSlot::ReplyDrafting => "reply_drafting",
            TaskSlot::BrowserNavigation => "browser_navigation",
        }
    }
}

/// The pieces a production caller exposes so a stored prompt override (LLM-19) can replace its built-in
/// wording. `vars` fill `{{name}}` placeholders in the override's user template; `required` vars that the
/// template forgot are appended so the model never loses its input; `guard` (system) and `extra` (user) are
/// non-negotiable parts such as the untrusted-text rules, the JSON output shape and per-call instructions.
#[derive(Debug, Clone, Default)]
pub struct PromptSlots {
    pub vars: Vec<(&'static str, String)>,
    pub required: Vec<&'static str>,
    pub guard: String,
    pub extra: String,
}

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub task: TaskSlot,
    pub system: String,
    pub user: String,
    /// The caller expects a JSON document; the provider may use a structured-output mode.
    pub json: bool,
    /// Base64 image payloads (PNG or JPEG) for vision-capable routes: screenshots and post thumbnails.
    pub images: Vec<String>,
    /// Optional model or provider the user asked for on this one call (side-by-side redraft, LLM-15).
    pub model_hint: Option<String>,
    /// When set, an active prompt override for the task replaces `system` and `user`.
    pub slots: Option<PromptSlots>,
    /// Lowest sampling temperature this call accepts. A routed temperature at or above it is kept; an unset or
    /// lower one is raised to it, so a call that wants varied output is never made deterministic by the route.
    pub min_temperature: Option<f32>,
}

impl CompletionRequest {
    pub fn new(task: TaskSlot, system: String, user: String, json: bool) -> Self {
        Self { task, system, user, json, images: vec![], model_hint: None, slots: None, min_temperature: None }
    }

    pub fn with_slots(mut self, slots: PromptSlots) -> Self {
        self.slots = Some(slots);
        self
    }

    pub fn with_min_temperature(mut self, floor: f32) -> Self {
        self.min_temperature = Some(floor);
        self
    }

    pub fn with_images(mut self, images: Vec<String>) -> Self {
        self.images = images;
        self
    }

    pub fn with_model_hint(mut self, hint: Option<String>) -> Self {
        self.model_hint = hint;
        self
    }
}

#[derive(Debug, Clone)]
pub struct Completion {
    pub text: String,
    /// Model id shown in the UI and stored with every score and draft.
    pub model: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CompletionError {
    #[error("no model is chosen for {0:?}; pick one in the model picker")]
    NotConfigured(TaskSlot),
    #[error("model call failed: {0}")]
    Failed(String),
}

#[async_trait::async_trait]
pub trait CompletionProvider: Send + Sync {
    /// Whether the slot has a model chosen (LLM-21 unset-route guard).
    fn is_configured(&self, task: TaskSlot) -> bool;
    async fn complete(&self, req: CompletionRequest) -> Result<Completion, CompletionError>;
}

/// Installed until a real router is plugged in: every slot is unset.
pub struct UnconfiguredProvider;

#[async_trait::async_trait]
impl CompletionProvider for UnconfiguredProvider {
    fn is_configured(&self, _task: TaskSlot) -> bool {
        false
    }
    async fn complete(&self, req: CompletionRequest) -> Result<Completion, CompletionError> {
        Err(CompletionError::NotConfigured(req.task))
    }
}

/// Test provider: returns queued responses in order and records every request.
pub struct ScriptedProvider {
    responses: Mutex<Vec<Result<String, String>>>,
    pub requests: Mutex<Vec<CompletionRequest>>,
}

impl ScriptedProvider {
    pub fn new(responses: Vec<&str>) -> Self {
        Self {
            responses: Mutex::new(responses.into_iter().rev().map(|s| Ok(s.to_string())).collect()),
            requests: Mutex::default(),
        }
    }

    pub fn call_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl CompletionProvider for ScriptedProvider {
    fn is_configured(&self, _task: TaskSlot) -> bool {
        true
    }
    async fn complete(&self, req: CompletionRequest) -> Result<Completion, CompletionError> {
        self.requests.lock().unwrap().push(req);
        match self.responses.lock().unwrap().pop() {
            Some(Ok(text)) => Ok(Completion { text, model: "scripted".into() }),
            Some(Err(e)) => Err(CompletionError::Failed(e)),
            None => Err(CompletionError::Failed("no scripted response left".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unconfigured_provider_reports_unset_routes() {
        let p = UnconfiguredProvider;
        assert!(!p.is_configured(TaskSlot::ReplyDrafting));
        let err = p
            .complete(CompletionRequest::new(TaskSlot::PostScoring, "s".into(), "u".into(), true))
            .await
            .unwrap_err();
        assert!(matches!(err, CompletionError::NotConfigured(TaskSlot::PostScoring)));
    }

    #[test]
    fn slot_ids_match_route_table_rows() {
        assert_eq!(TaskSlot::PostScoring.id(), "post_scoring");
        assert_eq!(TaskSlot::ReplyDrafting.id(), "reply_drafting");
        assert_eq!(TaskSlot::BrowserNavigation.id(), "browser_navigation");
    }
}
