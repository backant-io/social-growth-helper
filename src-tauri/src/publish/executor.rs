//! Runs an approved bundle or single action in the visible X window, one step at a time.
//! Requires an [`Approval`]; there is no other way in. The driver lock is held for the whole job so a
//! scout tick or another publish cannot interleave with it (BRW-10).

use super::actions::{Outcome, Page, Timing};
use super::gate::Approval;
use super::pacing;
use super::repo as prepo;
use crate::browser::service::BrowserService;
use crate::browser::{BrowserDriver, BrowserError};
use crate::control::{EventSink, KillSwitch};
use crate::creator::drafts;
use crate::drafts::repo as drepo;
use crate::error::AppResult;
use crate::selectors;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct PublishEnv<'a> {
    pub db: &'a Mutex<Connection>,
    pub browser: &'a BrowserService,
    pub kill: &'a KillSwitch,
    pub sink: &'a dyn EventSink,
    pub profile_id: &'a str,
    pub timing: Timing,
    /// Pause between steps in milliseconds (humanlike spacing). Tests use zero.
    pub step_gap_ms: (u64, u64),
    /// Directory for before/after screenshots; `None` disables them.
    pub shots_dir: Option<PathBuf>,
    /// Routed model for the vision fallback; `None` keeps the selector-only behaviour.
    pub vision: Option<&'a dyn crate::completion::CompletionProvider>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JobResult {
    pub ok: bool,
    pub message: Option<String>,
    pub published_post_id: Option<String>,
}

fn now_unix() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn gap(range: (u64, u64)) -> u64 {
    if range.1 <= range.0 {
        return range.0;
    }
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() as u64).unwrap_or(0);
    range.0 + n % (range.1 - range.0 + 1)
}

fn emit_update(env: &PublishEnv, draft_id: &str, status: &str) {
    env.sink.emit("publish:update", json!({ "draft_id": draft_id, "status": status }));
}

async fn screenshot(env: &PublishEnv<'_>, d: &dyn crate::browser::BrowserDriver, tag: &str, ref_id: &str) -> Option<String> {
    let dir = env.shots_dir.as_ref()?;
    let png = d.screenshot_png().await.ok()?;
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(format!("{ref_id}-{tag}.png"));
    std::fs::write(&path, png).ok()?;
    Some(path.display().to_string())
}

fn fail(env: &PublishEnv, a: &Approval, msg: &str) -> JobResult {
    if let Some(did) = a.draft_id() {
        let db = env.db.lock().unwrap();
        let _ = drepo::set_status(&db, did, "failed", Some(msg));
        let _ = prepo::write_audit(&db, env.profile_id, a.kind(), Some(did), a.ref_id(), &[], None, None, false, Some(msg));
        drop(db);
        emit_update(env, did, "failed");
        env.sink.emit("notify", json!({ "kind": "publish_failed", "title": "Publish stopped", "body": msg }));
    }
    JobResult { ok: false, message: Some(msg.to_string()), published_post_id: None }
}

/// Executes everything the approval covers. Never returns an error: failures are recorded on the draft.
pub async fn execute(env: &PublishEnv<'_>, a: &Approval) -> JobResult {
    if env.kill.is_engaged() {
        return fail(env, a, "Stopped by the kill switch before anything ran.");
    }
    if let Some(did) = a.draft_id() {
        let db = env.db.lock().unwrap();
        let _ = drepo::set_status(&db, did, "publishing", None);
        drop(db);
        emit_update(env, did, "publishing");
    }

    let d = env.browser.lock().await;
    if !d.is_running() {
        drop(d);
        return fail(env, a, "The X window is not open. Open it from the sidebar and try again.");
    }
    let url = drepo::post_url(a.handle(), a.platform_post_id());
    if let Err(e) = d.navigate(&url).await {
        drop(d);
        return fail(env, a, &format!("Could not open the post: {e}"));
    }
    if env.timing.settle_ms > 0 {
        env.kill.sleep(Duration::from_millis(env.timing.settle_ms * 3), &std::sync::atomic::AtomicBool::new(false)).await;
    }

    let me = d
        .eval_json(&selectors::extract_feed_js())
        .await
        .ok()
        .and_then(|v| v.get("me").and_then(|m| m.as_str().map(str::to_string)));
    let page = Page { d: &*d, post_id: a.platform_post_id(), handle: a.handle(), me, t: env.timing, kill: env.kill, vision: env.vision, vision_hits: Default::default() };

    let mut probe = None;
    for _ in 0..env.timing.polls {
        if let Ok(p) = page.probe().await {
            if p.found || p.page != "ok" {
                probe = Some(p);
                break;
            }
        }
        if env.timing.poll_ms > 0 {
            tokio::time::sleep(Duration::from_millis(env.timing.poll_ms)).await;
        }
    }
    match probe {
        None => {
            drop(d);
            return fail(env, a, "The post could not be found on its page. It may have been deleted.");
        }
        Some(p) if p.page == "logged_out" => {
            drop(d);
            env.sink.emit("notify", json!({ "kind": "attention", "title": "X needs you", "body": "You appear to be logged out of X." }));
            return fail(env, a, "You are logged out of X. Log in in the X window, then retry.");
        }
        Some(p) if p.page == "challenge" => {
            drop(d);
            env.sink.emit("notify", json!({ "kind": "attention", "title": "X needs you", "body": "X is showing a challenge. Solve it in the X window." }));
            return fail(env, a, "X is showing a challenge. Solve it in the X window, then retry.");
        }
        Some(_) => {}
    }

    let before = screenshot(env, &*d, "before", a.ref_id()).await;
    let mut executed: Vec<String> = vec![];
    let mut published_id: Option<String> = None;
    let mut problem: Option<String> = None;
    let bundle = a.bundle();

    for (i, kind) in bundle.iter().copied().enumerate() {
        if env.kill.is_engaged() {
            problem = Some("Stopped by the kill switch. Finished steps are kept; retry to continue.".into());
            break;
        }
        if let Some(did) = a.draft_id() {
            let db = env.db.lock().unwrap();
            if matches!(drepo::step_status(&db, did, kind).ok().flatten().as_deref(), Some("done") | Some("skipped")) {
                continue;
            }
        }
        let allowed = {
            let db = env.db.lock().unwrap();
            pacing::check(&db, env.profile_id, kind, now_unix()).unwrap_or(Ok(()))
        };
        if let Err(why) = allowed {
            if let Some(did) = a.draft_id() {
                let db = env.db.lock().unwrap();
                let _ = drepo::upsert_step(&db, did, kind, "failed", Some(&why));
            }
            problem = Some(why);
            break;
        }

        let outcome = if kind.is_text() { page.perform_text(kind, a.text()).await } else { page.perform_toggle(kind, a.undo()).await };
        let (status, detail, ok): (&str, Option<String>, bool) = match &outcome {
            Outcome::Done { detail, post_id } => {
                if kind.is_text() {
                    published_id = post_id.clone();
                }
                ("done", detail.clone(), true)
            }
            Outcome::AlreadyDone => ("skipped", Some("already in that state".into()), true),
            Outcome::Failed(m) => ("failed", Some(m.clone()), false),
            Outcome::Uncertain(m) => ("uncertain", Some(m.clone()), false),
        };
        {
            let db = env.db.lock().unwrap();
            if let Some(did) = a.draft_id() {
                let _ = drepo::upsert_step(&db, did, kind, status, detail.as_deref());
            }
            if status != "skipped" {
                let _ = prepo::record_event(&db, env.profile_id, a.platform_post_id(), &prepo::event_name(kind, a.undo()), a.source(), ok);
            }
        }
        if let Some(did) = a.draft_id() {
            emit_update(env, did, "publishing");
        }
        if ok {
            executed.push(prepo::event_name(kind, a.undo()));
        } else {
            problem = Some(format!("{} failed: {}", kind.as_str(), detail.unwrap_or_default()));
            break;
        }
        if i + 1 < bundle.len() {
            let ms = gap(env.step_gap_ms);
            if ms > 0 && !env.kill.sleep(Duration::from_millis(ms), &std::sync::atomic::AtomicBool::new(false)).await {
                problem = Some("Stopped by the kill switch. Finished steps are kept; retry to continue.".into());
                break;
            }
        }
    }

    let after = screenshot(env, &*d, "after", a.ref_id()).await;
    let looked = page.vision_used();
    drop(d);

    let db = env.db.lock().unwrap();
    if looked > 0 {
        crate::activity::record(&db, env.profile_id, "action", "Vision fallback located a control", Some("A selector no longer matched, so the model found the control on a screenshot. Check for X layout changes."), Some(problem.is_none()), None);
    }
    let draft = a.draft_id();
    let _ = prepo::write_audit(&db, env.profile_id, a.kind(), draft, a.ref_id(), &executed, before.as_deref(), after.as_deref(), problem.is_none(), problem.as_deref());
    match (&problem, draft) {
        (None, Some(did)) => {
            let url = published_id.as_deref().map(|id| format!("https://x.com/i/status/{id}"));
            let _ = prepo::mark_published(&db, did, published_id.as_deref(), url.as_deref());
            drop(db);
            emit_update(env, did, "published");
            env.sink.emit("notify", json!({ "kind": "publish_done", "title": "Published", "body": "Your actions on the post finished." }));
        }
        (Some(msg), Some(did)) => {
            let _ = drepo::set_status(&db, did, "failed", Some(msg));
            drop(db);
            emit_update(env, did, "failed");
            env.sink.emit("notify", json!({ "kind": "publish_failed", "title": "Publish stopped", "body": msg }));
        }
        _ => {}
    }
    JobResult { ok: problem.is_none(), message: problem, published_post_id: published_id }
}

/// Refreshes a post's live state (like, bookmark, repost, follow) from the page. Opens the post first when
/// the window is elsewhere. Read-only. `None` when the window is closed or the post is not found.
pub async fn probe_state(browser: &BrowserService, handle: &str, post_id: &str, kill: &KillSwitch) -> AppResult<Option<super::actions::Probe>> {
    let d = browser.lock().await;
    if !d.is_running() {
        return Ok(None);
    }
    if !d.current_url().await.unwrap_or_default().contains(post_id) {
        let _ = d.navigate(&drepo::post_url(handle, post_id)).await;
        tokio::time::sleep(Duration::from_millis(2500)).await;
    }
    let page = Page { d: &*d, post_id, handle, me: None, t: Timing::LIVE, kill, vision: None, vision_hits: Default::default() };
    Ok(page.probe().await.ok().filter(|p| p.found))
}

pub const COMPOSE_URL: &str = "https://x.com/compose/post";
const COMPOSER: &str = "[data-testid=\"tweetTextarea_0\"]";
const FILE_INPUT: &str = "input[data-testid=\"fileInput\"]";
const ATTACHMENTS: &str = "[data-testid=\"attachments\"]";
const POST_BUTTON: &str = "[data-testid=\"tweetButton\"]";
const TOAST_LINK: &str = "[data-testid=\"toast\"] a[href*=\"/status/\"]";

#[derive(Clone, Copy)]
pub struct ComposeTiming {
    pub poll: Duration,
    pub composer_wait: Duration,
    pub image_wait: Duration,
    pub video_wait: Duration,
    pub confirm_wait: Duration,
}

impl Default for ComposeTiming {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(300),
            composer_wait: Duration::from_secs(12),
            image_wait: Duration::from_secs(30),
            video_wait: Duration::from_secs(180),
            confirm_wait: Duration::from_secs(15),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ComposeOutcome {
    Published { url: Option<String> },
    /// Nothing was posted; safe to retry.
    Failed { message: String },
    /// The post button was clicked but success was not seen. The post may be live, so it is never retried.
    Unconfirmed { message: String },
}

/// Publishes an approved Creator (OGC) post through the composer in the visible X window. Same serial worker and
/// kill switch as replies and actions. Never returns an error: the outcome is recorded on the draft and the audit log.
pub async fn execute_top_level(env: &PublishEnv<'_>, a: &Approval, t: ComposeTiming) -> JobResult {
    let Some((draft_id, text, media)) = a.top_level() else {
        return JobResult { ok: false, message: Some("This approval is not an original post.".into()), published_post_id: None };
    };
    let outcome = if env.kill.is_engaged() {
        ComposeOutcome::Failed { message: "Stopped by the kill switch before anything ran.".into() }
    } else {
        let d = env.browser.lock().await;
        let outcome = compose_and_post(&*d, env.kill, text, media, t).await.unwrap_or_else(|e| ComposeOutcome::Failed { message: e.to_string() });
        drop(d);
        outcome
    };
    record_top_level(env, a.ref_id(), draft_id, &outcome)
}

fn record_top_level(env: &PublishEnv<'_>, approval_ref: &str, draft_id: &str, outcome: &ComposeOutcome) -> JobResult {
    let db = env.db.lock().unwrap();
    let result = match outcome {
        ComposeOutcome::Published { url } => {
            let _ = drafts::mark_published(&db, env.profile_id, draft_id, url.as_deref());
            let _ = drafts::write_audit(&db, env.profile_id, draft_id, approval_ref, true, None, None, None);
            JobResult { ok: true, message: None, published_post_id: None }
        }
        ComposeOutcome::Failed { message } => {
            let _ = drafts::mark_failed(&db, env.profile_id, draft_id, message, false);
            let _ = drafts::write_audit(&db, env.profile_id, draft_id, approval_ref, false, None, None, Some(message));
            JobResult { ok: false, message: Some(message.clone()), published_post_id: None }
        }
        ComposeOutcome::Unconfirmed { message } => {
            let _ = drafts::mark_failed(&db, env.profile_id, draft_id, message, true);
            let _ = drafts::write_audit(&db, env.profile_id, draft_id, approval_ref, false, None, None, Some(message));
            JobResult { ok: false, message: Some(message.clone()), published_post_id: None }
        }
    };
    drop(db);
    match outcome {
        ComposeOutcome::Published { .. } => {
            env.sink.emit("notify", json!({ "kind": "publish_done", "title": "Published", "body": "Your post is live on X." }));
        }
        ComposeOutcome::Failed { message } | ComposeOutcome::Unconfirmed { message } => {
            env.sink.emit("notify", json!({ "kind": "publish_failed", "title": "Publish stopped", "body": message }));
        }
    }
    result
}

fn q(selector: &str) -> String {
    serde_json::to_string(selector).unwrap()
}

async fn wait_for(d: &dyn BrowserDriver, js: &str, limit: Duration, poll: Duration) -> Result<bool, BrowserError> {
    let deadline = tokio::time::Instant::now() + limit;
    loop {
        if d.eval_json(js).await?.as_bool().unwrap_or(false) {
            return Ok(true);
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(false);
        }
        tokio::time::sleep(poll).await;
    }
}

async fn center(d: &dyn BrowserDriver, selector: &str) -> Result<Option<(f64, f64)>, BrowserError> {
    let js = format!(
        "(() => {{ const e = document.querySelector({}); if (!e) return null; e.scrollIntoView({{block: 'center'}}); \
         const r = e.getBoundingClientRect(); return {{x: r.left + r.width / 2, y: r.top + r.height / 2}}; }})()",
        q(selector)
    );
    let v = d.eval_json(&js).await?;
    Ok(match (v.get("x").and_then(Value::as_f64), v.get("y").and_then(Value::as_f64)) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    })
}

fn squash(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn compose_failed(message: impl Into<String>) -> Result<ComposeOutcome, BrowserError> {
    Ok(ComposeOutcome::Failed { message: message.into() })
}

async fn compose_and_post(d: &dyn BrowserDriver, kill: &KillSwitch, text: &str, media: &[std::path::PathBuf], t: ComposeTiming) -> Result<ComposeOutcome, BrowserError> {
    if !d.is_running() {
        return compose_failed("The X window is not open. Open it from the sidebar and try again.");
    }
    d.navigate(COMPOSE_URL).await?;
    let present = format!("!!document.querySelector({})", q(COMPOSER));
    if !wait_for(d, &present, t.composer_wait, t.poll).await? {
        return compose_failed("The compose box did not appear. Check that you are logged in to X in the X window.");
    }
    let Some((x, y)) = center(d, COMPOSER).await? else {
        return compose_failed("The compose box could not be located.");
    };
    d.click_point(x, y).await?;
    d.insert_text(text).await?;

    let read_back = format!("(document.querySelector({}) || {{}}).innerText || ''", q(COMPOSER));
    let typed = d.eval_json(&read_back).await?;
    if squash(typed.as_str().unwrap_or("")) != squash(text) {
        return compose_failed("The text in the compose box did not match the approved text, so nothing was posted. Clear the compose box and try again.");
    }

    if !media.is_empty() {
        d.set_file_input_files(FILE_INPUT, media).await?;
        let video = media.iter().any(|p| p.extension().and_then(|e| e.to_str()).map(|e| matches!(e.to_lowercase().as_str(), "mp4" | "mov")).unwrap_or(false));
        let limit = if video { t.video_wait } else { t.image_wait };
        let attached = format!("!!document.querySelector({})", q(ATTACHMENTS));
        if !wait_for(d, &attached, limit, t.poll).await? {
            return compose_failed("The media did not finish attaching, so nothing was posted.");
        }
    }

    let enabled = format!("(() => {{ const b = document.querySelector({}); return !!b && b.getAttribute('aria-disabled') !== 'true' && !b.disabled; }})()", q(POST_BUTTON));
    if !wait_for(d, &enabled, Duration::from_secs(5), t.poll).await? {
        return compose_failed("The Post button is not available (the text may be too long for your X account, or media is still processing).");
    }
    if kill.is_engaged() {
        return compose_failed("Stopped by the kill switch before posting.");
    }
    let Some((bx, by)) = center(d, POST_BUTTON).await? else {
        return compose_failed("The Post button could not be located.");
    };

    // Past this point the post may go out, so no error below is treated as safe to retry.
    d.click_point(bx, by).await?;

    let closed = format!("!document.querySelector({})", q(COMPOSER));
    if !wait_for(d, &closed, t.confirm_wait, t.poll).await.unwrap_or(false) {
        return Ok(ComposeOutcome::Unconfirmed {
            message: "The Post button was clicked but X did not confirm. Check your profile before trying again.".into(),
        });
    }
    let href = format!("(document.querySelector({}) || {{}}).href || null", q(TOAST_LINK));
    let url = d.eval_json(&href).await.ok().and_then(|v| v.as_str().map(str::to_string)).filter(|u| u.contains("/status/"));
    Ok(ComposeOutcome::Published { url })
}

#[cfg(test)]
mod top_level_tests {
    use super::*;
    use crate::browser::{ChromeLaunchConfig, WindowBounds};
    use crate::control::NullSink;
    use crate::creator::drafts::{insert, NewDraft};
    use crate::drafts::repo::testutil::{acknowledge, db};
    use crate::publish::gate::approve_top_level;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex as StdMutex};

    type Script = dyn Fn(&str, &[String]) -> Value + Send + Sync;

    /// Page stand-in: `script(expression, calls_so_far)` plays the part of the web page.
    #[derive(Clone)]
    struct PageDriver {
        calls: Arc<StdMutex<Vec<String>>>,
        script: Arc<Script>,
    }

    impl PageDriver {
        fn new(script: impl Fn(&str, &[String]) -> Value + Send + Sync + 'static) -> Self {
            Self { calls: Arc::default(), script: Arc::new(script) }
        }
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn rec(&self, s: String) {
            self.calls.lock().unwrap().push(s);
        }
    }

    #[async_trait::async_trait]
    impl BrowserDriver for PageDriver {
        async fn launch(&mut self, _: ChromeLaunchConfig) -> Result<(), BrowserError> {
            Ok(())
        }
        fn is_running(&self) -> bool {
            true
        }
        async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
            self.rec(format!("navigate {url}"));
            Ok(())
        }
        async fn current_url(&self) -> Result<String, BrowserError> {
            Ok(String::new())
        }
        async fn eval_json(&self, expression: &str) -> Result<Value, BrowserError> {
            let calls = self.calls();
            Ok((self.script)(expression, &calls))
        }
        async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError> {
            Ok(vec![])
        }
        async fn ax_tree(&self, _: Option<u32>) -> Result<Value, BrowserError> {
            Ok(Value::Null)
        }
        async fn click_point(&self, x: f64, y: f64) -> Result<(), BrowserError> {
            self.rec(format!("click {x},{y}"));
            Ok(())
        }
        async fn wheel_scroll(&self, _: f64, _: f64, _: f64) -> Result<(), BrowserError> {
            Ok(())
        }
        async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
            self.rec(format!("insert {text}"));
            Ok(())
        }
        async fn set_bounds(&self, _: WindowBounds) -> Result<(), BrowserError> {
            Ok(())
        }
        async fn get_bounds(&self) -> Result<WindowBounds, BrowserError> {
            Ok(WindowBounds { left: 0, top: 0, width: 0, height: 0 })
        }
        async fn set_file_input_files(&self, _: &str, paths: &[PathBuf]) -> Result<(), BrowserError> {
            self.rec(format!("files {}", paths.len()));
            Ok(())
        }
        async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
            Ok(())
        }
    }

    fn fast() -> ComposeTiming {
        let ms = Duration::from_millis(5);
        ComposeTiming { poll: ms, composer_wait: Duration::from_millis(30), image_wait: Duration::from_millis(30), video_wait: Duration::from_millis(30), confirm_wait: Duration::from_millis(30) }
    }

    struct Harness {
        db: Mutex<Connection>,
        pid: String,
        draft: String,
    }

    fn harness(text: &str, media: &[PathBuf]) -> Harness {
        let (conn, pid) = db();
        acknowledge(&conn);
        let d = insert(&conn, &pid, &NewDraft { format_id: None, batch_id: None, topic_seed: "t", text, model_used: "m", status: "variant", similarity: None }).unwrap();
        if !media.is_empty() {
            crate::creator::drafts::set_media(&conn, &pid, &d.id, media).unwrap();
        }
        Harness { db: Mutex::new(conn), pid, draft: d.id }
    }

    fn approval(h: &Harness, text: &str) -> Approval {
        let conn = h.db.lock().unwrap();
        approve_top_level(&conn, &h.pid, &h.draft, text, 280, false).unwrap()
    }

    async fn run(h: &Harness, d: &PageDriver, a: &Approval, kill: &KillSwitch) -> JobResult {
        let browser = BrowserService::new(Box::new(d.clone()));
        let sink = NullSink;
        let env = PublishEnv {
            db: &h.db,
            browser: &browser,
            kill,
            sink: &sink,
            profile_id: &h.pid,
            timing: Timing::LIVE,
            step_gap_ms: (0, 0),
            shots_dir: None,
            vision: None,
        };
        execute_top_level(&env, a, fast()).await
    }

    fn status(h: &Harness) -> String {
        drafts::get(&h.db.lock().unwrap(), &h.pid, &h.draft).unwrap().unwrap().status
    }

    fn clicks(calls: &[String]) -> usize {
        calls.iter().filter(|c| c.starts_with("click")).count()
    }

    /// A page that behaves: composer present, echoes inserted text, post closes the composer, toast has a link.
    fn healthy() -> PageDriver {
        PageDriver::new(|expr, calls| {
            let posted = calls.iter().filter(|c| c.starts_with("click")).count() >= 2;
            if expr.contains("getBoundingClientRect") {
                return serde_json::json!({ "x": 10.0, "y": 20.0 });
            }
            if expr.contains("innerText") {
                return Value::String(calls.iter().rev().find_map(|c| c.strip_prefix("insert ").map(str::to_string)).unwrap_or_default());
            }
            if expr.contains("aria-disabled") || expr.contains("attachments") {
                return Value::Bool(true);
            }
            if expr.contains(".href") {
                return Value::String("https://x.com/me/status/99".into());
            }
            if expr.starts_with("!document") {
                return Value::Bool(posted);
            }
            Value::Bool(true)
        })
    }

    #[tokio::test]
    async fn happy_path_types_verifies_then_posts_and_marks_published() {
        let h = harness("Hello\nworld", &[]);
        let a = approval(&h, "Hello\nworld");
        let d = healthy();
        let kill = KillSwitch::new(false);
        let out = run(&h, &d, &a, &kill).await;
        assert!(out.ok, "{out:?}");
        assert_eq!(status(&h), "published");
        let calls = d.calls();
        assert_eq!(calls[0], "navigate https://x.com/compose/post");
        assert!(calls.iter().any(|c| c == "insert Hello\nworld"));
        assert_eq!(clicks(&calls), 2, "one click into the composer, one on Post");
    }

    #[tokio::test]
    async fn media_is_attached_before_posting() {
        let dir = tempfile::tempdir().unwrap();
        let img = dir.path().join("a.png");
        std::fs::write(&img, [0u8; 8]).unwrap();
        let h = harness("Hi", &[img]);
        let a = approval(&h, "Hi");
        let d = healthy();
        let out = run(&h, &d, &a, &KillSwitch::new(false)).await;
        assert!(out.ok, "{out:?}");
        let calls = d.calls();
        let files = calls.iter().position(|c| c == "files 1").unwrap();
        let last_click = calls.iter().rposition(|c| c.starts_with("click")).unwrap();
        assert!(files < last_click);
    }

    #[tokio::test]
    async fn text_mismatch_aborts_before_the_post_button_is_touched() {
        let h = harness("Approved text", &[]);
        let a = approval(&h, "Approved text");
        let d = PageDriver::new(|expr, _| {
            if expr.contains("getBoundingClientRect") {
                serde_json::json!({ "x": 1.0, "y": 1.0 })
            } else if expr.contains("innerText") {
                Value::String("something else entirely".into())
            } else {
                Value::Bool(true)
            }
        });
        let out = run(&h, &d, &a, &KillSwitch::new(false)).await;
        assert!(!out.ok && out.message.unwrap().contains("did not match"));
        assert_eq!(status(&h), "failed");
        assert_eq!(clicks(&d.calls()), 1, "only the click into the composer");
    }

    #[tokio::test]
    async fn missing_composer_fails_safely() {
        let h = harness("Hi", &[]);
        let a = approval(&h, "Hi");
        let d = PageDriver::new(|_, _| Value::Bool(false));
        let out = run(&h, &d, &a, &KillSwitch::new(false)).await;
        assert!(!out.ok && out.message.unwrap().contains("compose box"));
        assert_eq!(clicks(&d.calls()), 0);
        assert_eq!(status(&h), "failed");
    }

    #[tokio::test]
    async fn kill_switch_stops_before_posting() {
        let h = harness("Hi", &[]);
        let a = approval(&h, "Hi");
        let kill = KillSwitch::new(false);
        kill.engage();
        let d = healthy();
        let out = run(&h, &d, &a, &kill).await;
        assert!(!out.ok && out.message.unwrap().contains("kill switch"));
        assert_eq!(clicks(&d.calls()), 0);
    }

    #[tokio::test]
    async fn unseen_confirmation_after_the_click_is_unconfirmed_not_failed() {
        let h = harness("Hi", &[]);
        let a = approval(&h, "Hi");
        let d = PageDriver::new(|expr, calls| {
            if expr.contains("getBoundingClientRect") {
                serde_json::json!({ "x": 1.0, "y": 1.0 })
            } else if expr.contains("innerText") {
                Value::String(calls.iter().rev().find_map(|c| c.strip_prefix("insert ").map(str::to_string)).unwrap_or_default())
            } else if expr.starts_with("!document") {
                Value::Bool(false)
            } else {
                Value::Bool(true)
            }
        });
        let out = run(&h, &d, &a, &KillSwitch::new(false)).await;
        assert!(!out.ok);
        assert_eq!(status(&h), "unconfirmed");
    }

    #[tokio::test]
    async fn missing_permalink_still_counts_as_published() {
        let h = harness("Hi", &[]);
        let a = approval(&h, "Hi");
        let d = PageDriver::new(|expr, calls| {
            let posted = calls.iter().filter(|c| c.starts_with("click")).count() >= 2;
            if expr.contains("getBoundingClientRect") {
                serde_json::json!({ "x": 1.0, "y": 1.0 })
            } else if expr.contains("innerText") {
                Value::String(calls.iter().rev().find_map(|c| c.strip_prefix("insert ").map(str::to_string)).unwrap_or_default())
            } else if expr.contains(".href") {
                Value::Null
            } else if expr.starts_with("!document") {
                Value::Bool(posted)
            } else {
                Value::Bool(true)
            }
        });
        let out = run(&h, &d, &a, &KillSwitch::new(false)).await;
        assert!(out.ok);
        assert_eq!(status(&h), "published");
    }

    #[test]
    fn each_outcome_updates_its_draft_and_writes_one_audit_row() {
        let (conn, pid) = db();
        acknowledge(&conn);
        let mk = |text: &str| insert(&conn, &pid, &NewDraft { format_id: None, batch_id: None, topic_seed: "t", text, model_used: "m", status: "variant", similarity: None }).unwrap().id;
        let ids = [mk("post 0"), mk("post 1"), mk("post 2")];
        let h = Harness { db: Mutex::new(conn), pid, draft: ids[0].clone() };
        let browser = BrowserService::new(Box::new(healthy()));
        let kill = KillSwitch::new(false);
        let sink = NullSink;
        let env = PublishEnv {
            db: &h.db,
            browser: &browser,
            kill: &kill,
            sink: &sink,
            profile_id: &h.pid,
            timing: Timing::LIVE,
            step_gap_ms: (0, 0),
            shots_dir: None,
            vision: None,
        };
        assert!(record_top_level(&env, "ref-a", &ids[0], &ComposeOutcome::Published { url: Some("https://x.com/a/status/1".into()) }).ok);
        assert!(!record_top_level(&env, "ref-b", &ids[1], &ComposeOutcome::Failed { message: "x".into() }).ok);
        assert!(!record_top_level(&env, "ref-c", &ids[2], &ComposeOutcome::Unconfirmed { message: "y".into() }).ok);
        let conn = h.db.lock().unwrap();
        let statuses: Vec<String> = ids.iter().map(|id| drafts::get(&conn, &h.pid, id).unwrap().unwrap().status).collect();
        assert_eq!(statuses, vec!["published", "failed", "unconfirmed"]);
        let refs: Vec<String> = conn
            .prepare("SELECT approval_ref FROM publish_audit_log WHERE target_kind='ogc' ORDER BY approval_ref").unwrap()
            .query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert_eq!(refs, vec!["ref-a", "ref-b", "ref-c"]);
    }
}
