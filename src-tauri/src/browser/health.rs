//! What state is the X window in, and what is the user being asked to do about it (BRW-06, BRW-08, BRW-11).
//!
//! Scout and Publish report a halt through the shared event sink; the app records it here. The UI polls
//! [`BrowserHealth::status`] (a read-only page probe that never waits behind a running job) and offers
//! Show X window / Resume / Abort for a halted job, or Relaunch after Chrome closed unexpectedly.

use super::service::{BrowserService, ProbeOutcome};
use crate::control::EventSink;
use crate::scout::repo::SessionParams;
use serde::Serialize;
use serde_json::json;
use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserState {
    Closed,
    Crashed,
    Ready,
    LoggedOut,
    Challenge,
    Suspended,
    Locked,
}

impl BrowserState {
    /// Maps the `pageState()` string of the page scripts. `None` for `ok` and anything unknown.
    pub fn from_page_state(s: &str) -> Option<Self> {
        match s {
            "logged_out" => Some(Self::LoggedOut),
            "challenge" => Some(Self::Challenge),
            "suspended" => Some(Self::Suspended),
            "locked" => Some(Self::Locked),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::Crashed => "crashed",
            Self::Ready => "ready",
            Self::LoggedOut => "logged_out",
            Self::Challenge => "challenge",
            Self::Suspended => "suspended",
            Self::Locked => "locked",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Self::Closed, Self::Crashed, Self::Ready, Self::LoggedOut, Self::Challenge, Self::Suspended, Self::Locked].into_iter().find(|k| k.as_str() == s)
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::Closed => "The X window is closed.",
            Self::Crashed => "The X window closed unexpectedly. Relaunch it to continue.",
            Self::Ready => "X is ready.",
            Self::LoggedOut => "You are logged out of X. Log in in the X window.",
            Self::Challenge => "X is showing a challenge. Complete it yourself in the X window.",
            Self::Suspended => "X says this account is suspended. Nothing will run until that is resolved on X.",
            Self::Locked => "X says this account is locked. Unlock it yourself in the X window.",
        }
    }
}

#[derive(Debug, Clone)]
struct Halt {
    state: BrowserState,
    /// `scout` or `publish`.
    source: String,
    scout_params: Option<SessionParams>,
}

#[derive(Default)]
struct Inner {
    expected_open: bool,
    crash_reported: bool,
    halt: Option<Halt>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BrowserStatus {
    pub state: BrowserState,
    pub message: String,
    /// The job that was stopped by this state, if any: `scout` or `publish`.
    pub job: Option<String>,
    /// Resume can restart the stopped job (a Scout session with its original settings).
    pub can_restart_job: bool,
}

pub struct ResumeOutcome {
    pub status: BrowserStatus,
    pub restart_scout: Option<SessionParams>,
}

#[derive(Default)]
pub struct BrowserHealth {
    inner: Mutex<Inner>,
}

/// Stops the current job with a halt: tells the app which state it hit and raises the usual
/// "X needs you" notification. Used by Scout and Publish.
pub fn report_halt(sink: &dyn EventSink, state: BrowserState, source: &str) {
    sink.emit("browser:halt", json!({ "state": state.as_str(), "source": source }));
    sink.emit("notify", json!({ "kind": "attention", "title": "X needs you", "body": state.message() }));
}

impl BrowserHealth {
    pub fn new() -> Self {
        Self::default()
    }

    /// The user (or a job) opened the X window. A later disappearance without `set_expected_open(false)` is a crash.
    pub fn set_expected_open(&self, open: bool) {
        let mut g = self.inner.lock().unwrap();
        g.expected_open = open;
        g.crash_reported = false;
    }

    pub fn record_halt(&self, state: BrowserState, source: &str) {
        self.inner.lock().unwrap().halt = Some(Halt { state, source: source.to_string(), scout_params: None });
    }

    /// Remembers the settings of the Scout session a halt stopped so Resume can start it again.
    pub fn attach_scout_params(&self, params: SessionParams) {
        if let Some(h) = self.inner.lock().unwrap().halt.as_mut().filter(|h| h.source == "scout") {
            h.scout_params = Some(params);
        }
    }

    /// Gives up on the stopped job. Returns whether there was one.
    pub fn abort(&self) -> bool {
        self.inner.lock().unwrap().halt.take().is_some()
    }

    /// `true` once per crash, so the watcher notifies a single time.
    pub fn take_new_crash(&self, running: bool) -> bool {
        let mut g = self.inner.lock().unwrap();
        if g.expected_open && !running && !g.crash_reported {
            g.crash_reported = true;
            return true;
        }
        false
    }

    fn view(&self, state: BrowserState) -> BrowserStatus {
        let g = self.inner.lock().unwrap();
        BrowserStatus {
            state,
            message: state.message().to_string(),
            job: g.halt.as_ref().map(|h| h.source.clone()),
            can_restart_job: g.halt.as_ref().is_some_and(|h| h.scout_params.is_some()),
        }
    }

    fn stored_or_ready(&self) -> BrowserState {
        self.inner.lock().unwrap().halt.as_ref().map(|h| h.state).unwrap_or(BrowserState::Ready)
    }

    pub async fn status(&self, svc: &BrowserService) -> BrowserStatus {
        let state = match svc.probe_page_state().await {
            ProbeOutcome::Closed => {
                if self.inner.lock().unwrap().expected_open {
                    BrowserState::Crashed
                } else {
                    BrowserState::Closed
                }
            }
            ProbeOutcome::State(s) => BrowserState::from_page_state(&s).unwrap_or(BrowserState::Ready),
            ProbeOutcome::Busy | ProbeOutcome::Error(_) => self.stored_or_ready(),
        };
        self.view(state)
    }

    /// Re-checks the page. When X no longer blocks anything the stopped job is released, and a stopped
    /// Scout session is handed back so the caller can start it again.
    pub async fn resume(&self, svc: &BrowserService) -> ResumeOutcome {
        let status = self.status(svc).await;
        if status.state != BrowserState::Ready {
            return ResumeOutcome { status, restart_scout: None };
        }
        let halt = self.inner.lock().unwrap().halt.take();
        let restart_scout = halt.and_then(|h| h.scout_params);
        ResumeOutcome { status: BrowserStatus { job: None, can_restart_job: false, ..status }, restart_scout }
    }
}

/// BRW-11: notices when Chrome disappears without the app having closed it, so the banner and an OS
/// notification offer a relaunch even when no job is running.
pub fn spawn_crash_watcher(app: &tauri::AppHandle) {
    use tauri::{Emitter, Manager};
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            let state = handle.state::<crate::state::AppState>();
            let closed = matches!(state.browser.probe_page_state().await, ProbeOutcome::Closed);
            if state.browser_health.take_new_crash(!closed) {
                let sink = crate::phase3::TauriSink(handle.clone());
                sink.emit("notify", json!({ "kind": "attention", "title": "X window closed", "body": BrowserState::Crashed.message() }));
                let _ = handle.emit("browser:halt", json!({ "state": "crashed", "source": "watcher" }));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::mock::MockDriver;
    use crate::browser::ChromeLaunchConfig;
    use crate::control::CollectSink;
    use serde_json::json;

    fn cfg() -> ChromeLaunchConfig {
        ChromeLaunchConfig { binary_path: "/x".into(), profile_dir: "/p".into(), start_url: "https://x.com/home".into(), width: 1, height: 1 }
    }

    fn params() -> SessionParams {
        SessionParams { feed_source: "for_you".into(), max_duration_sec: 60, max_posts: 10, stop_after_queued: None, threshold: 0.5 }
    }

    async fn svc_with_state(state: &'static str) -> (BrowserService, MockDriver) {
        let mock = MockDriver::new().with_eval_fn(move |_| json!({ "state": state, "url": "https://x.com/home" }));
        let svc = BrowserService::new(Box::new(mock.clone()));
        svc.launch(cfg()).await.unwrap();
        (svc, mock)
    }

    #[test]
    fn page_states_map_to_halts() {
        assert_eq!(BrowserState::from_page_state("challenge"), Some(BrowserState::Challenge));
        assert_eq!(BrowserState::from_page_state("suspended"), Some(BrowserState::Suspended));
        assert_eq!(BrowserState::from_page_state("locked"), Some(BrowserState::Locked));
        assert_eq!(BrowserState::from_page_state("logged_out"), Some(BrowserState::LoggedOut));
        assert_eq!(BrowserState::from_page_state("ok"), None);
        assert_eq!(BrowserState::parse("locked"), Some(BrowserState::Locked));
    }

    #[test]
    fn report_halt_emits_state_and_attention_notice() {
        let sink = CollectSink::default();
        report_halt(&sink, BrowserState::Suspended, "publish");
        let ev = sink.0.lock().unwrap();
        assert_eq!(ev[0].0, "browser:halt");
        assert_eq!(ev[0].1["state"], "suspended");
        assert_eq!(ev[1].0, "notify");
        assert_eq!(ev[1].1["kind"], "attention");
    }

    #[tokio::test]
    async fn closed_window_is_closed_until_it_was_expected_open_then_crashed() {
        let svc = BrowserService::new(Box::new(MockDriver::new()));
        let h = BrowserHealth::new();
        assert_eq!(h.status(&svc).await.state, BrowserState::Closed);
        h.set_expected_open(true);
        let st = h.status(&svc).await;
        assert_eq!(st.state, BrowserState::Crashed);
        assert!(st.message.contains("Relaunch"));
        h.set_expected_open(false);
        assert_eq!(h.status(&svc).await.state, BrowserState::Closed);
    }

    #[tokio::test]
    async fn crash_is_reported_once_and_rearms_on_relaunch() {
        let h = BrowserHealth::new();
        assert!(!h.take_new_crash(false), "never opened");
        h.set_expected_open(true);
        assert!(!h.take_new_crash(true));
        assert!(h.take_new_crash(false));
        assert!(!h.take_new_crash(false), "only once per crash");
        h.set_expected_open(true);
        assert!(h.take_new_crash(false));
    }

    #[tokio::test]
    async fn live_page_states_surface_in_status() {
        for (page, want) in [("logged_out", BrowserState::LoggedOut), ("challenge", BrowserState::Challenge), ("suspended", BrowserState::Suspended), ("locked", BrowserState::Locked), ("ok", BrowserState::Ready)] {
            let (svc, _m) = svc_with_state(page).await;
            let h = BrowserHealth::new();
            h.set_expected_open(true);
            assert_eq!(h.status(&svc).await.state, want, "{page}");
        }
    }

    #[tokio::test]
    async fn busy_browser_keeps_the_last_known_halt_without_waiting() {
        let (svc, _m) = svc_with_state("ok").await;
        let h = BrowserHealth::new();
        h.record_halt(BrowserState::Challenge, "publish");
        let _held = svc.lock().await;
        let st = h.status(&svc).await;
        assert_eq!(st.state, BrowserState::Challenge);
        assert_eq!(st.job.as_deref(), Some("publish"));
    }

    #[tokio::test]
    async fn resume_is_refused_while_x_still_blocks() {
        let (svc, _m) = svc_with_state("challenge").await;
        let h = BrowserHealth::new();
        h.record_halt(BrowserState::Challenge, "scout");
        h.attach_scout_params(params());
        let out = h.resume(&svc).await;
        assert_eq!(out.status.state, BrowserState::Challenge);
        assert!(out.restart_scout.is_none());
        assert_eq!(out.status.job.as_deref(), Some("scout"), "the job stays halted");
    }

    #[tokio::test]
    async fn resume_releases_the_job_and_returns_scout_settings_once_x_is_clear() {
        let (svc, _m) = svc_with_state("ok").await;
        let h = BrowserHealth::new();
        h.record_halt(BrowserState::Challenge, "scout");
        h.attach_scout_params(params());
        let out = h.resume(&svc).await;
        assert_eq!(out.status.state, BrowserState::Ready);
        assert_eq!(out.restart_scout.unwrap().max_posts, 10);
        assert_eq!(out.status.job, None);
        assert!(h.resume(&svc).await.restart_scout.is_none(), "restart happens once");
    }

    #[tokio::test]
    async fn resume_after_a_publish_halt_only_clears_it() {
        let (svc, _m) = svc_with_state("ok").await;
        let h = BrowserHealth::new();
        h.record_halt(BrowserState::LoggedOut, "publish");
        h.attach_scout_params(params());
        let st = h.status(&svc).await;
        assert!(!st.can_restart_job);
        let out = h.resume(&svc).await;
        assert!(out.restart_scout.is_none());
        assert_eq!(out.status.job, None);
    }

    #[tokio::test]
    async fn abort_drops_the_job_but_not_the_live_state() {
        let (svc, _m) = svc_with_state("logged_out").await;
        let h = BrowserHealth::new();
        h.record_halt(BrowserState::LoggedOut, "scout");
        assert!(h.abort());
        assert!(!h.abort());
        let st = h.status(&svc).await;
        assert_eq!(st.state, BrowserState::LoggedOut, "the persistent banner stays while X is logged out");
        assert_eq!(st.job, None);
    }

    #[tokio::test]
    async fn relaunch_after_crash_returns_to_ready_and_keeps_a_stopped_scout_resumable() {
        let mock = MockDriver::new().with_eval_fn(|_| json!({ "state": "ok", "url": "https://x.com/home" }));
        let svc = BrowserService::new(Box::new(mock.clone()));
        let h = BrowserHealth::new();
        svc.launch(cfg()).await.unwrap();
        h.set_expected_open(true);
        h.record_halt(BrowserState::Crashed, "scout");
        h.attach_scout_params(params());
        svc.close().await.unwrap();
        assert_eq!(h.status(&svc).await.state, BrowserState::Crashed);
        svc.launch(cfg()).await.unwrap();
        h.set_expected_open(true);
        let st = h.status(&svc).await;
        assert_eq!(st.state, BrowserState::Ready);
        assert!(st.can_restart_job);
    }
}
