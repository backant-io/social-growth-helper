//! Declarative widget specification (ANL-18, ANL-20, ANL-22).
//!
//! Generated widget specs from the `dashboard_builder` model are untrusted input (SEC-07 spirit):
//! every field is strictly validated against allowlisted views, columns, operators, and bounded limits.

use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};

pub const MAX_WIDGET_ROWS: u32 = 500;
pub const DEFAULT_WIDGET_ROWS: u32 = 50;
pub const MAX_WINDOW_DAYS: u32 = 365;
pub const DEFAULT_WINDOW_DAYS: u32 = 30;
pub const MAX_FILTERS: usize = 8;
pub const MAX_METRICS: usize = 6;
pub const MAX_TITLE_CHARS: usize = 120;

/// Allowlisted read-only SQL views that widgets may read from (ANL-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedView {
    VAnalyticsAccountOverview,
    VAnalyticsPostPerformance,
    VAnalyticsTimeSeries,
    VAnalyticsByModel,
    VAnalyticsTimingTier,
    VAnalyticsByAction,
    VAnalyticsByFormat,
    VAnalyticsWinnersFlops,
    VAnalyticsSnapshotHistory,
}

impl AllowedView {
    pub const ALL: &'static [AllowedView] = &[
        AllowedView::VAnalyticsAccountOverview,
        AllowedView::VAnalyticsPostPerformance,
        AllowedView::VAnalyticsTimeSeries,
        AllowedView::VAnalyticsByModel,
        AllowedView::VAnalyticsTimingTier,
        AllowedView::VAnalyticsByAction,
        AllowedView::VAnalyticsByFormat,
        AllowedView::VAnalyticsWinnersFlops,
        AllowedView::VAnalyticsSnapshotHistory,
    ];

    pub fn sql_name(self) -> &'static str {
        match self {
            AllowedView::VAnalyticsAccountOverview => "v_analytics_account_overview",
            AllowedView::VAnalyticsPostPerformance => "v_analytics_post_performance",
            AllowedView::VAnalyticsTimeSeries => "v_analytics_time_series",
            AllowedView::VAnalyticsByModel => "v_analytics_by_model",
            AllowedView::VAnalyticsTimingTier => "v_analytics_timing_tier",
            AllowedView::VAnalyticsByAction => "v_analytics_by_action",
            AllowedView::VAnalyticsByFormat => "v_analytics_by_format",
            AllowedView::VAnalyticsWinnersFlops => "v_analytics_winners_flops",
            AllowedView::VAnalyticsSnapshotHistory => "v_analytics_snapshot_history",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            AllowedView::VAnalyticsAccountOverview => "Account-level snapshots: impressions, engagement rate, profile visits, followers and delta",
            AllowedView::VAnalyticsPostPerformance => "Latest snapshot per post joined with draft metadata, model, timing, tier, format and external flag",
            AllowedView::VAnalyticsTimeSeries => "Daily and weekly post metrics grouped by day/week, post kind and model family",
            AllowedView::VAnalyticsByModel => "Post metrics attributed to Claude, Grok, External or Other including user edit distance",
            AllowedView::VAnalyticsTimingTier => "Reply impressions and engagement by reply delay bucket and target author follower tier",
            AllowedView::VAnalyticsByAction => "Performance split by post kind (reply/quote/ogc/external) and action bundle (like+reply vs reply-only)",
            AllowedView::VAnalyticsByFormat => "OGC performance by viral format blueprint and archetype",
            AllowedView::VAnalyticsWinnersFlops => "Top 10% winners and bottom 10% flops with reply text, source post context, model and metrics",
            AllowedView::VAnalyticsSnapshotHistory => "Time-series age checkpoints (1h, 6h, 24h, 72h, 7d) per published post",
        }
    }
}

/// Supported chart visualizations: v1 core (ANL-18) plus extended types (ANL-22).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Kpi,
    Line,
    Bar,
    Table,
    StackedBar,
    Heatmap,
    Scatter,
    Funnel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Neq,
    Gt,
    Gte,
    Lt,
    Lte,
    Contains,
    In,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetFilter {
    pub column: String,
    pub op: FilterOp,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AggFn {
    Sum,
    Avg,
    Count,
    Min,
    Max,
    Latest,
}

impl AggFn {
    pub fn sql_expr(self, col: &str) -> String {
        match self {
            AggFn::Sum => format!("COALESCE(SUM({col}), 0)"),
            AggFn::Avg => format!("ROUND(COALESCE(AVG({col}), 0), 2)"),
            AggFn::Count => format!("COUNT({col})"),
            AggFn::Min => format!("MIN({col})"),
            AggFn::Max => format!("MAX({col})"),
            // Latest value in the filtered time window is handled by ordering/subquery or MAX for snapshots;
            // in aggregated queries we use AVG/MAX or single-row ordering cleanly in sandbox.rs.
            AggFn::Latest => format!("MAX({col})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetMetric {
    pub column: String,
    pub agg: AggFn,
    pub alias: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortDir {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetSort {
    pub column: String,
    pub dir: SortDir,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidgetPosition {
    pub x: u32,
    pub y: u32,
    /// Width on a 12-column grid (3, 4, 6, 8, or 12).
    pub w: u32,
    /// Relative row height (1..=4).
    pub h: u32,
}

impl WidgetPosition {
    pub fn normalized(&self) -> Self {
        let w = match self.w {
            0..=3 => 3,
            4..=5 => 4,
            6..=7 => 6,
            8..=10 => 8,
            _ => 12,
        };
        let h = self.h.clamp(1, 4);
        Self {
            x: self.x.min(12u32.saturating_sub(w)),
            y: self.y.min(200),
            w,
            h,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WidgetOptions {
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub x_field: Option<String>,
    #[serde(default)]
    pub y_field: Option<String>,
    #[serde(default)]
    pub series_field: Option<String>,
    #[serde(default)]
    pub value_field: Option<String>,
    /// Display format for KPI / values: "number" | "percent" | "signed" | "decimal".
    #[serde(default)]
    pub format: Option<String>,
    /// When true, renders the Winners & Flops gallery card view for tables.
    #[serde(default)]
    pub gallery_mode: bool,
    /// Optional explicit columns to display in table mode.
    #[serde(default)]
    pub columns: Vec<String>,
}

/// Complete declarative widget specification (ANL-18).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetSpec {
    pub title: String,
    pub view: AllowedView,
    pub chart_type: ChartType,
    #[serde(default)]
    pub group_by: Vec<String>,
    #[serde(default)]
    pub metrics: Vec<WidgetMetric>,
    /// Columns selected directly when `group_by` and `metrics` are empty (e.g. raw table/gallery).
    #[serde(default)]
    pub select_columns: Vec<String>,
    #[serde(default)]
    pub filters: Vec<WidgetFilter>,
    #[serde(default)]
    pub sort: Option<WidgetSort>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default = "default_window_days")]
    pub window_days: u32,
    #[serde(default)]
    pub options: WidgetOptions,
}

fn default_limit() -> u32 {
    DEFAULT_WIDGET_ROWS
}

fn default_window_days() -> u32 {
    DEFAULT_WINDOW_DAYS
}

/// Rejects any raw JSON payload that attempts writes, arbitrary code, or non-allowlisted views (ANL-20).
pub fn parse_untrusted_spec_json(raw: &str) -> AppResult<WidgetSpec> {
    reject_unsafe_payload_text(raw)?;
    let val: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| AppError::Validation(format!("invalid widget spec JSON: {e}")))?;
    if let Some(obj) = val.as_object() {
        for forbidden in ["sql", "query", "raw_sql", "script", "code", "js", "eval", "url", "endpoint", "fetch"] {
            if obj.contains_key(forbidden) {
                return Err(AppError::Validation(format!(
                    "sandbox rejected field \"{forbidden}\": widgets may only read allowlisted views with declarative specs"
                )));
            }
        }
        if let Some(view_name) = obj.get("view").and_then(|v| v.as_str()) {
            let allowed = AllowedView::ALL.iter().any(|v| v.sql_name() == view_name);
            if !allowed {
                return Err(AppError::Validation(format!(
                    "sandbox rejected view \"{view_name}\": only allowlisted read-only analytics views are permitted"
                )));
            }
        }
    }
    let spec: WidgetSpec =
        serde_json::from_value(val).map_err(|e| AppError::Validation(format!("invalid widget spec: {e}")))?;
    validate_spec(&spec)?;
    Ok(spec)
}

/// Checks text for obvious code/SQL-write injection tokens before parsing/execution (ANL-20).
pub fn reject_unsafe_payload_text(text: &str) -> AppResult<()> {
    let lower = text.to_lowercase();
    let forbidden_tokens = [
        "<script",
        "javascript:",
        "eval(",
        "new function(",
        "fetch(",
        "xmlhttprequest",
        "drop table",
        "delete from",
        "insert into",
        "update ",
        "alter table",
        "attach database",
        "pragma ",
        "sqlite_master",
        "provider_accounts",
        "app_settings",
    ];
    for tok in forbidden_tokens {
        if lower.contains(tok) {
            return Err(AppError::Validation(format!(
                "sandbox blocked forbidden token \"{tok}\" — widgets are read-only and restricted to analytics views"
            )));
        }
    }
    Ok(())
}

pub fn is_safe_identifier(ident: &str) -> bool {
    !ident.is_empty()
        && ident.len() <= 64
        && ident
            .chars()
            .next()
            .map(|c| c.is_ascii_alphabetic() || c == '_')
            .unwrap_or(false)
        && ident.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

pub fn validate_spec(spec: &WidgetSpec) -> AppResult<()> {
    let title = spec.title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(AppError::Validation(format!(
            "widget title must be between 1 and {MAX_TITLE_CHARS} characters"
        )));
    }
    reject_unsafe_payload_text(title)?;
    if let Some(sub) = &spec.options.subtitle {
        reject_unsafe_payload_text(sub)?;
    }
    if spec.limit == 0 || spec.limit > MAX_WIDGET_ROWS {
        return Err(AppError::Validation(format!(
            "widget row limit must be between 1 and {MAX_WIDGET_ROWS}"
        )));
    }
    if spec.window_days == 0 || spec.window_days > MAX_WINDOW_DAYS {
        return Err(AppError::Validation(format!(
            "widget window_days must be between 1 and {MAX_WINDOW_DAYS}"
        )));
    }
    if spec.filters.len() > MAX_FILTERS {
        return Err(AppError::Validation(format!(
            "widget may have at most {MAX_FILTERS} filters"
        )));
    }
    if spec.metrics.len() > MAX_METRICS {
        return Err(AppError::Validation(format!(
            "widget may have at most {MAX_METRICS} metrics"
        )));
    }

    let allowed_cols = super::views::view_columns(spec.view);
    let check_col = |col: &str, ctx: &str| -> AppResult<()> {
        if !is_safe_identifier(col) || !allowed_cols.contains(&col) {
            return Err(AppError::Validation(format!(
                "sandbox rejected column \"{col}\" in {ctx} for view \"{}\"",
                spec.view.sql_name()
            )));
        }
        Ok(())
    };

    for g in &spec.group_by {
        check_col(g, "group_by")?;
    }
    for s in &spec.select_columns {
        check_col(s, "select_columns")?;
    }
    let mut aliases = Vec::new();
    for m in &spec.metrics {
        check_col(&m.column, "metrics")?;
        if !is_safe_identifier(&m.alias) {
            return Err(AppError::Validation(format!(
                "invalid metric alias \"{}\": use letters, digits, underscores only",
                m.alias
            )));
        }
        aliases.push(m.alias.as_str());
    }
    for f in &spec.filters {
        check_col(&f.column, "filters")?;
        if f.op == FilterOp::In {
            let arr = f
                .value
                .as_array()
                .ok_or_else(|| AppError::Validation("filter with op 'in' requires an array value".into()))?;
            if arr.is_empty() || arr.len() > 20 {
                return Err(AppError::Validation("filter 'in' list must contain 1..=20 values".into()));
            }
        }
    }
    if let Some(sort) = &spec.sort {
        let valid_sort = (is_safe_identifier(&sort.column) && allowed_cols.contains(&sort.column.as_str()))
            || aliases.contains(&sort.column.as_str());
        if !valid_sort {
            return Err(AppError::Validation(format!(
                "sandbox rejected sort column \"{}\" for view \"{}\"",
                sort.column,
                spec.view.sql_name()
            )));
        }
    }
    if spec.group_by.is_empty() && spec.metrics.is_empty() && spec.select_columns.is_empty() {
        return Err(AppError::Validation(
            "widget spec must specify at least one metric or select_columns entry".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_allowlisted_table_and_raw_sql_fields() {
        let bad_view = r#"{"title":"Leak","view":"provider_accounts","chart_type":"table","select_columns":["id"]}"#;
        assert!(parse_untrusted_spec_json(bad_view).is_err());

        let bad_sql = r#"{"title":"Drop","view":"v_analytics_by_model","chart_type":"table","sql":"DROP TABLE profiles"}"#;
        assert!(parse_untrusted_spec_json(bad_sql).is_err());

        let bad_code = r#"{"title":"<script>alert(1)</script>","view":"v_analytics_by_model","chart_type":"table","select_columns":["model_family"]}"#;
        assert!(parse_untrusted_spec_json(bad_code).is_err());
    }

    #[test]
    fn rejects_unknown_column_and_excessive_limits() {
        let bad_col = r#"{
            "title": "Bad col",
            "view": "v_analytics_by_model",
            "chart_type": "bar",
            "group_by": ["nonexistent_col"],
            "metrics": [{"column": "impressions", "agg": "avg", "alias": "avg_imp"}]
        }"#;
        assert!(parse_untrusted_spec_json(bad_col).is_err());

        let bad_limit = r#"{
            "title": "Too many",
            "view": "v_analytics_by_model",
            "chart_type": "table",
            "select_columns": ["model_family"],
            "limit": 5000
        }"#;
        assert!(parse_untrusted_spec_json(bad_limit).is_err());
    }

    #[test]
    fn accepts_valid_declarative_spec() {
        let ok = r#"{
            "title": "Impressions by Model",
            "view": "v_analytics_by_model",
            "chart_type": "bar",
            "group_by": ["model_family"],
            "metrics": [{"column": "impressions", "agg": "avg", "alias": "avg_impressions"}],
            "sort": {"column": "avg_impressions", "dir": "desc"},
            "limit": 10,
            "window_days": 30
        }"#;
        let spec = parse_untrusted_spec_json(ok).unwrap();
        assert_eq!(spec.chart_type, ChartType::Bar);
        assert_eq!(spec.view, AllowedView::VAnalyticsByModel);
    }
}
