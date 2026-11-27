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
use super::health::{load_status, SyncStatus};
use super::pacing;
use super::sandbox::{canonical_period_days, execute_widget, WidgetQueryResult};
use super::schedule::{compute_schedule, now_unix_secs, ScheduleSummary};
use super::spec::{parse_untrusted_spec_json, WidgetPosition, WidgetSpec};
use super::views::{list_view_schemas, ViewSchemaInfo};
use crate::error::AppResult;
use crate::state::AppState;
use rusqlite::OptionalExtension;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
pub struct PeriodOverview {
    pub period_days: u32,
    pub period_label: String,
    pub captured_at: String,
    pub impressions: Option<i64>,
    pub engagements: Option<i64>,
    pub engagement_rate: Option<f64>,
    pub profile_visits: Option<i64>,
    pub followers_total: Option<i64>,
    pub follower_delta: Option<i64>,
    pub new_follows: Option<i64>,
    pub unfollows: Option<i64>,
    pub likes: Option<i64>,
    pub replies: Option<i64>,
    pub reposts: Option<i64>,
    pub bookmarks: Option<i64>,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ActivitySummary {
    pub scouted_posts: usize,
    pub evaluated_posts: usize,
    pub queued_posts: usize,
    pub drafted_replies: usize,
    pub published_replies: usize,
    pub drafted_ogc: usize,
    pub published_ogc: usize,
    pub engagement_actions: usize,
    pub likes_given: usize,
    pub bookmarks_saved: usize,
    pub reposts_made: usize,
    pub follows_made: usize,
    pub quotes_made: usize,
    pub synced_post_snapshots: usize,
    pub synced_account_snapshots: usize,
    pub unsynced_published_posts: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardSnapshot {
    pub active_layout: DashboardLayout,
    pub layouts: Vec<DashboardLayout>,
    pub widgets: Vec<EvaluatedWidget>,
    pub schedule: ScheduleSummary,
    pub sync_status: SyncStatus,
    pub builder_route: BuilderRouteStatus,
    pub window_days: u32,
    pub period_overview: Option<PeriodOverview>,
    pub activity_summary: ActivitySummary,
}

pub fn load_period_overview(
    conn: &rusqlite::Connection,
    profile_id: &str,
    window_days: u32,
) -> AppResult<Option<PeriodOverview>> {
    super::views::ensure_views(conn)?;
    let canon = canonical_period_days(window_days);
    Ok(conn
        .query_row(
            "SELECT period_days, period_label, captured_at, impressions, engagements, engagement_rate,
                    profile_visits, followers_total, follower_delta, new_follows, unfollows,
                    likes, replies, reposts, bookmarks, source
             FROM v_analytics_account_overview
             WHERE profile_id = ?1 AND period_days = ?2
             LIMIT 1",
            rusqlite::params![profile_id, canon],
            |r| {
                Ok(PeriodOverview {
                    period_days: r.get::<_, i64>(0)?.max(1) as u32,
                    period_label: r.get(1)?,
                    captured_at: r.get(2)?,
                    impressions: r.get(3)?,
                    engagements: r.get(4)?,
                    engagement_rate: r.get(5)?,
                    profile_visits: r.get(6)?,
                    followers_total: r.get(7)?,
                    follower_delta: r.get(8)?,
                    new_follows: r.get(9)?,
                    unfollows: r.get(10)?,
                    likes: r.get(11)?,
                    replies: r.get(12)?,
                    reposts: r.get(13)?,
                    bookmarks: r.get(14)?,
                    source: r.get(15)?,
                })
            },
        )
        .optional()?)
}

pub fn compute_activity_summary(conn: &rusqlite::Connection, profile_id: &str) -> AppResult<ActivitySummary> {
    let count_one = |sql: &str| -> AppResult<usize> {
        let n: i64 = conn.query_row(sql, [profile_id], |r| r.get(0))?;
        Ok(n.max(0) as usize)
    };

    let count_action = |action: &str| -> AppResult<usize> {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM action_events WHERE profile_id = ?1 AND ok = 1 AND action = ?2",
            rusqlite::params![profile_id, action],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as usize)
    };

    super::views::ensure_views(conn)?;

    Ok(ActivitySummary {
        scouted_posts: count_one("SELECT COUNT(*) FROM observed_posts WHERE profile_id = ?1")?,
        evaluated_posts: count_one(
            "SELECT COUNT(*) FROM scout_evaluations se
             JOIN observed_posts op ON op.id = se.observed_post_id
             WHERE op.profile_id = ?1",
        )?,
        queued_posts: count_one(
            "SELECT COUNT(*) FROM scout_evaluations se
             JOIN observed_posts op ON op.id = se.observed_post_id
             WHERE op.profile_id = ?1 AND se.status = 'queued'",
        )?,
        drafted_replies: count_one("SELECT COUNT(*) FROM reply_drafts WHERE profile_id = ?1")?,
        published_replies: count_one(
            "SELECT COUNT(*) FROM reply_drafts WHERE profile_id = ?1 AND status = 'published'",
        )?,
        drafted_ogc: count_one("SELECT COUNT(*) FROM ogc_drafts WHERE profile_id = ?1")?,
        published_ogc: count_one(
            "SELECT COUNT(*) FROM ogc_drafts WHERE profile_id = ?1 AND status = 'published'",
        )?,
        engagement_actions: count_one(
            "SELECT COUNT(*) FROM action_events WHERE profile_id = ?1 AND ok = 1 AND action IN ('like','bookmark','repost','follow','quote')",
        )?,
        likes_given: count_action("like")?,
        bookmarks_saved: count_action("bookmark")?,
        reposts_made: count_action("repost")?,
        follows_made: count_action("follow")?,
        quotes_made: count_action("quote")?,
        synced_post_snapshots: count_one(
            "SELECT COUNT(DISTINCT platform_post_id) FROM analytics_snapshots
             WHERE profile_id = ?1 AND scope = 'post' AND platform_post_id IS NOT NULL",
        )?,
        synced_account_snapshots: count_one(
            "SELECT
               (SELECT COUNT(*) FROM analytics_snapshots WHERE profile_id = ?1 AND scope = 'account') +
               (SELECT COUNT(*) FROM analytics_period_snapshots WHERE profile_id = ?1)",
        )?,
        unsynced_published_posts: count_one(
            "SELECT COUNT(*) FROM v_analytics_post_performance
             WHERE profile_id = ?1 AND impressions IS NULL",
        )?,
    })
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
    if let Some(days) = window_days {
        let _ = pacing::set_preferred_window_days(&conn, days);
    }
    let widgets = dashboard::evaluate_layout(&conn, &state.profile_id, &active_layout.id, Some(eff_window))?;
    let schedule = compute_schedule(&conn, &state.profile_id, now_unix_secs())?;
    let sync_status = load_status(&conn);
    let builder_route = get_builder_route_status(&conn)?;
    let period_overview = load_period_overview(&conn, &state.profile_id, eff_window)?;
    let activity_summary = compute_activity_summary(&conn, &state.profile_id)?;

    Ok(DashboardSnapshot {
        active_layout,
        layouts,
        widgets,
        schedule,
        sync_status,
        builder_route,
        window_days: eff_window,
        period_overview,
        activity_summary,
    })
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
    {
        let conn = state.db.lock().unwrap();
        state
            .llm_router
            .sync_from_db(&conn, state.secrets.clone(), state.browser.clone())?;
    }
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
