//! A small stateful X simulator plus the executor tests that run against it. The fake recognises each
//! generated script by its content and the clicks by the coordinates it handed out, so the executor's
//! real selectors, state probes and retry logic are exercised end to end without a browser.

use crate::browser::service::BrowserService;
use crate::browser::{BrowserDriver, BrowserError, ChromeLaunchConfig, WindowBounds};
use crate::control::KillSwitch;
use crate::selectors::sel;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub const HANDLE: &str = "naval";

#[derive(Default)]
pub struct FakeState {
    pub page: String,
    pub found: bool,
    pub like: bool,
    pub bookmark: bool,
    pub repost: bool,
    pub follow: bool,
    pub inline_follow: bool,
    pub menu_open: bool,
    pub repost_menu: bool,
    pub composer_open: bool,
    pub composer_text: String,
    pub published: Vec<(String, String)>,
    pub calls: Vec<String>,
    pub ignore_like_clicks: bool,
    pub hide_like: bool,
    pub mangle_insert: bool,
    pub submit_hangs: bool,
    pub engage_after_like: Option<Arc<KillSwitch>>,
    next_id: u64,
}

#[derive(Clone)]
pub struct FakeX(pub Arc<Mutex<FakeState>>);

const TARGETS: &[(f64, &str)] = &[
    (1.0, "like"),
    (2.0, "unlike"),
    (3.0, "bookmark"),
    (4.0, "unbookmark"),
    (5.0, "retweet"),
    (6.0, "unretweet"),
    (7.0, "retweet_confirm"),
    (8.0, "unretweet_confirm"),
    (9.0, "follow"),
    (10.0, "unfollow"),
    (11.0, "unfollow_confirm"),
    (12.0, "caret"),
    (13.0, "reply"),
    (14.0, "menu_quote"),
    (15.0, "menu_follow"),
    (16.0, "menu_unfollow"),
    (17.0, "submit"),
];

fn has(js: &str, s: &crate::selectors::Selector) -> bool {
    js.contains(&json!(s.all()).to_string())
}

impl FakeX {
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(FakeState { page: "ok".into(), found: true, inline_follow: true, next_id: 9000, ..Default::default() })))
    }

    pub fn with(self, f: impl FnOnce(&mut FakeState)) -> Self {
        f(&mut self.0.lock().unwrap());
        self
    }

    pub fn service(&self) -> BrowserService {
        BrowserService::new(Box::new(self.clone()))
    }

    /// Names of the controls clicked so far, in order.
    pub fn clicks(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.iter().filter_map(|c| c.strip_prefix("click ").map(str::to_string)).collect()
    }

    pub fn inserts(&self) -> usize {
        self.0.lock().unwrap().calls.iter().filter(|c| c.starts_with("insert ")).count()
    }

    fn pair(on: bool, inline: bool) -> &'static str {
        if !inline {
            "missing"
        } else if on {
            "done"
        } else {
            "todo"
        }
    }

    fn rect_target(&self, js: &str) -> Option<&'static str> {
        let s = self.0.lock().unwrap();
        let name = if s.hide_like && has(js, &sel::LIKE) {
            return None;
        } else if has(js, &sel::UNLIKE) {
            "unlike"
        } else if has(js, &sel::LIKE) {
            "like"
        } else if has(js, &sel::UNBOOKMARK) {
            "unbookmark"
        } else if has(js, &sel::BOOKMARK) {
            "bookmark"
        } else if has(js, &sel::UNRETWEET_CONFIRM) {
            "unretweet_confirm"
        } else if has(js, &sel::RETWEET_CONFIRM) {
            "retweet_confirm"
        } else if has(js, &sel::UNRETWEET) {
            "unretweet"
        } else if has(js, &sel::RETWEET) {
            "retweet"
        } else if has(js, &sel::UNFOLLOW_CONFIRM) {
            "unfollow_confirm"
        } else if has(js, &sel::UNFOLLOW) {
            "unfollow"
        } else if has(js, &sel::FOLLOW) {
            "follow"
        } else if has(js, &sel::CARET) {
            "caret"
        } else if has(js, &sel::REPLY) {
            "reply"
        } else if has(js, &sel::MENU_ITEM) {
            if js.contains("\"quote\"") {
                "menu_quote"
            } else if js.contains("unfollow @") {
                "menu_unfollow"
            } else {
                "menu_follow"
            }
        } else {
            return None;
        };
        let available = match name {
            "like" => !s.like,
            "unlike" => s.like,
            "bookmark" => !s.bookmark,
            "unbookmark" => s.bookmark,
            "retweet" => !s.repost,
            "unretweet" => s.repost,
            "retweet_confirm" | "unretweet_confirm" | "menu_quote" => s.repost_menu,
            "follow" => s.inline_follow && !s.follow,
            "unfollow" => s.inline_follow && s.follow,
            "unfollow_confirm" => true,
            "menu_follow" | "menu_unfollow" => s.menu_open,
            _ => true,
        };
        if available {
            Some(name)
        } else {
            None
        }
    }

    fn click_effect(s: &mut FakeState, name: &str) {
        match name {
            "like" if !s.ignore_like_clicks => {
                s.like = true;
                if let Some(k) = &s.engage_after_like {
                    k.engage();
                }
            }
            "unlike" => s.like = false,
            "bookmark" => s.bookmark = true,
            "unbookmark" => s.bookmark = false,
            "retweet" | "unretweet" => s.repost_menu = true,
            "retweet_confirm" => {
                s.repost = true;
                s.repost_menu = false;
            }
            "unretweet_confirm" => {
                s.repost = false;
                s.repost_menu = false;
            }
            "follow" => s.follow = true,
            "unfollow" | "menu_unfollow" => s.follow = false,
            "menu_follow" => s.follow = true,
            "caret" => s.menu_open = true,
            "reply" => s.composer_open = true,
            "menu_quote" => {
                s.composer_open = true;
                s.repost_menu = false;
            }
            "submit" if s.composer_open => {
                let text = std::mem::take(&mut s.composer_text);
                s.next_id += 1;
                let id = s.next_id.to_string();
                s.published.push((id, text));
                if !s.submit_hangs {
                    s.composer_open = false;
                }
            }
            _ => {}
        }
    }
}

#[async_trait::async_trait]
impl BrowserDriver for FakeX {
    async fn launch(&mut self, _cfg: ChromeLaunchConfig) -> Result<(), BrowserError> {
        Ok(())
    }
    fn is_running(&self) -> bool {
        true
    }
    async fn navigate(&self, url: &str) -> Result<(), BrowserError> {
        self.0.lock().unwrap().calls.push(format!("navigate {url}"));
        Ok(())
    }
    async fn current_url(&self) -> Result<String, BrowserError> {
        Ok("https://x.com/home".into())
    }
    async fn eval_json(&self, js: &str) -> Result<Value, BrowserError> {
        let me = self.clone();
        let s = me.0.lock().unwrap();
        let out = if js.contains("posts.push") {
            json!({ "state": s.page, "me": "me", "posts": [] })
        } else if js.contains("const pair =") {
            if !s.found {
                json!({ "page": s.page, "found": false, "text": "", "like": "missing", "bookmark": "missing", "repost": "missing", "follow": "unknown" })
            } else {
                let follow = if s.inline_follow { Self::pair(s.follow, true) } else { "unknown" };
                json!({ "page": s.page, "found": true, "text": "a post", "like": Self::pair(s.like, true), "bookmark": Self::pair(s.bookmark, true), "repost": Self::pair(s.repost, true), "follow": follow })
            }
        } else if js.contains("scrollIntoView") {
            drop(s);
            return Ok(match self.rect_target(js) {
                Some(name) => {
                    let x = TARGETS.iter().find(|(_, n)| *n == name).map(|(x, _)| *x).unwrap_or(99.0);
                    json!({ "x": x, "y": 5.0 })
                }
                None => Value::Null,
            });
        } else if js.contains("return c ? c.innerText : null") {
            if s.composer_open {
                json!(s.composer_text)
            } else {
                Value::Null
            }
        } else if js.contains("aria-disabled") && js.contains("return false; }") {
            json!(s.composer_open && !s.composer_text.is_empty())
        } else if js.contains("aria-disabled") {
            json!({ "x": 17.0, "y": 5.0 })
        } else if js.contains("const m2") {
            let snippet = s.published.iter().find(|(_, t)| js.contains(&json!(t.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect::<String>()).to_string()));
            snippet.map(|(id, _)| json!(id)).unwrap_or(Value::Null)
        } else if js.contains("menuitem") && js.contains("map(e =>") {
            if s.menu_open {
                json!([if s.follow { format!("Unfollow @{HANDLE}") } else { format!("Follow @{HANDLE}") }])
            } else {
                json!([])
            }
        } else if js.contains("return !$all(") {
            json!(!s.composer_open)
        } else {
            Value::Null
        };
        Ok(out)
    }
    async fn screenshot_png(&self) -> Result<Vec<u8>, BrowserError> {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(100u32.to_be_bytes());
        png.extend(100u32.to_be_bytes());
        Ok(png)
    }
    async fn ax_tree(&self, _max_depth: Option<u32>) -> Result<Value, BrowserError> {
        Ok(Value::Null)
    }
    async fn click_point(&self, x: f64, _y: f64) -> Result<(), BrowserError> {
        let mut s = self.0.lock().unwrap();
        match TARGETS.iter().find(|(tx, _)| *tx == x) {
            Some((_, name)) => {
                s.calls.push(format!("click {name}"));
                Self::click_effect(&mut s, name);
            }
            None => {
                s.calls.push("click outside".into());
                s.menu_open = false;
            }
        }
        Ok(())
    }
    async fn wheel_scroll(&self, _x: f64, _y: f64, _d: f64) -> Result<(), BrowserError> {
        Ok(())
    }
    async fn insert_text(&self, text: &str) -> Result<(), BrowserError> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(format!("insert {text}"));
        if s.composer_open {
            let mangle = s.mangle_insert;
            s.composer_text = if mangle { format!("{text} (autocorrected)") } else { text.to_string() };
        }
        Ok(())
    }
    async fn set_bounds(&self, _b: WindowBounds) -> Result<(), BrowserError> {
        Ok(())
    }
    async fn close_gracefully(&mut self) -> Result<(), BrowserError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::CollectSink;
    use crate::drafts::bundle::ActionKind;
    use crate::drafts::repo::{self, testutil::*};
    use crate::publish::actions::Timing;
    use crate::publish::executor::{execute, PublishEnv};
    use crate::publish::gate::{approve_action, approve_bundle};
    use crate::publish::{pacing, repo as prepo};
    use rusqlite::Connection;

    struct Rig {
        db: Mutex<Connection>,
        profile: String,
        observed: String,
        draft: String,
        browser: BrowserService,
        fake: FakeX,
        kill: Arc<KillSwitch>,
        sink: CollectSink,
    }

    fn rig(actions: &[&str], tweak: impl FnOnce(&mut FakeState)) -> Rig {
        let (conn, profile) = db();
        acknowledge(&conn);
        let observed = post(&conn, &profile, "111", HANDLE, "a post worth answering");
        let draft = repo::create(&conn, &profile, &observed, None).unwrap();
        repo::save_edit(&conn, &draft, "my considered reply").unwrap();
        repo::set_actions(&conn, &draft, &actions.iter().map(|a| a.to_string()).collect::<Vec<_>>()).unwrap();
        let fake = FakeX::new().with(tweak);
        Rig { db: Mutex::new(conn), profile, observed, draft, browser: fake.service(), fake, kill: Arc::new(KillSwitch::new(false)), sink: CollectSink::default() }
    }

    impl Rig {
        fn env(&self) -> PublishEnv<'_> {
            PublishEnv {
                db: &self.db,
                browser: &self.browser,
                kill: &self.kill,
                sink: &self.sink,
                profile_id: &self.profile,
                timing: Timing::INSTANT,
                step_gap_ms: (0, 0),
                shots_dir: None,
                vision: None,
            }
        }

        async fn publish(&self) -> crate::publish::executor::JobResult {
            let a = {
                let db = self.db.lock().unwrap();
                approve_bundle(&db, &self.profile, &self.draft, "my considered reply", self.kill.is_engaged()).unwrap()
            };
            execute(&self.env(), &a).await
        }

        async fn single(&self, action: ActionKind, undo: bool) -> crate::publish::executor::JobResult {
            let a = approve_action(&self.db.lock().unwrap(), &self.observed, action, undo, false).unwrap();
            execute(&self.env(), &a).await
        }

        fn steps(&self) -> Vec<(String, String)> {
            let db = self.db.lock().unwrap();
            repo::steps(&db, &self.draft).unwrap().into_iter().map(|s| (s.action, s.status)).collect()
        }

        fn status(&self) -> String {
            repo::get_view(&self.db.lock().unwrap(), &self.draft).unwrap().status
        }
    }

    #[tokio::test]
    async fn a_bundle_runs_in_fixed_order_and_records_everything() {
        let r = rig(&["follow", "reply", "bookmark", "like"], |_| {});
        let res = r.publish().await;
        assert!(res.ok, "{:?}", res.message);
        assert_eq!(res.published_post_id.as_deref(), Some("9001"));
        assert_eq!(r.fake.clicks().iter().filter(|c| matches!(c.as_str(), "like" | "bookmark" | "follow" | "reply" | "submit")).cloned().collect::<Vec<_>>(), vec!["like", "bookmark", "follow", "reply", "submit"]);
        assert_eq!(r.status(), "published");
        assert!(r.steps().iter().all(|(_, s)| s == "done"), "{:?}", r.steps());
        let st = r.fake.0.lock().unwrap();
        assert_eq!(st.published, vec![("9001".to_string(), "my considered reply".to_string())]);
        assert!(st.like && st.bookmark && st.follow);
        drop(st);
        let db = r.db.lock().unwrap();
        let s = prepo::known_state(&db, &r.profile, "111").unwrap();
        assert!(s.like && s.bookmark && s.follow && s.replied);
        let audits: i64 = db.query_row("SELECT count(*) FROM publish_audit_log WHERE success = 1", [], |x| x.get(0)).unwrap();
        assert_eq!(audits, 1);
        assert!(r.sink.0.lock().unwrap().iter().any(|(k, _)| k == "publish:update"));
    }

    #[tokio::test]
    async fn a_hanging_submit_is_uncertain_and_retry_does_not_post_twice() {
        let r = rig(&["like", "reply"], |s| s.submit_hangs = true);
        let first = r.publish().await;
        assert!(!first.ok);
        assert_eq!(r.steps(), vec![("like".into(), "done".into()), ("reply".into(), "uncertain".into())]);
        assert_eq!(r.status(), "failed");
        let likes_before = r.fake.clicks().iter().filter(|c| *c == "like").count();

        let second = r.publish().await;
        assert!(second.ok, "{:?}", second.message);
        assert_eq!(r.fake.inserts(), 1, "the reply text must not be typed again");
        assert_eq!(r.fake.0.lock().unwrap().published.len(), 1);
        assert_eq!(r.fake.clicks().iter().filter(|c| *c == "like").count(), likes_before, "finished steps are skipped on retry");
        assert_eq!(r.status(), "published");
        assert_eq!(second.published_post_id.as_deref(), Some("9001"));
    }

    #[tokio::test]
    async fn a_failed_like_stops_the_bundle_and_retry_resumes_it() {
        let r = rig(&["like", "bookmark"], |s| s.ignore_like_clicks = true);
        let first = r.publish().await;
        assert!(!first.ok);
        assert!(first.message.unwrap().contains("like"));
        assert_eq!(r.steps(), vec![("like".into(), "failed".into()), ("bookmark".into(), "pending".into())]);
        assert!(!r.fake.0.lock().unwrap().bookmark, "later steps do not run after a failure");

        r.fake.0.lock().unwrap().ignore_like_clicks = false;
        assert!(r.publish().await.ok);
        assert!(r.steps().iter().all(|(_, s)| s == "done"));
    }

    #[tokio::test]
    async fn text_that_does_not_read_back_identically_is_never_submitted() {
        let r = rig(&["reply"], |s| s.mangle_insert = true);
        let res = r.publish().await;
        assert!(!res.ok);
        assert!(res.message.unwrap().contains("did not match"));
        assert!(!r.fake.clicks().contains(&"submit".to_string()));
        assert!(r.fake.0.lock().unwrap().published.is_empty());
    }

    #[tokio::test]
    async fn a_kill_switch_mid_bundle_keeps_finished_steps_and_runs_nothing_more() {
        let r = rig(&["like", "reply", "bookmark"], |_| {});
        r.fake.0.lock().unwrap().engage_after_like = Some(r.kill.clone());
        let res = r.publish().await;
        assert!(!res.ok);
        assert!(r.fake.0.lock().unwrap().published.is_empty());
        assert!(!r.fake.0.lock().unwrap().bookmark);
        let steps = r.steps();
        assert_eq!(steps[0], ("like".into(), "done".into()));
        assert_eq!(steps[1].1, "pending");
        assert_eq!(r.status(), "failed");
    }

    #[tokio::test]
    async fn an_engaged_kill_switch_prevents_any_browser_activity() {
        let r = rig(&["like"], |_| {});
        r.kill.engage();
        let a = {
            let db = r.db.lock().unwrap();
            approve_bundle(&db, &r.profile, &r.draft, "my considered reply", false).unwrap()
        };
        let res = execute(&r.env(), &a).await;
        assert!(!res.ok);
        assert!(r.fake.0.lock().unwrap().calls.is_empty());
        let db = r.db.lock().unwrap();
        assert!(approve_bundle(&db, &r.profile, &r.draft, "my considered reply", true).unwrap_err().to_string().contains("paused"));
        assert!(approve_action(&db, &r.observed, ActionKind::Like, false, true).is_err());
    }

    #[tokio::test]
    async fn pacing_caps_block_only_when_turned_on() {
        let r = rig(&["like"], |_| {});
        {
            let db = r.db.lock().unwrap();
            prepo::record_event(&db, &r.profile, "other", "like", "publish", true).unwrap();
            let cfg = pacing::PacingConfig { enabled: false, like: 1, ..Default::default() };
            pacing::set(&db, &cfg).unwrap();
        }
        assert!(r.publish().await.ok, "pacing is off by default and when disabled");

        let r = rig(&["like"], |_| {});
        {
            let db = r.db.lock().unwrap();
            prepo::record_event(&db, &r.profile, "other", "like", "publish", true).unwrap();
            pacing::set(&db, &pacing::PacingConfig { enabled: true, like: 1, ..Default::default() }).unwrap();
        }
        let res = r.publish().await;
        assert!(!res.ok);
        assert!(r.fake.clicks().is_empty(), "a blocked step never touches the page");
        assert_eq!(r.steps()[0].1, "failed");
    }

    #[tokio::test]
    async fn logged_out_and_challenge_pages_stop_before_any_click() {
        for (page, needle) in [("logged_out", "logged out"), ("challenge", "challenge")] {
            let r = rig(&["like"], |s| s.page = page.into());
            let res = r.publish().await;
            assert!(!res.ok);
            assert!(res.message.unwrap().contains(needle));
            assert!(r.fake.clicks().is_empty());
            assert!(r.sink.0.lock().unwrap().iter().any(|(k, v)| k == "notify" && v["kind"] == "attention"));
        }
    }

    #[tokio::test]
    async fn a_missing_post_fails_cleanly() {
        let r = rig(&["like"], |s| s.found = false);
        let res = r.publish().await;
        assert!(!res.ok);
        assert!(res.message.unwrap().contains("could not be found"));
    }

    #[tokio::test]
    async fn single_actions_toggle_and_reverse_and_skip_when_already_in_state() {
        let r = rig(&["like"], |_| {});
        let run = |action: ActionKind, undo: bool| r.single(action, undo);
        assert!(run(ActionKind::Repost, false).await.ok);
        assert!(r.fake.0.lock().unwrap().repost);
        assert!(run(ActionKind::Repost, false).await.ok);
        assert_eq!(r.fake.clicks().iter().filter(|c| *c == "retweet_confirm").count(), 1, "second repost is a no-op");
        assert!(run(ActionKind::Repost, true).await.ok);
        assert!(!r.fake.0.lock().unwrap().repost);
        assert!(run(ActionKind::Like, false).await.ok && run(ActionKind::Like, true).await.ok);
        assert!(!r.fake.0.lock().unwrap().like);
        assert!(run(ActionKind::Follow, false).await.ok && run(ActionKind::Follow, true).await.ok);
        assert!(r.fake.clicks().contains(&"unfollow_confirm".to_string()));
        assert!(!r.fake.0.lock().unwrap().follow);
        let db = r.db.lock().unwrap();
        let s = prepo::known_state(&db, &r.profile, "111").unwrap();
        assert!(!s.like && !s.repost && !s.follow);
    }

    #[tokio::test]
    async fn follow_falls_back_to_the_author_menu() {
        let r = rig(&["follow"], |s| s.inline_follow = false);
        let res = r.publish().await;
        assert!(res.ok, "{:?}", res.message);
        assert!(r.fake.0.lock().unwrap().follow);
        assert!(r.fake.clicks().contains(&"menu_follow".to_string()));
    }

    #[tokio::test]
    async fn quote_goes_through_the_repost_menu() {
        let r = rig(&["quote"], |_| {});
        let res = r.publish().await;
        assert!(res.ok, "{:?}", res.message);
        let clicks = r.fake.clicks();
        assert_eq!(clicks.iter().filter(|c| matches!(c.as_str(), "retweet" | "menu_quote" | "submit")).cloned().collect::<Vec<_>>(), vec!["retweet", "menu_quote", "submit"]);
        assert!(!r.fake.0.lock().unwrap().repost, "quote is not a plain repost");
    }

    /// The ownership gate is structural: only the Tauri command layer may construct an `Approval`, and
    /// scouting or drafting code never reaches the executor.
    #[test]
    fn only_the_command_layer_can_approve_and_scout_or_drafts_cannot_publish() {
        use std::path::Path;
        fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for e in std::fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![];
        walk(&root, &mut files);
        for f in files {
            let rel = f.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            let text = std::fs::read_to_string(&f).unwrap();
            let prod: String = text.split("#[cfg(test)]").next().unwrap_or("").lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
            let approves = prod.contains("approve_bundle(") || prod.contains("approve_action(");
            let allowed = matches!(rel.as_str(), "publish/gate.rs" | "publish/commands.rs");
            assert!(!approves || allowed, "{rel} must not mint approvals");
            if rel.starts_with("scout/") || rel.starts_with("drafts/") {
                assert!(!prod.contains("publish::") && !prod.contains("Approval"), "{rel} must not reach the publish layer");
            }
        }
    }

    #[tokio::test]
    async fn vision_fallback_finds_a_control_the_selector_missed_and_the_result_is_still_verified() {
        use crate::completion::ScriptedProvider;
        let r = rig(&["like"], |s| s.hide_like = true);
        let res = r.single(ActionKind::Like, false).await;
        assert!(!res.ok, "without a model the missing selector fails the step");
        assert!(!r.fake.0.lock().unwrap().like);

        let p = ScriptedProvider::new(vec![r#"{"action":"click","x":1,"y":5}"#]);
        let mut env = r.env();
        env.vision = Some(&p);
        let a = approve_action(&r.db.lock().unwrap(), &r.observed, ActionKind::Like, false, false).unwrap();
        let res = execute(&env, &a).await;
        assert!(res.ok, "{:?}", res.message);
        assert!(r.fake.0.lock().unwrap().like, "the page state confirms the like");
        assert_eq!(p.call_count(), 1);
        let log = crate::activity::list(&r.db.lock().unwrap(), &r.profile, 10).unwrap();
        assert!(log.iter().any(|e| e.title.contains("Vision fallback")));
    }

    #[tokio::test]
    async fn vision_giving_up_leaves_the_page_untouched() {
        use crate::completion::ScriptedProvider;
        let r = rig(&["like"], |s| s.hide_like = true);
        let p = ScriptedProvider::new(vec![r#"{"action":"give_up","reason":"no heart icon"}"#]);
        let mut env = r.env();
        env.vision = Some(&p);
        let a = approve_action(&r.db.lock().unwrap(), &r.observed, ActionKind::Like, false, false).unwrap();
        let res = execute(&env, &a).await;
        assert!(!res.ok);
        assert!(r.fake.clicks().is_empty());
    }

    struct FakeToggler<'a> {
        r: &'a Rig,
    }

    #[async_trait::async_trait]
    impl crate::publish::checklist::Toggler for FakeToggler<'_> {
        async fn probe(&self) -> Result<Option<crate::publish::actions::Probe>, String> {
            let page = crate::publish::actions::Page { d: &self.r.fake, post_id: "111", handle: HANDLE, me: None, t: Timing::INSTANT, kill: &self.r.kill, vision: None, vision_hits: Default::default() };
            Ok(page.probe().await.ok().filter(|p| p.found))
        }

        async fn toggle(&self, kind: ActionKind, undo: bool) -> crate::publish::executor::JobResult {
            self.r.single(kind, undo).await
        }
    }

    #[tokio::test]
    async fn the_live_check_does_and_undoes_each_toggle_and_stores_a_report() {
        let r = rig(&["like"], |s| s.follow = true);
        let items = crate::publish::checklist::run(&FakeToggler { r: &r }).await;
        let status = |a: &str| items.iter().find(|i| i.action == a).map(|i| i.status.clone()).unwrap();
        for a in ["like", "bookmark", "repost", "follow"] {
            assert_eq!(status(a), "pass", "{a}: {items:?}");
        }
        assert_eq!((status("reply"), status("quote")), ("manual".into(), "manual".into()));
        let s = r.fake.0.lock().unwrap();
        assert!(!s.like && !s.bookmark && !s.repost && s.follow, "every toggle ends in its starting state");
        drop(s);
        assert!(r.fake.inserts() == 0, "the check never types");

        let db = r.db.lock().unwrap();
        let saved = crate::publish::checklist::save(&db, &r.profile, "https://x.com/naval/status/111", &items).unwrap();
        assert_eq!(saved.registry_version, crate::selectors::REGISTRY_VERSION);
        let listed = crate::publish::checklist::list(&db, &r.profile, 5).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].items, items);
        assert!(crate::activity::list(&db, &r.profile, 5).unwrap().iter().any(|e| e.title.contains("passed")));
    }

    #[tokio::test]
    async fn the_live_check_reports_failures_and_skips_follow_when_absent() {
        let r = rig(&["like"], |s| {
            s.ignore_like_clicks = true;
            s.inline_follow = false;
        });
        let items = crate::publish::checklist::run(&FakeToggler { r: &r }).await;
        let status = |a: &str| items.iter().find(|i| i.action == a).map(|i| i.status.clone()).unwrap();
        assert_eq!(status("like"), "fail");
        assert_eq!(status("bookmark"), "pass");
        assert_eq!(status("follow"), "skipped");

        let r = rig(&["like"], |s| s.found = false);
        let items = crate::publish::checklist::run(&FakeToggler { r: &r }).await;
        assert_eq!(items[0].status, "fail");
        assert!(items.iter().any(|i| i.action == "reply" && i.status == "manual"));
    }
}
