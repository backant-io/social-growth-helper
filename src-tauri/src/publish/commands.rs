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
        let _job = state.jobs.register("Publishing");
        let env = PublishEnv {
            db: &state.db,
            browser: &state.browser,
            kill: &p3.kill,
            sink: &sink,
            profile_id: &state.profile_id,
            timing: Timing::LIVE,
            step_gap_ms: (1500, 4000),
            shots_dir: Some(state.paths.data_dir.join("audit-shots")),
        };
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

/// The user pressed Publish and confirmed the exact text. Queues the whole approved bundle.
#[tauri::command]
pub fn publish_draft(state: State<'_, AppState>, p3: State<'_, Phase3>, draft_id: String, confirmed_text: String) -> AppResult<()> {
    let approval = {
        let db = state.db.lock().unwrap();
        gate::approve_bundle(&db, &state.profile_id, &draft_id, &confirmed_text, p3.kill.is_engaged())?
    };
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
        executor::probe_state(&state.browser, &post.handle, &post.platform_post_id, &p3.kill)
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
