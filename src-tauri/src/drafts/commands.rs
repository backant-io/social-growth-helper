use super::bundle::{self, ActionKind};
use super::edit::{pre_publish_problems, DEFAULT_MAX_CHARS};
use super::generate::{generate, GenRequest, Instruction};
use super::repo::{self, DraftView};
use crate::error::{AppError, AppResult};
use crate::onboarding::require_acknowledged;
use crate::phase3::Phase3;
use crate::state::AppState;
use tauri::State;

#[tauri::command]
pub fn draft_open(state: State<'_, AppState>, evaluation_id: String) -> AppResult<DraftView> {
    let db = state.db.lock().unwrap();
    let id = repo::create_for_evaluation(&db, &state.profile_id, &evaluation_id)?;
    repo::get_view(&db, &id)
}

#[tauri::command]
pub fn draft_get(state: State<'_, AppState>, draft_id: String) -> AppResult<DraftView> {
    repo::get_view(&state.db.lock().unwrap(), &draft_id)
}

#[tauri::command]
pub fn draft_save_edit(state: State<'_, AppState>, draft_id: String, text: String) -> AppResult<DraftView> {
    let db = state.db.lock().unwrap();
    repo::save_edit(&db, &draft_id, &text)?;
    repo::get_view(&db, &draft_id)
}

#[tauri::command]
pub fn draft_select_variant(state: State<'_, AppState>, draft_id: String, variant_id: String) -> AppResult<DraftView> {
    let db = state.db.lock().unwrap();
    repo::select_variant(&db, &draft_id, &variant_id)?;
    repo::get_view(&db, &draft_id)
}

#[tauri::command]
pub fn draft_set_actions(state: State<'_, AppState>, draft_id: String, actions: Vec<String>) -> AppResult<DraftView> {
    let db = state.db.lock().unwrap();
    repo::set_actions(&db, &draft_id, &actions)?;
    repo::get_view(&db, &draft_id)
}

#[tauri::command]
pub fn draft_discard(state: State<'_, AppState>, draft_id: String, reason: Option<String>) -> AppResult<()> {
    repo::discard(&state.db.lock().unwrap(), &draft_id, reason.as_deref())
}

/// Problems that would block Publish right now, for inline display.
#[tauri::command]
pub fn draft_check(state: State<'_, AppState>, draft_id: String) -> AppResult<Vec<String>> {
    let db = state.db.lock().unwrap();
    let v = repo::get_view(&db, &draft_id)?;
    let style = repo::load_style(&db, &state.profile_id)?;
    let recent = repo::recent_published_texts(&db, &state.profile_id, 50)?;
    let actions: Vec<ActionKind> = bundle::normalize(&v.actions)?;
    Ok(pre_publish_problems(&v.edited_text, &actions, DEFAULT_MAX_CHARS, &style.banned, &recent))
}

/// Generates variants (`instruction`: fresh, shorter, funnier, more_direct). Transform instructions replace
/// the draft text with the first new variant; "fresh" keeps the current text and only adds options.
#[tauri::command]
pub async fn draft_generate(state: State<'_, AppState>, p3: State<'_, Phase3>, draft_id: String, instruction: Option<String>, model: Option<String>) -> AppResult<DraftView> {
    let (view, style) = {
        let db = state.db.lock().unwrap();
        require_acknowledged(&db)?;
        let v = repo::get_view(&db, &draft_id)?;
        if !matches!(v.status.as_str(), "draft" | "failed") {
            return Err(AppError::Validation(format!("this draft is {} and can no longer be changed", v.status)));
        }
        (v, repo::load_style(&db, &state.profile_id)?)
    };
    let context = {
        let db = state.db.lock().unwrap();
        repo::post_context(&db, &view.post.observed_id)?
    };
    let instr = instruction.as_deref().and_then(Instruction::parse).unwrap_or(Instruction::Fresh);
    let provider = p3.completion();
    let current = (instr != Instruction::Fresh).then(|| view.edited_text.clone());
    let req = GenRequest {
        quote: view.kind == "quote",
        handle: &view.post.handle,
        post_text: &view.post.text,
        max_chars: DEFAULT_MAX_CHARS,
        style: &style,
        instruction: instr,
        current: current.as_deref(),
        angle: view.angle.as_deref(),
        model_hint: model.as_deref().filter(|m| !m.trim().is_empty()),
        context: context.as_deref(),
    };
    let generated = generate(&*provider, &req).await.map_err(|e| AppError::Other(e.to_string()))?;
    let db = state.db.lock().unwrap();
    let ids = repo::add_variants(&db, &draft_id, &generated.variants, &generated.model)?;
    if instr != Instruction::Fresh {
        if let Some(first) = ids.first() {
            repo::select_variant(&db, &draft_id, first)?;
        }
    }
    repo::get_view(&db, &draft_id)
}

/// Connected models the user can pick for a one-off redraft (LLM-15).
#[tauri::command]
pub fn draft_model_options(state: State<'_, AppState>) -> Vec<crate::llm::phase3_bridge::ModelOption> {
    crate::llm::phase3_bridge::model_options(&state.db.lock().unwrap())
}
