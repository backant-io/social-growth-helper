//! Authenticated `x.com/i/grok` web-tab fallback provider (Architecture Plan section 4, fallback 2).
//!
//! Marked experimental/slower. Uses the user's open, signed-in Chrome window via [`BrowserService`].
//! Never used unless the user explicitly connects it and assigns it to a route or fallback.

use super::super::{
    LlmError, LlmErrorKind, LlmProvider, LlmRawResponse, LlmRequest, LlmUsage, ProviderCapabilities,
    ProviderHealth, ProviderStatus, ProviderType,
};
use crate::browser::service::BrowserService;
use crate::util::now_rfc3339;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const X_GROK_URL: &str = "https://x.com/i/grok";

const GROK_CHECK_READY_JS: &str = r#"(() => {
  const url = location.href || "";
  if (url.includes("/i/flow/login") || url.includes("/i/jf/onboarding/web")) {
    return { ready: false, reason: "logged_out", url };
  }
  const input = document.querySelector('textarea, div[contenteditable="true"][role="textbox"]');
  return { ready: !!input, reason: input ? "ok" : "no_composer", url };
})()"#;

const GROK_SNAPSHOT_JS: &str = r#"(() => {
  const nodes = Array.from(document.querySelectorAll('[data-testid*="grok"], main [dir="auto"], main article'));
  const texts = nodes.map(n => (n.innerText || '').trim()).filter(Boolean);
  return { count: texts.length, last: texts[texts.length - 1] || "" };
})()"#;

pub struct GrokWebTabProvider {
    account_id: String,
    browser: Arc<BrowserService>,
}

impl GrokWebTabProvider {
    pub fn new(account_id: impl Into<String>, browser: Arc<BrowserService>) -> Self {
        Self {
            account_id: account_id.into(),
            browser,
        }
    }

    fn build_combined_prompt(req: &LlmRequest) -> String {
        let mut out = String::new();
        if let Some(sys) = req.system_prompt.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str(sys);
            out.push_str("\n\n");
        }
        for msg in &req.messages {
            out.push_str(&msg.text_content());
            out.push('\n');
        }
        if let Some(spec) = &req.json_schema {
            out.push_str(&format!(
                "\nReturn ONLY valid JSON matching schema `{}`:\n{}",
                spec.name, spec.schema
            ));
        }
        out.trim().to_string()
    }
}

#[async_trait]
impl LlmProvider for GrokWebTabProvider {
    fn account_id(&self) -> &str {
        &self.account_id
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::GrokWeb
    }

    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            vision: false,
            json_schema: false,
            streaming: false,
            thinking: true,
        }
    }

    async fn health_check(&self) -> Result<ProviderStatus, LlmError> {
        let checked_at = now_rfc3339();
        if !self.browser.is_running().await {
            return Ok(ProviderStatus {
                health: ProviderHealth::Disconnected,
                message: "Open the X Chrome window in Settings and sign in to use the Grok web-tab fallback.".into(),
                checked_at,
                details: None,
            });
        }

        if let Err(e) = self.browser.navigate(X_GROK_URL).await {
            return Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("Could not navigate to x.com/i/grok: {e}"),
                checked_at,
                details: None,
            });
        }

        tokio::time::sleep(Duration::from_millis(800)).await;
        match self.browser.eval_json(GROK_CHECK_READY_JS).await {
            Ok(v) if v["ready"].as_bool() == Some(true) => Ok(ProviderStatus {
                health: ProviderHealth::Connected,
                message: "Authenticated x.com/i/grok tab ready (experimental fallback)".into(),
                checked_at,
                details: v["url"].as_str().map(ToString::to_string),
            }),
            Ok(v) if v["reason"].as_str() == Some("logged_out") => Ok(ProviderStatus {
                health: ProviderHealth::NeedsLogin,
                message: "X window is not logged in. Sign in to X in the browser window first.".into(),
                checked_at,
                details: v["url"].as_str().map(ToString::to_string),
            }),
            Ok(v) => Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("Grok composer not found on page ({})", v["reason"].as_str().unwrap_or("unknown")),
                checked_at,
                details: v["url"].as_str().map(ToString::to_string),
            }),
            Err(e) => Ok(ProviderStatus {
                health: ProviderHealth::Error,
                message: format!("Failed checking x.com/i/grok: {e}"),
                checked_at,
                details: None,
            }),
        }
    }

    async fn complete(&self, req: &LlmRequest) -> Result<LlmRawResponse, LlmError> {
        if !self.browser.is_running().await {
            return Err(LlmError::new(
                LlmErrorKind::BrowserNotRunning,
                "Grok web-tab fallback requires the X Chrome window to be open and signed in.",
            ));
        }

        let t0 = Instant::now();
        self.browser
            .navigate(X_GROK_URL)
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, format!("navigating to Grok tab: {e}")))?;

        let status = self
            .browser
            .eval_json(GROK_CHECK_READY_JS)
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e.to_string()))?;

        if status["reason"].as_str() == Some("logged_out") {
            return Err(LlmError::new(
                LlmErrorKind::NotLoggedIn,
                "X session is logged out; sign in to X inside the Chrome window.",
            ));
        }
        if status["ready"].as_bool() != Some(true) {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                "Could not find Grok prompt box on x.com/i/grok.",
            ));
        }

        let before = self
            .browser
            .eval_json(GROK_SNAPSHOT_JS)
            .await
            .unwrap_or(Value::Null);
        let before_count = before["count"].as_u64().unwrap_or(0);

        let coords = self
            .browser
            .eval_json(
                r#"(() => {
                  const el = document.querySelector('textarea, div[contenteditable="true"][role="textbox"]');
                  if (!el) return null;
                  const r = el.getBoundingClientRect();
                  el.focus();
                  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
                })()"#,
            )
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e.to_string()))?;

        let x = coords["x"].as_f64().ok_or_else(|| {
            LlmError::new(LlmErrorKind::ProviderDown, "Grok input box has no coordinates")
        })?;
        let y = coords["y"].as_f64().unwrap_or(0.0);

        let prompt = Self::build_combined_prompt(req);
        self.browser
            .click_point(x, y)
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e.to_string()))?;
        self.browser
            .insert_text(&prompt)
            .await
            .map_err(|e| LlmError::new(LlmErrorKind::ProviderDown, e.to_string()))?;

        // Submit via send button click.
        let send_coords = self
            .browser
            .eval_json(
                r#"(() => {
                  const btn = document.querySelector('button[aria-label*="Send"], button[data-testid*="send"], button[type="submit"]');
                  if (!btn) return null;
                  const r = btn.getBoundingClientRect();
                  return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
                })()"#,
            )
            .await
            .unwrap_or(Value::Null);

        if let (Some(sx), Some(sy)) = (send_coords["x"].as_f64(), send_coords["y"].as_f64()) {
            let _ = self.browser.click_point(sx, sy).await;
        }

        // Poll up to 25s for a stable response node.
        let mut last_text = String::new();
        let mut stable_ticks = 0u8;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(25);
        while tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(750)).await;
            if let Ok(snap) = self.browser.eval_json(GROK_SNAPSHOT_JS).await {
                let count = snap["count"].as_u64().unwrap_or(0);
                let txt = snap["last"].as_str().unwrap_or("").to_string();
                if count > before_count && !txt.is_empty() && txt != prompt {
                    if txt == last_text {
                        stable_ticks += 1;
                        if stable_ticks >= 2 {
                            break;
                        }
                    } else {
                        last_text = txt;
                        stable_ticks = 0;
                    }
                }
            }
        }

        if last_text.is_empty() {
            return Err(LlmError::new(
                LlmErrorKind::ProviderDown,
                "Timed out waiting for response on x.com/i/grok.",
            ));
        }

        Ok(LlmRawResponse {
            text: last_text,
            structured_json: None,
            usage: LlmUsage::default(),
            latency_ms: t0.elapsed().as_millis() as u64,
        })
    }
}
