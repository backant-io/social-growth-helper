//! Standard starter dashboard layout built entirely on declarative WidgetSpecs (ANL-07..ANL-11).

use super::spec::{
    AggFn, AllowedView, ChartType, FilterOp, SortDir, WidgetFilter, WidgetMetric, WidgetOptions,
    WidgetPosition, WidgetSort, WidgetSpec,
};

#[derive(Debug, Clone)]
pub struct StarterWidgetDef {
    pub title: &'static str,
    pub spec: WidgetSpec,
    pub position: WidgetPosition,
}

pub fn starter_widgets() -> Vec<StarterWidgetDef> {
    vec![
        // Row 0: North-Star KPI Strip (6 KPIs, 2 rows of 3 or 1 row of 6; using 4-col or 3-col cards -> 4+4+4 and 4+4+4)
        StarterWidgetDef {
            title: "Impressions",
            position: WidgetPosition { x: 0, y: 0, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "Impressions".into(),
                view: AllowedView::VAnalyticsAccountOverview,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "impressions".into(),
                    agg: AggFn::Latest,
                    alias: "total_impressions".into(),
                }],
                select_columns: vec![],
                filters: vec![],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Account impressions for selected period".into()),
                    value_field: Some("total_impressions".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        StarterWidgetDef {
            title: "Engagement Rate",
            position: WidgetPosition { x: 4, y: 0, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "Engagement Rate".into(),
                view: AllowedView::VAnalyticsAccountOverview,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "engagement_rate".into(),
                    agg: AggFn::Latest,
                    alias: "avg_engagement_rate".into(),
                }],
                select_columns: vec![],
                filters: vec![],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Account engagement rate for selected period".into()),
                    value_field: Some("avg_engagement_rate".into()),
                    format: Some("percent".into()),
                    ..Default::default()
                },
            },
        },
        StarterWidgetDef {
            title: "Follower Delta",
            position: WidgetPosition { x: 8, y: 0, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "Follower Delta".into(),
                view: AllowedView::VAnalyticsAccountOverview,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "follower_delta".into(),
                    agg: AggFn::Latest,
                    alias: "net_followers".into(),
                }],
                select_columns: vec![],
                filters: vec![],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Net followers gained in selected period".into()),
                    value_field: Some("net_followers".into()),
                    format: Some("signed".into()),
                    ..Default::default()
                },
            },
        },
        StarterWidgetDef {
            title: "Replies Published",
            position: WidgetPosition { x: 0, y: 1, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "Replies Published".into(),
                view: AllowedView::VAnalyticsPostPerformance,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "platform_post_id".into(),
                    agg: AggFn::Count,
                    alias: "reply_count".into(),
                }],
                select_columns: vec![],
                filters: vec![WidgetFilter {
                    column: "draft_kind".into(),
                    op: FilterOp::In,
                    value: serde_json::json!(["reply", "quote"]),
                }],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Tracked replies & quotes".into()),
                    value_field: Some("reply_count".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        StarterWidgetDef {
            title: "OGC Published",
            position: WidgetPosition { x: 4, y: 1, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "OGC Published".into(),
                view: AllowedView::VAnalyticsPostPerformance,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "platform_post_id".into(),
                    agg: AggFn::Count,
                    alias: "ogc_count".into(),
                }],
                select_columns: vec![],
                filters: vec![WidgetFilter {
                    column: "draft_kind".into(),
                    op: FilterOp::Eq,
                    value: serde_json::json!("ogc"),
                }],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Original posts tracked".into()),
                    value_field: Some("ogc_count".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        StarterWidgetDef {
            title: "Avg Impressions / Reply",
            position: WidgetPosition { x: 8, y: 1, w: 4, h: 1 },
            spec: WidgetSpec {
                title: "Avg Impressions / Reply".into(),
                view: AllowedView::VAnalyticsPostPerformance,
                chart_type: ChartType::Kpi,
                group_by: vec![],
                metrics: vec![WidgetMetric {
                    column: "impressions".into(),
                    agg: AggFn::Avg,
                    alias: "avg_reply_impressions".into(),
                }],
                select_columns: vec![],
                filters: vec![WidgetFilter {
                    column: "draft_kind".into(),
                    op: FilterOp::In,
                    value: serde_json::json!(["reply", "quote"]),
                }],
                sort: None,
                limit: 1,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Average reach per published reply".into()),
                    value_field: Some("avg_reply_impressions".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        // Row 2: Daily Impressions Trend (Line, ANL-07 / ANL-18)
        StarterWidgetDef {
            title: "Impressions Trend by Model",
            position: WidgetPosition { x: 0, y: 2, w: 8, h: 2 },
            spec: WidgetSpec {
                title: "Impressions Trend by Model".into(),
                view: AllowedView::VAnalyticsTimeSeries,
                chart_type: ChartType::Line,
                group_by: vec!["day".into(), "model_family".into()],
                metrics: vec![WidgetMetric {
                    column: "impressions".into(),
                    agg: AggFn::Sum,
                    alias: "impressions".into(),
                }],
                select_columns: vec![],
                filters: vec![],
                sort: Some(WidgetSort { column: "day".into(), dir: SortDir::Asc }),
                limit: 120,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Daily impressions split by model family".into()),
                    x_field: Some("day".into()),
                    y_field: Some("impressions".into()),
                    series_field: Some("model_family".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        // Row 2 right: Breakdown by Model (Bar, ANL-08)
        StarterWidgetDef {
            title: "Claude vs Grok Performance",
            position: WidgetPosition { x: 8, y: 2, w: 4, h: 2 },
            spec: WidgetSpec {
                title: "Claude vs Grok Performance".into(),
                view: AllowedView::VAnalyticsByModel,
                chart_type: ChartType::Bar,
                group_by: vec!["model_family".into()],
                metrics: vec![
                    WidgetMetric {
                        column: "impressions".into(),
                        agg: AggFn::Avg,
                        alias: "avg_impressions".into(),
                    },
                    WidgetMetric {
                        column: "engagement_rate".into(),
                        agg: AggFn::Avg,
                        alias: "avg_engagement".into(),
                    },
                    WidgetMetric {
                        column: "edit_distance".into(),
                        agg: AggFn::Avg,
                        alias: "avg_edit_distance".into(),
                    },
                    WidgetMetric {
                        column: "platform_post_id".into(),
                        agg: AggFn::Count,
                        alias: "posts".into(),
                    },
                ],
                select_columns: vec![],
                filters: vec![],
                sort: Some(WidgetSort { column: "avg_impressions".into(), dir: SortDir::Desc }),
                limit: 10,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Avg impressions, engagement & edit distance".into()),
                    x_field: Some("model_family".into()),
                    y_field: Some("avg_impressions".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        // Row 3 left: Timing & Target Tier Heatmap (ANL-09, ANL-22)
        StarterWidgetDef {
            title: "Reply Timing × Target Tier",
            position: WidgetPosition { x: 0, y: 4, w: 6, h: 2 },
            spec: WidgetSpec {
                title: "Reply Timing × Target Tier".into(),
                view: AllowedView::VAnalyticsTimingTier,
                chart_type: ChartType::Heatmap,
                group_by: vec!["timing_bucket".into(), "follower_tier".into()],
                metrics: vec![
                    WidgetMetric {
                        column: "impressions".into(),
                        agg: AggFn::Avg,
                        alias: "avg_impressions".into(),
                    },
                    WidgetMetric {
                        column: "platform_post_id".into(),
                        agg: AggFn::Count,
                        alias: "posts".into(),
                    },
                ],
                select_columns: vec![],
                filters: vec![],
                sort: Some(WidgetSort { column: "avg_impressions".into(), dir: SortDir::Desc }),
                limit: 40,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Avg impressions by reply age and author follower bucket".into()),
                    x_field: Some("timing_bucket".into()),
                    y_field: Some("follower_tier".into()),
                    value_field: Some("avg_impressions".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        // Row 3 right: Action Bundle & Post Kind (Bar, ANL-10)
        StarterWidgetDef {
            title: "By Action Bundle & Kind",
            position: WidgetPosition { x: 6, y: 4, w: 6, h: 2 },
            spec: WidgetSpec {
                title: "By Action Bundle & Kind".into(),
                view: AllowedView::VAnalyticsByAction,
                chart_type: ChartType::Bar,
                group_by: vec!["actions_bundle".into()],
                metrics: vec![
                    WidgetMetric {
                        column: "impressions".into(),
                        agg: AggFn::Avg,
                        alias: "avg_impressions".into(),
                    },
                    WidgetMetric {
                        column: "engagement_rate".into(),
                        agg: AggFn::Avg,
                        alias: "avg_engagement".into(),
                    },
                    WidgetMetric {
                        column: "platform_post_id".into(),
                        agg: AggFn::Count,
                        alias: "posts".into(),
                    },
                ],
                select_columns: vec![],
                filters: vec![],
                sort: Some(WidgetSort { column: "avg_impressions".into(), dir: SortDir::Desc }),
                limit: 12,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Replies vs OGC, like+reply vs reply-only, external".into()),
                    x_field: Some("actions_bundle".into()),
                    y_field: Some("avg_impressions".into()),
                    format: Some("number".into()),
                    ..Default::default()
                },
            },
        },
        // Row 4: By Viral Format (Table, ANL-10)
        StarterWidgetDef {
            title: "OGC Performance by Viral Format",
            position: WidgetPosition { x: 0, y: 6, w: 12, h: 2 },
            spec: WidgetSpec {
                title: "OGC Performance by Viral Format".into(),
                view: AllowedView::VAnalyticsByFormat,
                chart_type: ChartType::Table,
                group_by: vec!["viral_format_name".into(), "archetype".into()],
                metrics: vec![
                    WidgetMetric {
                        column: "platform_post_id".into(),
                        agg: AggFn::Count,
                        alias: "posts".into(),
                    },
                    WidgetMetric {
                        column: "impressions".into(),
                        agg: AggFn::Avg,
                        alias: "avg_impressions".into(),
                    },
                    WidgetMetric {
                        column: "engagement_rate".into(),
                        agg: AggFn::Avg,
                        alias: "avg_engagement".into(),
                    },
                    WidgetMetric {
                        column: "bookmarks".into(),
                        agg: AggFn::Sum,
                        alias: "bookmarks".into(),
                    },
                ],
                select_columns: vec![],
                filters: vec![],
                sort: Some(WidgetSort { column: "avg_impressions".into(), dir: SortDir::Desc }),
                limit: 20,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Which OGC templates resonate best".into()),
                    ..Default::default()
                },
            },
        },
        // Row 5: Winners & Flops Gallery (ANL-11)
        StarterWidgetDef {
            title: "Winners & Flops Gallery",
            position: WidgetPosition { x: 0, y: 8, w: 12, h: 3 },
            spec: WidgetSpec {
                title: "Winners & Flops Gallery".into(),
                view: AllowedView::VAnalyticsWinnersFlops,
                chart_type: ChartType::Table,
                group_by: vec![],
                metrics: vec![],
                select_columns: vec![
                    "tier".into(),
                    "draft_kind".into(),
                    "model_family".into(),
                    "post_text".into(),
                    "context_author".into(),
                    "context_text".into(),
                    "impressions".into(),
                    "engagement_rate".into(),
                    "edit_distance".into(),
                    "is_external".into(),
                ],
                filters: vec![WidgetFilter {
                    column: "tier".into(),
                    op: FilterOp::In,
                    value: serde_json::json!(["winner", "flop"]),
                }],
                sort: Some(WidgetSort { column: "impressions".into(), dir: SortDir::Desc }),
                limit: 20,
                window_days: 30,
                options: WidgetOptions {
                    subtitle: Some("Top 10% and bottom 10% posts with text & context".into()),
                    gallery_mode: true,
                    ..Default::default()
                },
            },
        },
    ]
}
