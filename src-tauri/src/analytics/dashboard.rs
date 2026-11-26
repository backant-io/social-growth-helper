//! Configurable dashboard layouts, widget CRUD, layout versioning, and starter reset (ANL-07, ANL-15, ANL-21).

use super::sandbox::{execute_widget, WidgetQueryResult};
use super::spec::{validate_spec, WidgetPosition, WidgetSpec};
use super::starter::starter_widgets;
use crate::error::{AppError, AppResult};
use crate::util::{new_id, now_rfc3339};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardLayout {
    pub id: String,
    pub profile_id: String,
    pub name: String,
    pub version: u32,
    pub is_active: bool,
    pub is_starter: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DashboardWidget {
    pub id: String,
    pub layout_id: String,
    pub title: String,
    pub spec: WidgetSpec,
    pub position: WidgetPosition,
    pub source_prompt: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluatedWidget {
    pub widget: DashboardWidget,
    pub data: Option<WidgetQueryResult>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct WidgetPositionUpdate {
    pub widget_id: String,
    pub position: WidgetPosition,
}

const STARTER_NAME: &str = "Starter Overview";

/// Ensures the profile has at least one active dashboard layout, seeding the starter layout on first access.
pub fn ensure_active_layout(conn: &Connection, profile_id: &str) -> AppResult<DashboardLayout> {
    if let Some(active) = get_active_layout(conn, profile_id)? {
        return Ok(active);
    }
    let layouts = list_layouts(conn, profile_id)?;
    if let Some(first) = layouts.into_iter().next() {
        return switch_layout(conn, profile_id, &first.id);
    }
    create_starter_layout(conn, profile_id, true)
}

fn create_starter_layout(conn: &Connection, profile_id: &str, activate: bool) -> AppResult<DashboardLayout> {
    let next_ver = next_version_for_name(conn, profile_id, STARTER_NAME)?;
    let id = new_id();
    let now = now_rfc3339();
    if activate {
        conn.execute(
            "UPDATE dashboard_layouts SET is_active = 0 WHERE profile_id = ?1",
            [profile_id],
        )?;
    }
    conn.execute(
        "INSERT INTO dashboard_layouts (id, profile_id, name, version, is_active, is_starter, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
        params![id, profile_id, STARTER_NAME, next_ver, if activate { 1 } else { 0 }, now],
    )?;

    for w in starter_widgets() {
        let spec_json = serde_json::to_string(&w.spec)
            .map_err(|e| AppError::Other(format!("serializing starter spec: {e}")))?;
        let pos_json = serde_json::to_string(&w.position.normalized())
            .map_err(|e| AppError::Other(format!("serializing starter position: {e}")))?;
        conn.execute(
            "INSERT INTO dashboard_widgets (id, layout_id, title, spec_json, position_json, source_prompt, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6)",
            params![new_id(), id, w.title, spec_json, pos_json, now],
        )?;
    }

    Ok(DashboardLayout {
        id,
        profile_id: profile_id.to_string(),
        name: STARTER_NAME.to_string(),
        version: next_ver,
        is_active: activate,
        is_starter: true,
        created_at: now,
    })
}

fn next_version_for_name(conn: &Connection, profile_id: &str, name: &str) -> AppResult<u32> {
    let max_v: Option<u32> = conn
        .query_row(
            "SELECT MAX(version) FROM dashboard_layouts WHERE profile_id = ?1 AND name = ?2",
            params![profile_id, name],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(max_v.unwrap_or(0) + 1)
}

fn row_to_layout(r: &rusqlite::Row<'_>) -> rusqlite::Result<DashboardLayout> {
    Ok(DashboardLayout {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        name: r.get(2)?,
        version: r.get(3)?,
        is_active: r.get::<_, i64>(4)? != 0,
        is_starter: r.get::<_, i64>(5)? != 0,
        created_at: r.get(6)?,
    })
}

pub fn get_active_layout(conn: &Connection, profile_id: &str) -> AppResult<Option<DashboardLayout>> {
    Ok(conn
        .query_row(
            "SELECT id, profile_id, name, version, is_active, is_starter, created_at
             FROM dashboard_layouts WHERE profile_id = ?1 AND is_active = 1
             ORDER BY version DESC, created_at DESC LIMIT 1",
            [profile_id],
            row_to_layout,
        )
        .optional()?)
}

pub fn get_layout(conn: &Connection, profile_id: &str, layout_id: &str) -> AppResult<DashboardLayout> {
    conn.query_row(
        "SELECT id, profile_id, name, version, is_active, is_starter, created_at
         FROM dashboard_layouts WHERE profile_id = ?1 AND id = ?2",
        params![profile_id, layout_id],
        row_to_layout,
    )
    .optional()?
    .ok_or_else(|| AppError::Validation(format!("dashboard layout \"{layout_id}\" not found")))
}

pub fn list_layouts(conn: &Connection, profile_id: &str) -> AppResult<Vec<DashboardLayout>> {
    let mut stmt = conn.prepare(
        "SELECT id, profile_id, name, version, is_active, is_starter, created_at
         FROM dashboard_layouts WHERE profile_id = ?1
         ORDER BY is_active DESC, name ASC, version DESC, created_at DESC",
    )?;
    let rows = stmt
        .query_map([profile_id], row_to_layout)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn list_widgets(conn: &Connection, layout_id: &str) -> AppResult<Vec<DashboardWidget>> {
    let mut stmt = conn.prepare(
        "SELECT id, layout_id, title, spec_json, position_json, source_prompt, created_at
         FROM dashboard_widgets WHERE layout_id = ?1
         ORDER BY created_at ASC",
    )?;
    let raw_rows = stmt
        .query_map([layout_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, String>(6)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut widgets = Vec::with_capacity(raw_rows.len());
    for (id, layout_id, title, spec_json, pos_json, source_prompt, created_at) in raw_rows {
        let spec: WidgetSpec = serde_json::from_str(&spec_json)
            .map_err(|e| AppError::Other(format!("corrupt widget spec for {id}: {e}")))?;
        let position: WidgetPosition = serde_json::from_str(&pos_json)
            .unwrap_or(WidgetPosition { x: 0, y: 0, w: 6, h: 2 });
        widgets.push(DashboardWidget {
            id,
            layout_id,
            title,
            spec,
            position,
            source_prompt,
            created_at,
        });
    }

    widgets.sort_by_key(|w| (w.position.y, w.position.x));
    Ok(widgets)
}

pub fn evaluate_layout(
    conn: &Connection,
    profile_id: &str,
    layout_id: &str,
    window_override_days: Option<u32>,
) -> AppResult<Vec<EvaluatedWidget>> {
    let widgets = list_widgets(conn, layout_id)?;
    let mut out = Vec::with_capacity(widgets.len());
    for widget in widgets {
        match execute_widget(conn, profile_id, &widget.spec, window_override_days) {
            Ok(data) => out.push(EvaluatedWidget {
                widget,
                data: Some(data),
                error: None,
            }),
            Err(e) => out.push(EvaluatedWidget {
                widget,
                data: None,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(out)
}

pub fn switch_layout(conn: &Connection, profile_id: &str, layout_id: &str) -> AppResult<DashboardLayout> {
    let mut layout = get_layout(conn, profile_id, layout_id)?;
    conn.execute(
        "UPDATE dashboard_layouts SET is_active = 0 WHERE profile_id = ?1",
        [profile_id],
    )?;
    conn.execute(
        "UPDATE dashboard_layouts SET is_active = 1 WHERE profile_id = ?1 AND id = ?2",
        params![profile_id, layout_id],
    )?;
    layout.is_active = true;
    Ok(layout)
}

pub fn rename_layout(conn: &Connection, profile_id: &str, layout_id: &str, new_name: &str) -> AppResult<DashboardLayout> {
    let name = new_name.trim();
    if name.is_empty() || name.chars().count() > 80 {
        return Err(AppError::Validation("layout name must be 1..=80 characters".into()));
    }
    let mut layout = get_layout(conn, profile_id, layout_id)?;
    conn.execute(
        "UPDATE dashboard_layouts SET name = ?1 WHERE profile_id = ?2 AND id = ?3",
        params![name, profile_id, layout_id],
    )?;
    layout.name = name.to_string();
    Ok(layout)
}

/// Clones an existing layout as a new layout name or as an incremented immutable version (ANL-21).
pub fn clone_layout(
    conn: &Connection,
    profile_id: &str,
    source_layout_id: &str,
    target_name: Option<&str>,
    activate: bool,
) -> AppResult<DashboardLayout> {
    let source = get_layout(conn, profile_id, source_layout_id)?;
    let widgets = list_widgets(conn, &source.id)?;
    let name = target_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&source.name);
    if name.chars().count() > 80 {
        return Err(AppError::Validation("layout name must be <= 80 characters".into()));
    }
    let version = next_version_for_name(conn, profile_id, name)?;
    let id = new_id();
    let now = now_rfc3339();

    if activate {
        conn.execute(
            "UPDATE dashboard_layouts SET is_active = 0 WHERE profile_id = ?1",
            [profile_id],
        )?;
    }
    conn.execute(
        "INSERT INTO dashboard_layouts (id, profile_id, name, version, is_active, is_starter, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6)",
        params![id, profile_id, name, version, if activate { 1 } else { 0 }, now],
    )?;

    for w in widgets {
        let spec_json = serde_json::to_string(&w.spec)
            .map_err(|e| AppError::Other(format!("serializing widget spec: {e}")))?;
        let pos_json = serde_json::to_string(&w.position.normalized())
            .map_err(|e| AppError::Other(format!("serializing widget position: {e}")))?;
        conn.execute(
            "INSERT INTO dashboard_widgets (id, layout_id, title, spec_json, position_json, source_prompt, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![new_id(), id, w.title, spec_json, pos_json, w.source_prompt, now],
        )?;
    }

    Ok(DashboardLayout {
        id,
        profile_id: profile_id.to_string(),
        name: name.to_string(),
        version,
        is_active: activate,
        is_starter: false,
        created_at: now,
    })
}

pub fn delete_layout(conn: &Connection, profile_id: &str, layout_id: &str) -> AppResult<DashboardLayout> {
    let target = get_layout(conn, profile_id, layout_id)?;
    let all = list_layouts(conn, profile_id)?;
    if all.len() <= 1 {
        return Err(AppError::Validation("cannot delete the only remaining dashboard layout".into()));
    }
    conn.execute("DELETE FROM dashboard_widgets WHERE layout_id = ?1", [&target.id])?;
    conn.execute(
        "DELETE FROM dashboard_layouts WHERE profile_id = ?1 AND id = ?2",
        params![profile_id, target.id],
    )?;
    if target.is_active {
        let fallback = all.into_iter().find(|l| l.id != target.id).expect("at least one remains");
        return switch_layout(conn, profile_id, &fallback.id);
    }
    ensure_active_layout(conn, profile_id)
}

/// Resets to an exact copy of the starter layout as a fresh active version while keeping prior versions restorable (ANL-21).
pub fn reset_to_starter(conn: &Connection, profile_id: &str) -> AppResult<DashboardLayout> {
    create_starter_layout(conn, profile_id, true)
}

pub fn add_widget(
    conn: &Connection,
    profile_id: &str,
    layout_id: &str,
    spec: WidgetSpec,
    position: Option<WidgetPosition>,
    source_prompt: Option<String>,
) -> AppResult<DashboardWidget> {
    let _layout = get_layout(conn, profile_id, layout_id)?;
    validate_spec(&spec)?;

    let pos = match position {
        Some(p) => p.normalized(),
        None => {
            let existing = list_widgets(conn, layout_id)?;
            let max_y = existing.iter().map(|w| w.position.y + w.position.h).max().unwrap_or(0);
            WidgetPosition { x: 0, y: max_y, w: 6, h: 2 }
        }
    };

    let id = new_id();
    let now = now_rfc3339();
    let spec_json = serde_json::to_string(&spec)
        .map_err(|e| AppError::Other(format!("serializing widget spec: {e}")))?;
    let pos_json = serde_json::to_string(&pos)
        .map_err(|e| AppError::Other(format!("serializing widget position: {e}")))?;

    conn.execute(
        "INSERT INTO dashboard_widgets (id, layout_id, title, spec_json, position_json, source_prompt, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, layout_id, spec.title, spec_json, pos_json, source_prompt, now],
    )?;

    Ok(DashboardWidget {
        id,
        layout_id: layout_id.to_string(),
        title: spec.title.clone(),
        spec,
        position: pos,
        source_prompt,
        created_at: now,
    })
}

pub fn update_widget(
    conn: &Connection,
    profile_id: &str,
    layout_id: &str,
    widget_id: &str,
    spec: WidgetSpec,
    position: Option<WidgetPosition>,
    source_prompt: Option<String>,
) -> AppResult<DashboardWidget> {
    let _layout = get_layout(conn, profile_id, layout_id)?;
    validate_spec(&spec)?;

    let existing = list_widgets(conn, layout_id)?
        .into_iter()
        .find(|w| w.id == widget_id)
        .ok_or_else(|| AppError::Validation(format!("widget \"{widget_id}\" not found in layout")))?;

    let pos = position.map(|p| p.normalized()).unwrap_or(existing.position);
    let spec_json = serde_json::to_string(&spec)
        .map_err(|e| AppError::Other(format!("serializing widget spec: {e}")))?;
    let pos_json = serde_json::to_string(&pos)
        .map_err(|e| AppError::Other(format!("serializing widget position: {e}")))?;
    let prompt_to_store = source_prompt.or(existing.source_prompt);

    conn.execute(
        "UPDATE dashboard_widgets
         SET title = ?1, spec_json = ?2, position_json = ?3, source_prompt = ?4
         WHERE id = ?5 AND layout_id = ?6",
        params![spec.title, spec_json, pos_json, prompt_to_store, widget_id, layout_id],
    )?;

    Ok(DashboardWidget {
        id: existing.id,
        layout_id: existing.layout_id,
        title: spec.title.clone(),
        spec,
        position: pos,
        source_prompt: prompt_to_store,
        created_at: existing.created_at,
    })
}

pub fn remove_widget(conn: &Connection, profile_id: &str, layout_id: &str, widget_id: &str) -> AppResult<()> {
    let _layout = get_layout(conn, profile_id, layout_id)?;
    let changed = conn.execute(
        "DELETE FROM dashboard_widgets WHERE id = ?1 AND layout_id = ?2",
        params![widget_id, layout_id],
    )?;
    if changed == 0 {
        return Err(AppError::Validation(format!("widget \"{widget_id}\" not found")));
    }
    Ok(())
}

pub fn update_widget_positions(
    conn: &Connection,
    profile_id: &str,
    layout_id: &str,
    updates: &[WidgetPositionUpdate],
) -> AppResult<Vec<DashboardWidget>> {
    let _layout = get_layout(conn, profile_id, layout_id)?;
    for u in updates {
        let pos_json = serde_json::to_string(&u.position.normalized())
            .map_err(|e| AppError::Other(format!("serializing widget position: {e}")))?;
        conn.execute(
            "UPDATE dashboard_widgets SET position_json = ?1 WHERE id = ?2 AND layout_id = ?3",
            params![pos_json, u.widget_id, layout_id],
        )?;
    }
    list_widgets(conn, layout_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::spec::{AggFn, AllowedView, ChartType, WidgetMetric, WidgetOptions};
    use crate::storage::{open, repo};
    use tempfile::tempdir;

    #[test]
    fn seeds_starter_layout_and_supports_widget_crud_versioning_and_reset() {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/tmp/p").unwrap();

        let starter = ensure_active_layout(&conn, &pid).unwrap();
        assert!(starter.is_starter);
        assert_eq!(starter.version, 1);
        let starter_list = list_widgets(&conn, &starter.id).unwrap();
        let starter_count = starter_list.len();
        assert_eq!(starter_count, starter_widgets().len());
        assert!(
            starter_list.iter().any(|w| w.title == "OGC Published" && w.spec.chart_type == ChartType::Kpi),
            "starter layout must include OGC Published KPI card"
        );

        // Save a new version before editing (ANL-21)
        let v2 = clone_layout(&conn, &pid, &starter.id, None, true).unwrap();
        assert_eq!(v2.version, 2);
        assert!(v2.is_active);

        // Add custom widget (ANL-15)
        let added = add_widget(
            &conn,
            &pid,
            &v2.id,
            WidgetSpec {
                title: "Custom Model Chart".into(),
                view: AllowedView::VAnalyticsByModel,
                chart_type: ChartType::Bar,
                group_by: vec!["model_family".into()],
                metrics: vec![WidgetMetric {
                    column: "impressions".into(),
                    agg: AggFn::Sum,
                    alias: "impressions".into(),
                }],
                select_columns: vec![],
                filters: vec![],
                sort: None,
                limit: 10,
                window_days: 7,
                options: WidgetOptions::default(),
            },
            Some(WidgetPosition { x: 0, y: 12, w: 6, h: 2 }),
            Some("impressions by model".into()),
        )
        .unwrap();
        assert_eq!(list_widgets(&conn, &v2.id).unwrap().len(), starter_count + 1);

        // Resize / reposition
        let updated = update_widget_positions(
            &conn,
            &pid,
            &v2.id,
            &[WidgetPositionUpdate {
                widget_id: added.id.clone(),
                position: WidgetPosition { x: 0, y: 12, w: 12, h: 3 },
            }],
        )
        .unwrap();
        assert_eq!(updated.iter().find(|w| w.id == added.id).unwrap().position.w, 12);

        // Rename & duplicate
        let renamed = rename_layout(&conn, &pid, &v2.id, "Weekly Focus").unwrap();
        assert_eq!(renamed.name, "Weekly Focus");
        let dup = clone_layout(&conn, &pid, &renamed.id, Some("Weekly Focus Copy"), false).unwrap();
        assert_eq!(dup.name, "Weekly Focus Copy");

        // Reset to starter restores exact starter widgets while keeping prior version restorable
        let reset = reset_to_starter(&conn, &pid).unwrap();
        assert!(reset.is_starter);
        assert_eq!(list_widgets(&conn, &reset.id).unwrap().len(), starter_count);
        let restored = switch_layout(&conn, &pid, &v2.id).unwrap();
        assert_eq!(restored.id, v2.id);
        assert_eq!(list_widgets(&conn, &restored.id).unwrap().len(), starter_count + 1);

        // Verify layout and resized widget positions persist across closing and reopening SQLite
        drop(conn);
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let persisted_layout = ensure_active_layout(&conn, &pid).unwrap();
        assert_eq!(persisted_layout.id, v2.id);
        let persisted_widgets = list_widgets(&conn, &persisted_layout.id).unwrap();
        let persisted_added = persisted_widgets.iter().find(|w| w.id == added.id).unwrap();
        assert_eq!(persisted_added.position, WidgetPosition { x: 0, y: 12, w: 12, h: 3 });

        // Remove widget and delete duplicate layout
        remove_widget(&conn, &pid, &v2.id, &added.id).unwrap();
        assert_eq!(list_widgets(&conn, &v2.id).unwrap().len(), starter_count);
        delete_layout(&conn, &pid, &dup.id).unwrap();
        assert!(list_layouts(&conn, &pid).unwrap().iter().all(|l| l.id != dup.id));
    }
}
