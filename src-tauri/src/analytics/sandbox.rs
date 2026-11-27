//! Sandboxed read-only SQL compiler & executor for declarative widget specs (ANL-20).
//!
//! Guarantees:
//! - Only allowlisted `v_analytics_*` views can be queried.
//! - Every identifier is checked against the view's explicit column allowlist (never interpolated untrusted SQL).
//! - Always scopes by `profile_id` and `window_days` (<= 365).
//! - Always bounds result rows (`LIMIT <= 500`).
//! - Executes under `PRAGMA query_only = ON` so SQLite rejects any mutation even in theory.

use super::spec::{validate_spec, AllowedView, FilterOp, SortDir, WidgetSpec};
use super::views::view_time_column;
use crate::error::{AppError, AppResult};
use crate::util::format_unix;
use rusqlite::{types::Value as SqlValue, types::ValueRef, Connection};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WidgetQueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<BTreeMap<String, serde_json::Value>>,
    pub row_count: usize,
    pub truncated: bool,
    pub window_days: u32,
    pub executed_view: String,
}

/// RAII guard that enforces `PRAGMA query_only = ON` for the duration of a widget read.
struct QueryOnlyGuard<'a> {
    conn: &'a Connection,
}

impl<'a> QueryOnlyGuard<'a> {
    fn enter(conn: &'a Connection) -> AppResult<Self> {
        conn.pragma_update(None, "query_only", "ON")?;
        Ok(Self { conn })
    }
}

impl Drop for QueryOnlyGuard<'_> {
    fn drop(&mut self) {
        let _ = self.conn.pragma_update(None, "query_only", "OFF");
    }
}

/// Maps a UI window selection (`window_days`) to the canonical X Creator Analytics period (`7` or `30`).
pub fn canonical_period_days(window_days: u32) -> i64 {
    if window_days <= 7 { 7 } else { 30 }
}

fn cutoff_rfc3339(window_days: u32) -> String {
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let cutoff = now_secs - (window_days as i64) * 86_400;
    format_unix(cutoff)
}

fn json_to_sql(val: &serde_json::Value) -> AppResult<SqlValue> {
    match val {
        serde_json::Value::Null => Ok(SqlValue::Null),
        serde_json::Value::Bool(b) => Ok(SqlValue::Integer(if *b { 1 } else { 0 })),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(SqlValue::Integer(i))
            } else if let Some(f) = n.as_f64() {
                Ok(SqlValue::Real(f))
            } else {
                Err(AppError::Validation("unsupported numeric filter value".into()))
            }
        }
        serde_json::Value::String(s) => Ok(SqlValue::Text(s.clone())),
        _ => Err(AppError::Validation("filter value must be scalar (string, number, boolean)".into())),
    }
}

fn sql_ref_to_json(val: ValueRef<'_>) -> serde_json::Value {
    match val {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => serde_json::Value::from(i),
        ValueRef::Real(f) => serde_json::Number::from_f64(f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        ValueRef::Text(bytes) => serde_json::Value::String(String::from_utf8_lossy(bytes).into_owned()),
        ValueRef::Blob(_) => serde_json::Value::Null,
    }
}

/// Compiles and runs a validated [`WidgetSpec`] strictly inside the read-only analytics sandbox.
pub fn execute_widget(
    conn: &Connection,
    profile_id: &str,
    spec: &WidgetSpec,
    window_override_days: Option<u32>,
) -> AppResult<WidgetQueryResult> {
    validate_spec(spec)?;
    let window_days = window_override_days
        .unwrap_or(spec.window_days)
        .clamp(1, super::spec::MAX_WINDOW_DAYS);

    // Ensure views exist before flipping to query_only = ON.
    super::views::ensure_views(conn)?;

    let mut select_parts: Vec<String> = Vec::new();
    let mut output_columns: Vec<String> = Vec::new();

    if !spec.group_by.is_empty() || !spec.metrics.is_empty() {
        for g in &spec.group_by {
            select_parts.push(g.clone());
            output_columns.push(g.clone());
        }
        for m in &spec.metrics {
            select_parts.push(format!("{} AS {}", m.agg.sql_expr(&m.column), m.alias));
            output_columns.push(m.alias.clone());
        }
    } else {
        for c in &spec.select_columns {
            select_parts.push(c.clone());
            output_columns.push(c.clone());
        }
    }

    let (mut where_parts, mut params) = if spec.view == AllowedView::VAnalyticsAccountOverview {
        (
            vec!["profile_id = ?".to_string(), "period_days = ?".to_string()],
            vec![
                SqlValue::Text(profile_id.to_string()),
                SqlValue::Integer(canonical_period_days(window_days)),
            ],
        )
    } else {
        let time_col = view_time_column(spec.view);
        let cutoff = cutoff_rfc3339(window_days);
        (
            vec!["profile_id = ?".to_string(), format!("{time_col} >= ?")],
            vec![
                SqlValue::Text(profile_id.to_string()),
                SqlValue::Text(cutoff),
            ],
        )
    };

    for f in &spec.filters {
        match f.op {
            FilterOp::Eq => {
                where_parts.push(format!("{} = ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Neq => {
                where_parts.push(format!("{} != ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Gt => {
                where_parts.push(format!("{} > ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Gte => {
                where_parts.push(format!("{} >= ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Lt => {
                where_parts.push(format!("{} < ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Lte => {
                where_parts.push(format!("{} <= ?", f.column));
                params.push(json_to_sql(&f.value)?);
            }
            FilterOp::Contains => {
                let needle = f
                    .value
                    .as_str()
                    .ok_or_else(|| AppError::Validation("contains filter requires a string value".into()))?;
                where_parts.push(format!("lower({}) LIKE lower(?)", f.column));
                params.push(SqlValue::Text(format!("%{needle}%")));
            }
            FilterOp::In => {
                let arr = f
                    .value
                    .as_array()
                    .ok_or_else(|| AppError::Validation("'in' filter requires an array".into()))?;
                let placeholders = vec!["?"; arr.len()].join(", ");
                where_parts.push(format!("{} IN ({placeholders})", f.column));
                for item in arr {
                    params.push(json_to_sql(item)?);
                }
            }
        }
    }

    let mut sql = format!(
        "SELECT {} FROM {} WHERE {}",
        select_parts.join(", "),
        spec.view.sql_name(),
        where_parts.join(" AND ")
    );

    if !spec.group_by.is_empty() {
        sql.push_str(&format!(" GROUP BY {}", spec.group_by.join(", ")));
    }

    if let Some(sort) = &spec.sort {
        let dir = match sort.dir {
            SortDir::Asc => "ASC",
            SortDir::Desc => "DESC",
        };
        sql.push_str(&format!(" ORDER BY {} {dir}", sort.column));
    }

    // Request limit + 1 to accurately report `truncated`.
    let fetch_limit = (spec.limit as i64) + 1;
    sql.push_str(" LIMIT ?");
    params.push(SqlValue::Integer(fetch_limit));

    let _guard = QueryOnlyGuard::enter(conn)?;
    let mut stmt = conn.prepare(&sql)?;
    let mut rows_out: Vec<BTreeMap<String, serde_json::Value>> = Vec::new();
    let mut q_rows = stmt.query(rusqlite::params_from_iter(params.iter()))?;
    while let Some(row) = q_rows.next()? {
        let mut map = BTreeMap::new();
        for (idx, col) in output_columns.iter().enumerate() {
            let v = row.get_ref(idx)?;
            map.insert(col.clone(), sql_ref_to_json(v));
        }
        rows_out.push(map);
    }

    let truncated = rows_out.len() > spec.limit as usize;
    if truncated {
        rows_out.truncate(spec.limit as usize);
    }

    Ok(WidgetQueryResult {
        columns: output_columns,
        row_count: rows_out.len(),
        rows: rows_out,
        truncated,
        window_days,
        executed_view: spec.view.sql_name().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analytics::spec::{AggFn, AllowedView, ChartType, WidgetFilter, WidgetMetric, WidgetOptions, WidgetSort};
    use crate::storage::{open, repo};
    use crate::util::now_rfc3339;
    use tempfile::tempdir;

    fn seed_db() -> (tempfile::TempDir, Connection, String) {
        let dir = tempdir().unwrap();
        let (conn, _) = open(&dir.path().join("a.db"), &dir.path().join("b")).unwrap();
        let pid = repo::ensure_default_profile(&conn, "/tmp/p").unwrap();
        let now = now_rfc3339();

        conn.execute(
            "INSERT INTO observed_posts (id, profile_id, platform_post_id, author_handle, author_followers, text, feed_source, posted_at, first_seen_at, metrics_at_capture_json)
             VALUES ('op1', ?1, 'src1', 'naval', 250000, 'Build leverage', 'for_you', ?2, ?2, '{}')",
            rusqlite::params![pid, now],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, edit_distance, model_used, style_guide_version, draft_kind, actions_json, status, published_at, published_post_id, created_at)
             VALUES ('rd1', ?1, 'op1', 'Great point', 'Sharp point', 4, 'claude-sonnet-4', 1, 'reply', '[\"like\",\"reply\"]', 'published', ?2, 'p_claude', ?2)",
            rusqlite::params![pid, now],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO reply_drafts (id, profile_id, observed_post_id, initial_text, edited_text, edit_distance, model_used, style_guide_version, draft_kind, actions_json, status, published_at, published_post_id, created_at)
             VALUES ('rd2', ?1, 'op1', 'Funny take', 'Funny take edited', 12, 'grok-4.6', 1, 'reply', '[\"reply\"]', 'published', ?2, 'p_grok', ?2)",
            rusqlite::params![pid, now],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO analytics_snapshots (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours, impressions, likes, replies, reposts, bookmarks, engagement_rate)
             VALUES ('s1', ?1, 'post', 'p_claude', 'reply', 'rd1', ?2, 24.0, 4200, 120, 14, 9, 30, 4.12)",
            rusqlite::params![pid, now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO analytics_snapshots (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours, impressions, likes, replies, reposts, bookmarks, engagement_rate)
             VALUES ('s2', ?1, 'post', 'p_grok', 'reply', 'rd2', ?2, 24.0, 1800, 40, 5, 2, 10, 3.17)",
            rusqlite::params![pid, now],
        )
        .unwrap();
        // External post not in drafts
        conn.execute(
            "INSERT INTO analytics_snapshots (id, profile_id, scope, platform_post_id, draft_kind, draft_id, captured_at, age_hours, impressions, likes, replies, reposts, bookmarks, engagement_rate, raw_json)
             VALUES ('s3', ?1, 'post', 'p_ext', 'external', NULL, ?2, 12.0, 900, 15, 2, 1, 3, 2.33, '{\"text\":\"Manual tweet\"}')",
            rusqlite::params![pid, now],
        )
        .unwrap();

        (dir, conn, pid)
    }

    #[test]
    fn executes_model_breakdown_and_filters_safely() {
        let (_d, conn, pid) = seed_db();
        let spec = WidgetSpec {
            title: "By Model".into(),
            view: AllowedView::VAnalyticsByModel,
            chart_type: ChartType::Bar,
            group_by: vec!["model_family".into()],
            metrics: vec![
                WidgetMetric { column: "impressions".into(), agg: AggFn::Avg, alias: "avg_impressions".into() },
                WidgetMetric { column: "edit_distance".into(), agg: AggFn::Avg, alias: "avg_edit_distance".into() },
            ],
            select_columns: vec![],
            filters: vec![WidgetFilter {
                column: "draft_kind".into(),
                op: FilterOp::Eq,
                value: serde_json::json!("reply"),
            }],
            sort: Some(WidgetSort { column: "avg_impressions".into(), dir: SortDir::Desc }),
            limit: 10,
            window_days: 30,
            options: WidgetOptions::default(),
        };

        let res = execute_widget(&conn, &pid, &spec, None).unwrap();
        assert_eq!(res.row_count, 2);
        assert_eq!(res.rows[0]["model_family"], "Claude");
        assert_eq!(res.rows[0]["avg_impressions"], 4200.0);
        assert_eq!(res.rows[0]["avg_edit_distance"], 4.0);
        assert_eq!(res.rows[1]["model_family"], "Grok");
        assert_eq!(res.rows[1]["avg_edit_distance"], 12.0);
    }

    #[test]
    fn winners_flops_and_external_flags_work_and_connection_remains_writable_after_guard() {
        let (_d, conn, pid) = seed_db();
        let spec = WidgetSpec {
            title: "Winners & Flops".into(),
            view: AllowedView::VAnalyticsWinnersFlops,
            chart_type: ChartType::Table,
            group_by: vec![],
            metrics: vec![],
            select_columns: vec!["platform_post_id".into(), "tier".into(), "is_external".into(), "impressions".into()],
            filters: vec![],
            sort: Some(WidgetSort { column: "impressions".into(), dir: SortDir::Desc }),
            limit: 20,
            window_days: 30,
            options: WidgetOptions::default(),
        };
        let res = execute_widget(&conn, &pid, &spec, None).unwrap();
        assert_eq!(res.row_count, 3);
        assert_eq!(res.rows[0]["tier"], "winner");
        assert_eq!(res.rows[2]["tier"], "flop");
        assert_eq!(res.rows[2]["is_external"], 1);

        // Verify query_only pragma was restored cleanly after execute_widget.
        repo::set_setting(&conn, "after_sandbox_check", "ok").unwrap();
    }
}
