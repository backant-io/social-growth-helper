//! Allowlisted read-only SQLite analytics views (ANL-08, ANL-09, ANL-10, ANL-11, ANL-20).
//!
//! Defined by migration `0004_phase4_analytics_views.sql`; `ensure_views` only re-applies the DDL when a view is
//! missing (e.g. a connection that bypassed migrations), so dashboard loads never write to the database.

use super::spec::AllowedView;
use crate::error::AppResult;
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ViewSchemaInfo {
    pub view: AllowedView,
    pub sql_name: &'static str,
    pub description: &'static str,
    pub columns: &'static [&'static str],
    pub time_column: &'static str,
}

/// Returns the allowlisted columns exposed by each read-only analytics view.
pub fn view_columns(view: AllowedView) -> &'static [&'static str] {
    match view {
        AllowedView::VAnalyticsAccountOverview => &[
            "captured_at",
            "day",
            "impressions",
            "engagement_rate",
            "profile_visits",
            "followers_total",
            "follower_delta",
        ],
        AllowedView::VAnalyticsPostPerformance => &[
            "platform_post_id",
            "draft_kind",
            "draft_id",
            "captured_at",
            "published_at",
            "day",
            "week",
            "age_hours",
            "post_text",
            "context_text",
            "context_author",
            "model_used",
            "model_family",
            "actions_bundle",
            "viral_format_name",
            "edit_distance",
            "reply_delay_minutes",
            "timing_bucket",
            "author_followers",
            "follower_tier",
            "impressions",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "profile_visits",
            "link_clicks",
            "engagement_rate",
            "is_external",
        ],
        AllowedView::VAnalyticsTimeSeries => &[
            "captured_at",
            "day",
            "week",
            "draft_kind",
            "model_family",
            "impressions",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "engagement_rate",
            "edit_distance",
            "platform_post_id",
        ],
        AllowedView::VAnalyticsByModel => &[
            "captured_at",
            "day",
            "week",
            "model_family",
            "model_used",
            "draft_kind",
            "impressions",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "engagement_rate",
            "edit_distance",
            "platform_post_id",
        ],
        AllowedView::VAnalyticsTimingTier => &[
            "captured_at",
            "day",
            "timing_bucket",
            "follower_tier",
            "reply_delay_minutes",
            "author_followers",
            "draft_kind",
            "model_family",
            "impressions",
            "engagement_rate",
            "likes",
            "replies",
            "platform_post_id",
        ],
        AllowedView::VAnalyticsByAction => &[
            "captured_at",
            "day",
            "draft_kind",
            "actions_bundle",
            "model_family",
            "impressions",
            "engagement_rate",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "edit_distance",
            "platform_post_id",
        ],
        AllowedView::VAnalyticsByFormat => &[
            "captured_at",
            "day",
            "viral_format_id",
            "viral_format_name",
            "archetype",
            "model_family",
            "impressions",
            "engagement_rate",
            "likes",
            "replies",
            "bookmarks",
            "observed_win_rate",
            "personal_win_rate",
            "platform_post_id",
        ],
        AllowedView::VAnalyticsWinnersFlops => &[
            "captured_at",
            "platform_post_id",
            "tier",
            "rank_desc",
            "total_posts",
            "draft_kind",
            "model_family",
            "post_text",
            "context_text",
            "context_author",
            "impressions",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "engagement_rate",
            "edit_distance",
            "is_external",
        ],
        AllowedView::VAnalyticsSnapshotHistory => &[
            "captured_at",
            "day",
            "platform_post_id",
            "draft_kind",
            "age_hours",
            "impressions",
            "likes",
            "replies",
            "reposts",
            "bookmarks",
            "engagement_rate",
        ],
    }
}

pub fn view_time_column(_view: AllowedView) -> &'static str {
    "captured_at"
}

pub fn list_view_schemas() -> Vec<ViewSchemaInfo> {
    AllowedView::ALL
        .iter()
        .map(|&view| ViewSchemaInfo {
            view,
            sql_name: view.sql_name(),
            description: view.description(),
            columns: view_columns(view),
            time_column: view_time_column(view),
        })
        .collect()
}

/// Creates the allowlisted read-only SQLite views when any is missing; a no-op once migration 0004 has run.
pub fn ensure_views(conn: &Connection) -> AppResult<()> {
    let expected = list_view_schemas();
    let present: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'view' AND name LIKE 'v\\_analytics\\_%' ESCAPE '\\'",
        [],
        |r| r.get(0),
    )?;
    if (present as usize) < expected.len() {
        conn.execute_batch(VIEWS_SQL)?;
    }
    Ok(())
}

const VIEWS_SQL: &str = include_str!("../storage/migrations/0004_phase4_analytics_views.sql");

