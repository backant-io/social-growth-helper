use super::actions::Timing;
use super::executor::{self, JobResult, PublishEnv};
use super::gate::{self, Approval};
use super::pacing::{self, PacingConfig, PacingUsage};
use super::queue::JobRunner;
use super::repo::{self as prepo, ActionState};
use crate::drafts::bundle::ActionKind;
use crate::drafts::repo as drepo;
use crate::error::{AppError, AppResult};
use crate::phase3::{Phase3, TauriSink};
use crate::state::AppState;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

pub struct AppRunner {
    pub handle: AppHandle,
}

#[async_trait::async_trait]
impl JobRunner for AppRunner {
    async fn run(&self, a: &Approval) -> JobResult {
        let state = self.handle.state::<AppState>();
        let p3 = self.handle.state::<Phase3>();
        let sink = TauriSink(self.handle.clone());
        let completion = p3.completion();
        let _job = state.jobs.register("Publishing");
        if let Err(e) = crate::commands::ensure_publish_browser_ready(&state).await {
            return JobResult {
                ok: false,
                message: Some(format!("Could not prepare background publish browser: {e}")),
                published_post_id: None,
            };
        }
        let env = PublishEnv {
            db: &state.db,
            browser: &state.publish_browser,
            kill: &p3.kill,
            sink: &sink,
            profile_id: &state.profile_id,
            timing: Timing::LIVE,
            step_gap_ms: (1500, 4000),
            shots_dir: Some(state.paths.data_dir.join("audit-shots")),
            vision: Some(&*completion),
        };
        if a.top_level().is_some() {
            return executor::execute_top_level(&env, a, executor::ComposeTiming::default()).await;
        }
        executor::execute(&env, a).await
    }
}

/// Starts the serial worker. Called once from setup.
pub fn spawn_worker(app: &AppHandle) {
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let p3 = handle.state::<Phase3>();
        if let Some(rx) = p3.publish.take_receiver() {
            let runner = AppRunner { handle: handle.clone() };
            p3.publish.run_worker(rx, &runner).await;
        }
    });
}

/// The user pressed Publish and confirmed the exact text. Queues the whole approved bundle
/// on the separate invisible publish browser (sharing cookies/profile state with the Scout browser).
#[tauri::command]
pub async fn publish_draft(state: State<'_, AppState>, p3: State<'_, Phase3>, draft_id: String, confirmed_text: String) -> AppResult<()> {
    let approval = {
        let db = state.db.lock().unwrap();
        gate::approve_bundle(&db, &state.profile_id, &draft_id, &confirmed_text, p3.kill.is_engaged())?
    };
    crate::commands::ensure_publish_browser_ready(&state).await?;
    p3.publish.submit(approval);
    Ok(())
}

/// One of the no-text action buttons (or its reversal). Waits for the result so the button can update.
#[tauri::command]
pub async fn engage_action(state: State<'_, AppState>, p3: State<'_, Phase3>, observed_post_id: String, action: String, undo: bool) -> AppResult<JobResult> {
    let kind = ActionKind::parse(&action)?;
    let approval = {
        let db = state.db.lock().unwrap();
        gate::approve_action(&db, &observed_post_id, kind, undo, p3.kill.is_engaged())?
    };
    crate::commands::ensure_publish_browser_ready(&state).await?;
    let rx = p3.publish.submit_and_wait(approval);
    rx.await.map_err(|_| AppError::Other("the publish worker stopped".into()))
}

#[derive(Serialize)]
pub struct LiveState {
    pub like: String,
    pub bookmark: String,
    pub repost: String,
    pub follow: String,
}

#[derive(Serialize)]
pub struct PostActionState {
    pub known: ActionState,
    pub live: Option<LiveState>,
}

#[tauri::command]
pub async fn get_action_state(state: State<'_, AppState>, p3: State<'_, Phase3>, observed_post_id: String, refresh: bool) -> AppResult<PostActionState> {
    let (post, known) = {
        let db = state.db.lock().unwrap();
        let post = drepo::post_by_observed(&db, &observed_post_id)?;
        let known = prepo::known_state(&db, &state.profile_id, &post.platform_post_id)?;
        (post, known)
    };
    let live = if refresh {
        crate::commands::ensure_publish_browser_ready(&state).await?;
        executor::probe_state(&state.publish_browser, &post.handle, &post.platform_post_id, &p3.kill)
            .await?
            .map(|p| LiveState { like: p.like, bookmark: p.bookmark, repost: p.repost, follow: p.follow })
    } else {
        None
    };
    Ok(PostActionState { known, live })
}

#[tauri::command]
pub fn get_pacing(state: State<'_, AppState>) -> AppResult<PacingConfig> {
    pacing::get(&state.db.lock().unwrap())
}

#[tauri::command]
pub fn set_pacing(state: State<'_, AppState>, config: PacingConfig) -> AppResult<PacingConfig> {
    pacing::set(&state.db.lock().unwrap(), &config)
}

#[tauri::command]
pub fn pacing_usage(state: State<'_, AppState>) -> AppResult<Vec<PacingUsage>> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    pacing::usage(&state.db.lock().unwrap(), &state.profile_id, now)
}

struct LiveToggler<'a> {
    state: &'a AppState,
    p3: &'a Phase3,
    observed_id: String,
    handle: String,
    post_id: String,
}

#[async_trait::async_trait]
impl super::checklist::Toggler for LiveToggler<'_> {
    async fn probe(&self) -> Result<Option<super::actions::Probe>, String> {
        executor::probe_state(&self.state.publish_browser, &self.handle, &self.post_id, &self.p3.kill).await.map_err(|e| e.to_string())
    }

    async fn toggle(&self, kind: ActionKind, undo: bool) -> JobResult {
        let approval = {
            let db = self.state.db.lock().unwrap();
            gate::approve_action(&db, &self.observed_id, kind, undo, self.p3.kill.is_engaged())
        };
        match approval {
            Ok(a) => match self.p3.publish.submit_and_wait(a).await {
                Ok(r) => r,
                Err(_) => JobResult { ok: false, message: Some("the publish worker stopped".into()), published_post_id: None },
            },
            Err(e) => JobResult { ok: false, message: Some(e.to_string()), published_post_id: None },
        }
    }
}

/// Runs the repeatable live check on a post the user names and stores the report with the selector version.
#[tauri::command]
pub async fn checklist_run(state: State<'_, AppState>, p3: State<'_, Phase3>, post_url: String) -> AppResult<super::checklist::ChecklistReport> {
    let (handle, post_id) = super::checklist::parse_post_url(&post_url)?;
    crate::commands::ensure_publish_browser_ready(&state).await?;
    let observed_id = {
        let db = state.db.lock().unwrap();
        prepo::ensure_observed(&db, &state.profile_id, &handle, &post_id)?
    };
    let toggler = LiveToggler { state: &state, p3: &p3, observed_id, handle, post_id };
    let items = super::checklist::run(&toggler).await;
    super::checklist::save(&state.db.lock().unwrap(), &state.profile_id, &post_url, &items)
}

#[tauri::command]
pub fn checklist_reports(state: State<'_, AppState>) -> AppResult<Vec<super::checklist::ChecklistReport>> {
    super::checklist::list(&state.db.lock().unwrap(), &state.profile_id, 20)
}
