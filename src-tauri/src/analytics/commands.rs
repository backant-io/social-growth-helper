//! Tauri IPC commands for Phase 4 Analytics & Dynamic Dashboard.
//!
//! Kept isolated under `analytics/` so `src-tauri/src/commands.rs` stays untouched for Phase 2/3 merges.

use super::builder::{
    get_builder_route_status, reprompt_widget_with_completer, BuilderRouteStatus, RepromptResult,
    RouterBuilderCompleter,
};
use super::csv::to_csv;
use super::dashboard::{
    self, DashboardLayout, DashboardWidget, EvaluatedWidget, WidgetPositionUpdate,
};
use super::health::{load_last_health, SyncHealthReport};
use super::ingest::{self, SyncOptions, SyncReport};
use super::sandbox::{execute_widget, WidgetQueryResult};
use super::schedule::{compute_schedule, now_unix_secs, ScheduleSummary};
use super::spec::{parse_untrusted_spec_json, WidgetPosition, WidgetSpec};
use super::views::{list_view_schemas, ViewSchemaInfo};
use crate::error::AppResult;
use crate::onboarding;
use crate::state::AppState;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub active_layout: DashboardLayout,
    pub layouts: Vec<DashboardLayout>,
    pub widgets: Vec<EvaluatedWidget>,
    pub schedule: ScheduleSummary,
    pub last_health: Option<SyncHealthReport>,
    pub builder_route: BuilderRouteStatus,
    pub window_days: u32,
}

#[tauri::command]
pub fn analytics_get_dashboard(
    state: State<'_, AppState>,
    window_days: Option<u32>,
) -> AppResult<DashboardSnapshot> {
    let conn = state.db.lock().unwrap();
    let active_layout = dashboard::ensure_active_layout(&conn, &state.profile_id)?;
    let layouts = dashboard::list_layouts(&conn, &state.profile_id)?;
    let eff_window = window_days.unwrap_or(30).clamp(1, 365);
    let widgets = dashboard::evaluate_layout(&conn, &state.profile_id, &active_layout.id, Some(eff_window))?;
    let schedule = compute_schedule(&conn, &state.profile_id, now_unix_secs())?;
    let last_health = load_last_health(&conn)?;
    let builder_route = get_builder_route_status(&conn)?;

    Ok(DashboardSnapshot {
        active_layout,
        layouts,
        widgets,
        schedule,
        last_health,
        builder_route,
        window_days: eff_window,
    })
}

#[tauri::command]
pub async fn analytics_sync_stats(
    state: State<'_, AppState>,
    options: Option<SyncOptions>,
) -> AppResult<SyncReport> {
    {
        let conn = state.db.lock().unwrap();
        onboarding::require_acknowledged(&conn)?;
    }
    ingest::sync_stats(
        &state.db,
        &state.profile_id,
        state.browser.as_ref(),
        &state.jobs,
        options.unwrap_or_default(),
    )
    .await
}

#[tauri::command]
pub fn analytics_list_views() -> Vec<ViewSchemaInfo> {
    list_view_schemas()
}

#[tauri::command]
pub fn analytics_switch_layout(state: State<'_, AppState>, layout_id: String) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::switch_layout(&conn, &state.profile_id, &layout_id)
}

#[tauri::command]
pub fn analytics_save_layout_version(
    state: State<'_, AppState>,
    layout_id: String,
    new_name: Option<String>,
) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::clone_layout(&conn, &state.profile_id, &layout_id, new_name.as_deref(), true)
}

#[tauri::command]
pub fn analytics_rename_layout(
    state: State<'_, AppState>,
    layout_id: String,
    new_name: String,
) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::rename_layout(&conn, &state.profile_id, &layout_id, &new_name)
}

#[tauri::command]
pub fn analytics_duplicate_layout(
    state: State<'_, AppState>,
    layout_id: String,
    new_name: String,
) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::clone_layout(&conn, &state.profile_id, &layout_id, Some(&new_name), true)
}

#[tauri::command]
pub fn analytics_delete_layout(state: State<'_, AppState>, layout_id: String) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::delete_layout(&conn, &state.profile_id, &layout_id)
}

#[tauri::command]
pub fn analytics_reset_starter_layout(state: State<'_, AppState>) -> AppResult<DashboardLayout> {
    let conn = state.db.lock().unwrap();
    dashboard::reset_to_starter(&conn, &state.profile_id)
}

#[tauri::command]
pub fn analytics_add_widget(
    state: State<'_, AppState>,
    layout_id: String,
    spec: WidgetSpec,
    position: Option<WidgetPosition>,
    source_prompt: Option<String>,
) -> AppResult<DashboardWidget> {
    let conn = state.db.lock().unwrap();
    dashboard::add_widget(&conn, &state.profile_id, &layout_id, spec, position, source_prompt)
}

#[tauri::command]
pub fn analytics_update_widget(
    state: State<'_, AppState>,
    layout_id: String,
    widget_id: String,
    spec: WidgetSpec,
    position: Option<WidgetPosition>,
    source_prompt: Option<String>,
) -> AppResult<DashboardWidget> {
    let conn = state.db.lock().unwrap();
    dashboard::update_widget(
        &conn,
        &state.profile_id,
        &layout_id,
        &widget_id,
        spec,
        position,
        source_prompt,
    )
}

#[tauri::command]
pub fn analytics_remove_widget(
    state: State<'_, AppState>,
    layout_id: String,
    widget_id: String,
) -> AppResult<()> {
    let conn = state.db.lock().unwrap();
    dashboard::remove_widget(&conn, &state.profile_id, &layout_id, &widget_id)
}

#[tauri::command]
pub fn analytics_update_widget_positions(
    state: State<'_, AppState>,
    layout_id: String,
    updates: Vec<WidgetPositionUpdate>,
) -> AppResult<Vec<DashboardWidget>> {
    let conn = state.db.lock().unwrap();
    dashboard::update_widget_positions(&conn, &state.profile_id, &layout_id, &updates)
}

#[tauri::command]
pub fn analytics_preview_widget_spec(
    state: State<'_, AppState>,
    spec_json: String,
    window_days: Option<u32>,
) -> AppResult<(WidgetSpec, WidgetQueryResult)> {
    let spec = parse_untrusted_spec_json(&spec_json)?;
    let conn = state.db.lock().unwrap();
    let result = execute_widget(&conn, &state.profile_id, &spec, window_days)?;
    Ok((spec, result))
}

#[tauri::command]
pub async fn analytics_reprompt_widget(
    state: State<'_, AppState>,
    prompt: String,
    current_spec: Option<WidgetSpec>,
    window_days: Option<u32>,
) -> AppResult<RepromptResult> {
    let completer = RouterBuilderCompleter {
        router: &state.llm_router,
        db: &state.db,
    };
    reprompt_widget_with_completer(
        &state.db,
        &state.profile_id,
        &prompt,
        current_spec.as_ref(),
        window_days,
        Some(&completer),
    )
    .await
}

#[tauri::command]
pub fn analytics_export_widget_csv(
    state: State<'_, AppState>,
    spec: WidgetSpec,
    window_days: Option<u32>,
) -> AppResult<String> {
    let conn = state.db.lock().unwrap();
    let result = execute_widget(&conn, &state.profile_id, &spec, window_days)?;
    Ok(to_csv(&result))
}
