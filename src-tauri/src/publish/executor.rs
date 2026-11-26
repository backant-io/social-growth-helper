//! Runs an approved bundle or single action in the visible X window, one step at a time.
//! Requires an [`Approval`]; there is no other way in. The driver lock is held for the whole job so a
//! scout tick or another publish cannot interleave with it (BRW-10).

use super::actions::{Outcome, Page, Timing};
use super::gate::Approval;
use super::pacing;
use super::repo as prepo;
use crate::browser::service::BrowserService;
use crate::control::{EventSink, KillSwitch};
use crate::drafts::repo as drepo;
use crate::error::AppResult;
use crate::selectors;
use rusqlite::Connection;
use serde::Serialize;
use serde_json::json;
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
    let page = Page { d: &*d, post_id: a.platform_post_id(), handle: a.handle(), me, t: env.timing, kill: env.kill };

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
    drop(d);

    let db = env.db.lock().unwrap();
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
    let page = Page { d: &*d, post_id, handle, me: None, t: Timing::LIVE, kill };
    Ok(page.probe().await.ok().filter(|p| p.found))
}
