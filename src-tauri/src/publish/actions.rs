//! Page-level steps for the six engagement actions. Every state-changing step reads the page state
//! first and again after, so repeating a step is safe (like never un-likes, repost never un-reposts).
//! Clicks go through real CDP mouse events at the element's centre; text goes through `insert_text`.

use crate::browser::{BrowserDriver, BrowserError};
use crate::completion::{CompletionProvider, TaskSlot};
use crate::control::KillSwitch;
use crate::drafts::bundle::ActionKind;
use crate::selectors::{self, sel, Selector};
use serde::Deserialize;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

static NEVER: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub poll_ms: u64,
    pub polls: u32,
    pub settle_ms: u64,
}

impl Timing {
    pub const LIVE: Timing = Timing { poll_ms: 400, polls: 12, settle_ms: 700 };
    pub const INSTANT: Timing = Timing { poll_ms: 0, polls: 6, settle_ms: 0 };
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Done { detail: Option<String>, post_id: Option<String> },
    AlreadyDone,
    Failed(String),
    /// The click happened but the result could not be confirmed. Never retried blindly.
    Uncertain(String),
}

impl Outcome {
    fn done() -> Self {
        Outcome::Done { detail: None, post_id: None }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Probe {
    pub page: String,
    pub found: bool,
    #[serde(default)]
    pub text: String,
    pub like: String,
    pub bookmark: String,
    pub repost: String,
    pub follow: String,
}

#[derive(Debug, Deserialize)]
struct SubmitResult {
    #[serde(default)]
    holds: bool,
    #[serde(default)]
    toast: Option<String>,
    #[serde(default)]
    id: Option<String>,
}

pub struct Page<'a> {
    pub d: &'a dyn BrowserDriver,
    pub post_id: &'a str,
    pub handle: &'a str,
    pub me: Option<String>,
    pub t: Timing,
    pub kill: &'a KillSwitch,
    /// Vision fallback for controls whose selector no longer matches. Only set when a model is routed.
    pub vision: Option<&'a dyn CompletionProvider>,
    pub vision_hits: AtomicU32,
}

fn err(e: BrowserError) -> String {
    e.to_string()
}

impl Page<'_> {
    async fn nap(&self, ms: u64) -> bool {
        ms == 0 || self.kill.sleep(Duration::from_millis(ms), &NEVER).await
    }

    pub async fn probe(&self) -> Result<Probe, BrowserError> {
        let v = self.d.eval_json(&selectors::probe_js(self.post_id)).await?;
        serde_json::from_value(v).map_err(|e| BrowserError::Evaluation(format!("unexpected probe payload: {e}")))
    }

    async fn rect(&self, scope: Option<&str>, s: &Selector, needle: Option<&str>) -> Result<Option<(f64, f64)>, BrowserError> {
        let v = self.d.eval_json(&selectors::rect_js(scope, s, needle)).await?;
        Ok(match (v.get("x").and_then(|x| x.as_f64()), v.get("y").and_then(|y| y.as_f64())) {
            (Some(x), Some(y)) => Some((x, y)),
            _ => None,
        })
    }

    /// Scrolls the element into view, waits briefly, then clicks its centre. When the selector finds nothing and a
    /// browser-navigation model is routed, asks it to point at the control from a screenshot (LLM-20).
    /// `false` when it is not on the page.
    async fn click(&self, scope: Option<&str>, s: &Selector, needle: Option<&str>) -> Result<bool, BrowserError> {
        self.click_with(scope, s, needle, true).await
    }

    async fn click_with(&self, scope: Option<&str>, s: &Selector, needle: Option<&str>, look: bool) -> Result<bool, BrowserError> {
        let found = match self.rect(scope, s, needle).await? {
            Some(p) => Some(p),
            None if look => self.look_for(s, needle).await,
            None => None,
        };
        let Some((x, y)) = found else { return Ok(false) };
        self.nap(self.t.settle_ms / 2).await;
        self.d.click_point(x, y).await?;
        self.nap(self.t.settle_ms).await;
        Ok(true)
    }

    async fn look_for(&self, s: &Selector, needle: Option<&str>) -> Option<(f64, f64)> {
        let provider = self.vision.filter(|p| p.is_configured(TaskSlot::BrowserNavigation))?;
        let what = s.name.to_lowercase().replace('_', " ");
        let goal = match needle {
            Some(n) => format!("the \"{n}\" menu item ({what})"),
            None => format!("the {what} control"),
        };
        let goal = format!("{goal} belonging to the post by @{} (post id {})", self.handle, self.post_id);
        let found = super::vision::locate(provider, self.d, self.kill, &goal).await.ok()?;
        self.vision_hits.fetch_add(1, Ordering::Relaxed);
        Some(found)
    }

    pub fn vision_used(&self) -> u32 {
        self.vision_hits.load(Ordering::Relaxed)
    }

    async fn wait_probe(&self, want: impl Fn(&Probe) -> bool) -> Result<bool, String> {
        for _ in 0..self.t.polls {
            if let Ok(p) = self.probe().await {
                if want(&p) {
                    return Ok(true);
                }
            }
            if self.kill.is_engaged() {
                return Err("stopped by the kill switch".into());
            }
            self.nap(self.t.poll_ms).await;
        }
        Ok(false)
    }

    async fn wait_eval_bool(&self, script: &str, want: bool) -> Result<bool, String> {
        for _ in 0..self.t.polls {
            if let Ok(v) = self.d.eval_json(script).await {
                if v.as_bool() == Some(want) || (want && v.is_string()) {
                    return Ok(true);
                }
            }
            if self.kill.is_engaged() {
                return Err("stopped by the kill switch".into());
            }
            self.nap(self.t.poll_ms).await;
        }
        Ok(false)
    }

    pub async fn perform_toggle(&self, kind: ActionKind, undo: bool) -> Outcome {
        match self.toggle(kind, undo).await {
            Ok(o) => o,
            Err(e) => Outcome::Failed(e),
        }
    }

    async fn toggle(&self, kind: ActionKind, undo: bool) -> Result<Outcome, String> {
        let p = self.probe().await.map_err(err)?;
        if !p.found {
            return Ok(Outcome::Failed("the post is not on the page".into()));
        }
        let (state, on, off): (&str, &Selector, &Selector) = match kind {
            ActionKind::Like => (&p.like, &sel::LIKE, &sel::UNLIKE),
            ActionKind::Bookmark => (&p.bookmark, &sel::BOOKMARK, &sel::UNBOOKMARK),
            ActionKind::Repost => (&p.repost, &sel::RETWEET, &sel::UNRETWEET),
            ActionKind::Follow => return self.follow(&p.follow, undo).await,
            _ => return Ok(Outcome::Failed("not a toggle action".into())),
        };
        let wanted = if undo { "todo" } else { "done" };
        if state == wanted {
            return Ok(Outcome::AlreadyDone);
        }
        if state == "missing" {
            return Ok(Outcome::Failed(format!("the {} button was not found on this post", kind.as_str())));
        }
        let target = if undo { off } else { on };
        if !self.click(Some(self.post_id), target, None).await.map_err(err)? {
            return Ok(Outcome::Failed(format!("the {} button was not clickable", kind.as_str())));
        }
        if kind == ActionKind::Repost {
            let confirm = if undo { &sel::UNRETWEET_CONFIRM } else { &sel::RETWEET_CONFIRM };
            if !self.click(None, confirm, None).await.map_err(err)? {
                return Ok(Outcome::Failed("the repost confirmation menu did not appear".into()));
            }
        }
        let ok = self
            .wait_probe(|p| match kind {
                ActionKind::Like => p.like == wanted,
                ActionKind::Bookmark => p.bookmark == wanted,
                _ => p.repost == wanted,
            })
            .await?;
        Ok(if ok { Outcome::done() } else { Outcome::Failed(format!("{} did not register on the page", kind.as_str())) })
    }

    /// Inline follow button when the card renders one, otherwise the post's caret menu.
    async fn follow(&self, inline: &str, undo: bool) -> Result<Outcome, String> {
        let wanted = if undo { "todo" } else { "done" };
        match inline {
            "done" | "todo" => {
                if inline == wanted {
                    return Ok(Outcome::AlreadyDone);
                }
                let target = if undo { &sel::UNFOLLOW } else { &sel::FOLLOW };
                if !self.click(Some(self.post_id), target, None).await.map_err(err)? {
                    return Ok(Outcome::Failed("the follow button was not clickable".into()));
                }
                if undo {
                    self.click_with(None, &sel::UNFOLLOW_CONFIRM, None, false).await.map_err(err)?;
                }
                let ok = self.wait_probe(|p| p.follow == wanted).await?;
                Ok(if ok { Outcome::done() } else { Outcome::Failed("follow did not register on the page".into()) })
            }
            _ => self.follow_via_menu(undo).await,
        }
    }

    async fn follow_via_menu(&self, undo: bool) -> Result<Outcome, String> {
        if !self.click(Some(self.post_id), &sel::CARET, None).await.map_err(err)? {
            return Ok(Outcome::Failed("no follow control was found for this author".into()));
        }
        let texts = self.d.eval_json(&selectors::menu_texts_js()).await.map_err(err)?;
        let lower: Vec<String> = texts.as_array().map(|a| a.iter().filter_map(|t| t.as_str()).map(|t| t.to_lowercase()).collect()).unwrap_or_default();
        let handle = self.handle.to_lowercase();
        let following = lower.iter().any(|t| t.starts_with("unfollow") && t.contains(&format!("@{handle}")));
        let followable = lower.iter().any(|t| t.starts_with("follow") && t.contains(&format!("@{handle}")));
        let _ = self.d.click_point(1.0, 1.0).await;
        if undo && !following || !undo && following {
            return Ok(Outcome::AlreadyDone);
        }
        if !undo && !followable || undo && !following {
            return Ok(Outcome::Failed("the author menu has no follow option (you may not be able to follow this account)".into()));
        }
        if !self.click(Some(self.post_id), &sel::CARET, None).await.map_err(err)? {
            return Ok(Outcome::Failed("the author menu could not be reopened".into()));
        }
        let needle = format!("{}follow @{handle}", if undo { "un" } else { "" });
        if !self.click(None, &sel::MENU_ITEM, Some(&needle)).await.map_err(err)? {
            return Ok(Outcome::Failed("the follow menu item was not clickable".into()));
        }
        if undo {
            self.click_with(None, &sel::UNFOLLOW_CONFIRM, None, false).await.map_err(err)?;
        }
        Ok(Outcome::Done { detail: Some("via the author menu".into()), post_id: None })
    }

    /// Reply (from the post's reply button) or Quote (from the repost menu). Writes `text` and submits.
    pub async fn perform_text(&self, kind: ActionKind, text: &str) -> Outcome {
        match self.text_flow(kind, text).await {
            Ok(o) => o,
            Err(e) => Outcome::Failed(e),
        }
    }

    async fn text_flow(&self, kind: ActionKind, text: &str) -> Result<Outcome, String> {
        let snippet = selectors::fingerprint(text);
        if let Some(existing) = self.find_published(&snippet).await {
            return Ok(Outcome::Done { detail: Some("found your earlier submission on the page".into()), post_id: Some(existing) });
        }
        if kind == ActionKind::Reply {
            if !self.click(Some(self.post_id), &sel::REPLY, None).await.map_err(err)? {
                return Ok(Outcome::Failed("the reply button was not found".into()));
            }
        } else {
            if !self.click(Some(self.post_id), &sel::RETWEET, None).await.map_err(err)? {
                return Ok(Outcome::Failed("the repost menu was not found".into()));
            }
            if !self.click(None, &sel::MENU_ITEM, Some("quote")).await.map_err(err)? {
                return Ok(Outcome::Failed("the Quote option was not found in the menu".into()));
            }
        }
        let composer = selectors::composer_text_js();
        if !self.wait_eval_bool(&composer, true).await? {
            return Ok(Outcome::Failed("the composer did not open".into()));
        }
        self.d.insert_text(text).await.map_err(err)?;
        self.nap(self.t.settle_ms).await;
        let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        let read_back = self.d.eval_json(&composer).await.map_err(err)?;
        if read_back.as_str().map(norm).as_deref() != Some(norm(text).as_str()) {
            return Ok(Outcome::Failed("the composer text did not match the approved text; nothing was posted".into()));
        }
        if self.kill.is_engaged() {
            return Err("stopped by the kill switch before submitting".into());
        }
        if !self.wait_eval_bool(&selectors::submit_ready_js(), true).await? {
            return Ok(Outcome::Failed("the post button never became ready".into()));
        }
        let rect = self.d.eval_json(&selectors::submit_rect_js()).await.map_err(err)?;
        let (Some(x), Some(y)) = (rect.get("x").and_then(|v| v.as_f64()), rect.get("y").and_then(|v| v.as_f64())) else {
            return Ok(Outcome::Failed("the post button could not be located".into()));
        };
        self.d.click_point(x, y).await.map_err(err)?;
        self.confirm_submitted(kind, &snippet).await
    }

    /// Decides whether a pressed Post button worked by looking for what was posted: your reply's text on the
    /// page. An open composer proves nothing either way (X keeps an inline composer on a post page), so it is
    /// never used as a signal. If the reply is not visible yet, the post is reloaded and searched again before
    /// giving up. A quote lands on your profile rather than under the post, so it also accepts X's own success
    /// toast or the text leaving the composer.
    async fn confirm_submitted(&self, kind: ActionKind, snippet: &str) -> Result<Outcome, String> {
        let js = selectors::submit_result_js(self.post_id, self.me.as_deref(), snippet);
        let mut text_gone = 0u32;
        let mut toast_ok = false;
        let mut error_toast = false;
        for _ in 0..self.t.polls.saturating_mul(2) {
            if let Ok(v) = self.d.eval_json(&js).await {
                if let Ok(r) = serde_json::from_value::<SubmitResult>(v) {
                    if let Some(id) = r.id {
                        return Ok(Outcome::Done { detail: None, post_id: Some(id) });
                    }
                    toast_ok |= r.toast.as_deref() == Some("ok");
                    error_toast |= r.toast.as_deref() == Some("error");
                    if kind == ActionKind::Quote {
                        if r.holds {
                            text_gone = 0;
                        } else if toast_ok {
                            return Ok(Outcome::Done { detail: Some("X confirmed the post".into()), post_id: None });
                        } else {
                            text_gone += 1;
                            if text_gone >= 2 {
                                return Ok(Outcome::Done { detail: Some("the composer emptied after posting".into()), post_id: None });
                            }
                        }
                    }
                }
            }
            if self.kill.is_engaged() {
                return Err("stopped by the kill switch".into());
            }
            self.nap(self.t.poll_ms).await;
        }
        if kind == ActionKind::Reply {
            let url = crate::drafts::repo::post_url(self.handle, self.post_id);
            if self.d.navigate(&url).await.is_ok() {
                self.nap(self.t.settle_ms.saturating_mul(4)).await;
                for _ in 0..self.t.polls {
                    if let Some(id) = self.find_published(snippet).await {
                        return Ok(Outcome::Done { detail: Some("found your reply after reloading the post".into()), post_id: Some(id) });
                    }
                    if self.kill.is_engaged() {
                        return Err("stopped by the kill switch".into());
                    }
                    self.nap(self.t.poll_ms).await;
                }
            }
            if toast_ok {
                return Ok(Outcome::Done { detail: Some("X confirmed the reply; it was not visible under the post yet".into()), post_id: None });
            }
        }
        Ok(Outcome::Uncertain(if error_toast {
            "X showed an error and your reply was not found under the post; check X before retrying".into()
        } else {
            "your reply was not found under the post, even after reloading it; check X before retrying".into()
        }))
    }

    async fn find_published(&self, snippet: &str) -> Option<String> {
        if snippet.is_empty() {
            return None;
        }
        let v = self.d.eval_json(&selectors::find_published_js(self.post_id, self.me.as_deref(), snippet)).await.ok()?;
        v.as_str().map(str::to_string)
    }
}
