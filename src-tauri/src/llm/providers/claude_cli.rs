//! Unmodified `claude` CLI provider (LLM-02, LLM-03, SEC-12).
//!
//! Policy & Spike S3 constraints enforced here:
//! - Always passes `--safe-mode`, NEVER `--bare` (which disables OAuth subscription login).
//! - Always passes `--tools "" --disallowedTools "mcp__*"` (`SEC-07` prompt-injection isolation).
//! - Never reads `~/.claude` credentials directly; authentication stays inside `claude login`.
//! - Marked personal single-user only (`SEC-12`).

use crate::llm::{
    LlmContentPart, LlmError, LlmErrorKind, LlmProvider, LlmRawResponse, LlmRequest, LlmRole, LlmUsage,
    ProviderCapabilities, ProviderHealth, ProviderStatus, ProviderType,
};
use crate::util::now_rfc3339;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

const CLI_TIMEOUT: Duration = Duration::from_secs(90);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(10);

pub const PERSONAL_USE_NOTICE: &str = "Personal single-user use only. Uses your local unmodified `claude` CLI login (`--safe-mode`). Any external distribution requires each user's own Anthropic API key or Linked Billing.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeCliInstallState {
    Missing,
    InstalledLoggedOut,
    InstalledReady,
    InstalledUnknownAuth,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCliDetection {
    pub state: ClaudeCliInstallState,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub supports_safe_mode: bool,
    pub auth_detail: String,
    pub personal_use_notice: String,
}

pub fn discover_claude_binary(override_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(if cfg!(windows) { "claude.exe" } else { "claude" });
            if cand.is_file() {
                return Some(cand);
            }
        }
    }

    let mut fallbacks = vec![
        PathBuf::from("/opt/homebrew/bin/claude"),
        PathBuf::from("/usr/local/bin/claude"),
        PathBuf::from("/usr/bin/claude"),
    ];
    if let Some(home) = dirs::home_dir() {
        fallbacks.push(home.join(".local/bin/claude"));
        fallbacks.push(home.join(".npm-global/bin/claude"));
    }
    fallbacks.into_iter().find(|p| p.is_file())
}

pub async fn detect_claude_cli(override_path: Option<&Path>) -> ClaudeCliDetection {
    let Some(bin) = discover_claude_binary(override_path) else {
        return ClaudeCliDetection {
            state: ClaudeCliInstallState::Missing,
            binary_path: None,
            version: None,
            supports_safe_mode: false,
            auth_detail: "Claude Code CLI was not found on PATH. Install it and run `claude login`.".into(),
            personal_use_notice: PERSONAL_USE_NOTICE.into(),
        };
    };

    let version_out = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(&bin).arg("--version").output(),
    )
    .await;

    let version = match version_out {
        Ok(Ok(o)) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).trim().to_string()),
        _ => None,
    };

    let help_out = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(&bin).arg("--help").output(),
    )
    .await;
    let supports_safe_mode = match help_out {
        Ok(Ok(o)) => String::from_utf8_lossy(&o.stdout).contains("--safe-mode"),
        _ => false,
    };

    // Query `claude auth status` without ever reading ~/.claude files directly.
    let auth_out = tokio::time::timeout(
        Duration::from_secs(5),
        Command::new(&bin).args(["auth", "status"]).output(),
    )
    .await;

    let (state, auth_detail) = match auth_out {
        Ok(Ok(o)) => {
            let combined = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            let lower = combined.to_lowercase();
            if lower.contains("\"loggedin\":true")
                || lower.contains("\"logged_in\":true")
                || (o.status.success() && lower.contains("logged in") && !lower.contains("not logged in"))
            {
                (ClaudeCliInstallState::InstalledReady, "Signed in via `claude login`.".into())
            } else if lower.contains("not logged in")
                || lower.contains("logged out")
                || lower.contains("\"loggedin\":false")
                || lower.contains("please run `claude login`")
            {
                (
                    ClaudeCliInstallState::InstalledLoggedOut,
                    "Claude CLI is installed but not signed in. Run `claude login` in your terminal.".into(),
                )
            } else if o.status.success() {
                (ClaudeCliInstallState::InstalledReady, "Claude CLI is installed and auth status succeeded.".into())
            } else {
                (
                    ClaudeCliInstallState::InstalledUnknownAuth,
                    "Claude CLI installed. Click Test to verify your `claude login` session.".into(),
                )
            }
        }
        _ => (
            ClaudeCliInstallState::InstalledUnknownAuth,
            "Claude CLI installed. Click Test to verify your `claude login` session.".into(),
        ),
    };

    ClaudeCliDetection {
        state,
        binary_path: Some(bin.display().to_string()),
        version,
        supports_safe_mode,
        auth_detail,
        personal_use_notice: PERSONAL_USE_NOTICE.into(),
    }
}

/// Builds the exact `claude` CLI arguments (pure function so tests verify `--safe-mode`, zero tools, and absence of `--bare`).
pub fn build_cli_args(req: &LlmRequest, vision_stream_mode: bool) -> Vec<String> {
    let mut args = vec!["-p".to_string()];

    if !vision_stream_mode {
        args.push(render_prompt_text(req));
    }

    // Critical Spike S3 rule: --safe-mode preserves OAuth subscription login; NEVER pass --bare.
    args.extend([
        "--safe-mode".to_string(),
        "--no-session-persistence".to_string(),
        "--tools".to_string(),
        "".to_string(),
        "--disallowedTools".to_string(),
        "mcp__*".to_string(),
        "--max-turns".to_string(),
        "1".to_string(),
        "--model".to_string(),
        req.model_id.clone(),
    ]);

    if let Some(effort) = req.reasoning_effort.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        args.push("--effort".to_string());
        args.push(effort.to_string());
    }

    let mut sys = req.system_prompt.clone().unwrap_or_default();
    if vision_stream_mode {
        if let Some(spec) = &req.json_schema {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str(&format!(
                "Return ONLY valid JSON matching this JSON Schema (name={}):\n{}",
                spec.name, spec.schema
            ));
        }
    }
    if !sys.trim().is_empty() {
        args.push("--system-prompt".to_string());
        args.push(sys);
    }

    if vision_stream_mode {
        args.extend([
            "--input-format".to_string(),
            "stream-json".to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--verbose".to_string(),
        ]);
    } else {
        args.extend(["--output-format".to_string(), "json".to_string()]);
        if let Some(spec) = &req.json_schema {
            args.push("--json-schema".to_string());
            args.push(spec.schema.to_string());
        }
    }

    args
}

fn render_prompt_text(req: &LlmRequest) -> String {
    if req.messages.len() == 1 && req.messages[0].role == LlmRole::User {
        return req.messages[0].text_content();
    }
    req.messages
        .iter()
        .map(|m| {
            let role = match m.role {
                LlmRole::System => "System",
                LlmRole::User => "User",
                LlmRole::Assistant => "Assistant",
            };
            format!("{role}:\n{}", m.text_content())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Builds the `--input-format stream-json` NDJSON payload with inline base64 image blocks (zero temp files).
pub fn build_vision_stream_payload(req: &LlmRequest) -> String {
    let mut blocks: Vec<Value> = Vec::new();
    for msg in &req.messages {
        let role_prefix = match msg.role {
            LlmRole::Assistant => Some("[Assistant] "),
            LlmRole::System => Some("[System] "),
            LlmRole::User => None,
        };
        for part in &msg.parts {
            match part {
                LlmContentPart::Text { text } => {
                    let full = match role_prefix {
                        Some(p) => format!("{p}{text}"),
                        None => text.clone(),
                    };
                    blocks.push(json!({ "type": "text", "text": full }));
                }
                LlmContentPart::ImageBase64 { media_type, data_base64 } => {
                    blocks.push(json!({
                        "type": "image",
                        "data": data_base64,
                        "mimeType": media_type
                    }));
                }
            }
        }
    }
    let line = json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": blocks
        }
    });
    format!("{line}\n")
}

pub fn parse_json_output(stdout: &str) -> Result<(String, Option<Value>, LlmUsage), String> {
    let v: Value = serde_json::from_str(stdout.trim()).map_err(|e| format!("invalid CLI JSON output: {e}"))?;
    if v["is_error"].as_bool() == Some(true) {
        let msg = v["result"].as_str().unwrap_or("claude CLI reported an error");
        return Err(msg.to_string());
    }
    let structured = v.get("structured_output").filter(|s| !s.is_null()).cloned();
    let text = match v["result"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => s.to_string(),
        None => structured.as_ref().map(|st| st.to_string()).unwrap_or_default(),
    };
    let usage = LlmUsage {
        input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
    };
    Ok((text, structured, usage))
}

pub fn parse_stream_json_output(stdout: &str) -> Result<(String, Option<Value>, LlmUsage), String> {
    let mut final_ev: Option<Value> = None;
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        if let Ok(v) = serde_json::from_str::<Value>(line) {
            if v["type"] == "result" {
                final_ev = Some(v);
            }
        }
    }
    let ev = final_ev.ok_or_else(|| "claude stream-json output contained no result event".to_string())?;
    if ev["is_error"].as_bool() == Some(true) {
        let msg = ev["result"].as_str().unwrap_or("claude CLI reported a vision error");
        return Err(msg.to_string());
    }
    let structured = ev.get("structured_output").filter(|s| !s.is_null()).cloned();
    let text = ev["result"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| structured.as_ref().map(|s| s.to_string()))
        .unwrap_or_default();
    let usage = LlmUsage {
        input_tokens: ev["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: ev["usage"]["output_tokens"].as_u64().unwrap_or(0),
    };
    Ok((text, structured, usage))
}

pub fn classify_cli_failure(stderr: &str, stdout: &str) -> LlmError {
    let combined = format!("{stderr}\n{stdout}");
    let lower = combined.to_lowercase();
    if lower.contains("not logged in")
        || lower.contains("please run `claude login`")
        || lower.contains("login required")
        || lower.contains("unauthorized")
    {
        return LlmError::new(
            LlmErrorKind::NotLoggedIn,
            "Claude CLI is not signed in. Run `claude login` in your terminal and retry.",
        );
    }
    if lower.contains("rate limit") || lower.contains("429") || lower.contains("quota") || lower.contains("overloaded") {
        return LlmError::new(
            LlmErrorKind::RateLimited,
            format!("Claude subscription rate/quota limit reached: {}", combined.trim().chars().take(220).collect::<String>()),
        );
    }
    if lower.contains("enotfound")
        || lower.contains("econnrefused")
        || lower.contains("network")
        || lower.contains("offline")
        || lower.contains("could not resolve host")
        || lower.contains("fetch failed")
    {
        return LlmError::new(
            LlmErrorKind::Offline,
            "Claude CLI could not reach Anthropic; check your internet connection.",
        );
    }
    LlmError::new(
        LlmErrorKind::ProviderDown,
        format!("Claude CLI failed: {}", combined.trim().chars().take(280).collect::<String>()),
    )
}

pub struct ClaudeCliProvider {
    account_id: String,
    binary_override: Option<PathBuf>,
}

impl ClaudeCliProvider {
    pub fn new(account_id: impl Into<String>, binary_override: Option<PathBuf>) -> Self {
        Self {
            account_id: account_id.into(),
            binary_override,
        }
    }

    fn resolve_bin(&self) -> Result<PathBuf, LlmError> {
        discover_claude_binary(self.binary_override.as_deref()).ok_or_else(|| {
            LlmError::new(
                LlmErrorKind::NotLoggedIn,
                "Claude CLI binary not found. Install Claude Code and run `claude login`.",
            )
        })
    }
}

#[async_trait]
impl LlmProvider for ClaudeCliProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::ClaudeCli
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
        let detection = detect_claude_cli(self.binary_override.as_deref()).await;
        let checked_at = now_rfc3339();
        match detection.state {
            ClaudeCliInstallState::Missing => Ok(ProviderStatus {
                health: ProviderHealth::Disconnected,
                message: detection.auth_detail,
                checked_at,
                details: None,
            }),
            ClaudeCliInstallState::InstalledLoggedOut => Ok(ProviderStatus {
                health: ProviderHealth::NeedsLogin,
                message: detection.auth_detail,
                checked_at,
                details: detection.version,
            }),
            ClaudeCliInstallState::InstalledReady | ClaudeCliInstallState::InstalledUnknownAuth => {
                // Verify with a fast 1-token safe-mode ping within 10s so failing providers surface cleanly (LLM-10).
                let bin = self.resolve_bin()?;
                let fut = Command::new(&bin)
                    .args([
                        "-p",
                        "Reply with OK",
                        "--safe-mode",
                        "--no-session-persistence",
                        "--tools",
                        "",
                        "--disallowedTools",
                        "mcp__*",
                        "--max-turns",
                        "1",
                        "--model",
                        "haiku",
                        "--output-format",
                        "json",
                    ])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();

                match tokio::time::timeout(HEALTH_TIMEOUT, fut).await {
                    Ok(Ok(out)) if out.status.success() => Ok(ProviderStatus {
                        health: ProviderHealth::Connected,
                        message: format!(
                            "Claude CLI ready ({})",
                            detection.version.as_deref().unwrap_or("installed")
                        ),
                        checked_at,
                        details: detection.binary_path,
                    }),
                    Ok(Ok(out)) => {
                        let err = classify_cli_failure(
                            &String::from_utf8_lossy(&out.stderr),
                            &String::from_utf8_lossy(&out.stdout),
                        );
                        let health = match err.kind {
                            LlmErrorKind::NotLoggedIn => ProviderHealth::NeedsLogin,
                            LlmErrorKind::Offline => ProviderHealth::Offline,
                            _ => ProviderHealth::Error,
                        };
                        Ok(ProviderStatus {
                            health,
                            message: err.message,
                            checked_at,
                            details: detection.version,
                        })
                    }
                    Ok(Err(e)) => Ok(ProviderStatus {
                        health: ProviderHealth::Error,
                        message: format!("failed to launch claude CLI: {e}"),
                        checked_at,
                        details: None,
                    }),
                    Err(_) => Ok(ProviderStatus {
                        health: ProviderHealth::Offline,
                        message: "Claude CLI health check timed out after 10s".into(),
                        checked_at,
                        details: detection.version,
                    }),
                }
            }
        }
    }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError> {
        let bin = self.resolve_bin()?;
        let vision = req.has_images();
        let args = build_cli_args(req, vision);
        let t0 = Instant::now();

        let mut cmd = Command::new(&bin);
        cmd.args(&args)
            .stdin(if vision { Stdio::piped() } else { Stdio::null() })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            LlmError::new(LlmErrorKind::ProviderDown, format!("failed to spawn claude CLI: {e}"))
        })?;

        if vision {
            if let Some(mut stdin) = child.stdin.take() {
                let payload = build_vision_stream_payload(req);
                stdin.write_all(payload.as_bytes()).await.map_err(|e| {
                    LlmError::new(LlmErrorKind::ProviderDown, format!("failed writing vision stdin: {e}"))
                })?;
                let _ = stdin.shutdown().await;
            }
        }

        let output = tokio::time::timeout(CLI_TIMEOUT, child.wait_with_output())
            .await
            .map_err(|_| LlmError::new(LlmErrorKind::ProviderDown, "Claude CLI call timed out"))?
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("Claude CLI wait error: {e}")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(classify_cli_failure(&stderr, &stdout));
        }

        let parsed = if vision {
            parse_stream_json_output(&stdout)
        } else {
            parse_json_output(&stdout)
        };

        let (text, structured_json, usage) = parsed.map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e))?;
        Ok(LlmRawResponse {
            text,
            structured_json,
            usage,
            latency_ms: t0.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{JsonSchemaSpec, LlmMessage, TaskType};

    #[test]
    fn cli_args_use_safe_mode_not_bare_and_disable_tools() {
        let req = LlmRequest {
            task_type: TaskType::ReplyDrafting,
            model_id: "sonnet".into(),
            system_prompt: Some("Voice".into()),
            messages: vec![LlmMessage::user_text("Write reply")],
            temperature: None,
            reasoning_effort: Some("medium".into()),
            max_tokens: None,
            json_schema: Some(JsonSchemaSpec {
                name: "reply".into(),
                schema: json!({"type":"object"}),
            }),
            prompt_version: Some(1),
        };
        let args = build_cli_args(&req, false);
        assert!(args.contains(&"--safe-mode".to_string()));
        assert!(!args.contains(&"--bare".to_string()), "must never pass --bare");
        assert!(args.contains(&"--no-session-persistence".to_string()));
        assert!(args.windows(2).any(|w| w == ["--tools", ""]));
        assert!(args.windows(2).any(|w| w == ["--disallowedTools", "mcp__*"]));
        assert!(args.windows(2).any(|w| w == ["--output-format", "json"]));
        assert!(args.iter().any(|a| a == "--json-schema"));
    }

    #[test]
    fn vision_stream_json_embeds_inline_base64_without_read_tool() {
        let req = LlmRequest {
            task_type: TaskType::BrowserNavigation,
            model_id: "sonnet".into(),
            system_prompt: None,
            messages: vec![LlmMessage {
                role: LlmRole::User,
                parts: vec![
                    LlmContentPart::Text { text: "Click Like".into() },
                    LlmContentPart::ImageBase64 {
                        media_type: "image/png".into(),
                        data_base64: "AAAA".into(),
                    },
                ],
            }],
            temperature: None,
            reasoning_effort: None,
            max_tokens: None,
            json_schema: None,
            prompt_version: None,
        };
        let args = build_cli_args(&req, true);
        assert!(args.windows(2).any(|w| w == ["--input-format", "stream-json"]));
        assert!(args.windows(2).any(|w| w == ["--output-format", "stream-json"]));
        assert!(args.windows(2).any(|w| w == ["--tools", ""]));

        let line = build_vision_stream_payload(&req);
        let parsed: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(parsed["type"], "user");
        assert_eq!(parsed["message"]["content"][1]["type"], "image");
        assert_eq!(parsed["message"]["content"][1]["data"], "AAAA");
    }

    #[test]
    fn parses_structured_output_and_stream_json() {
        let out = r#"{"type":"result","is_error":false,"result":"","structured_output":{"variants":[{"text":"hi"}]},"usage":{"input_tokens":100,"output_tokens":20}}"#;
        let (text, structured, usage) = parse_json_output(out).unwrap();
        assert!(structured.is_some());
        assert!(text.contains("variants"));
        assert_eq!(usage.input_tokens, 100);

        let stream = "{\"type\":\"assistant\"}\n{\"type\":\"result\",\"is_error\":false,\"result\":\"done\",\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}\n";
        let (stext, _, susage) = parse_stream_json_output(stream).unwrap();
        assert_eq!(stext, "done");
        assert_eq!(susage.output_tokens, 5);
    }

    #[test]
    fn classifies_auth_and_offline_errors() {
        let auth = classify_cli_failure("Not logged in. Please run `claude login`", "");
        assert_eq!(auth.kind, LlmErrorKind::NotLoggedIn);
        let offline = classify_cli_failure("fetch failed: ENOTFOUND api.anthropic.com", "");
        assert_eq!(offline.kind, LlmErrorKind::Offline);
    }
}
