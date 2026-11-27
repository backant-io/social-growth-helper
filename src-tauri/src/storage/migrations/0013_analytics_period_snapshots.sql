-- Migration 13: store X Creator / Premium subscription analytics period snapshots (7d / 30d)
-- separately from per-post snapshots, and upgrade untouched starter KPI cards to read
-- period totals from `v_analytics_account_overview` instead of local post aggregates.

CREATE TABLE analytics_period_snapshots (
    id TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profiles(id),
    period_days INTEGER NOT NULL CHECK (period_days IN (7, 30)),
    period_label TEXT,
    captured_at TEXT NOT NULL,
    impressions INTEGER,
    engagements INTEGER,
    engagement_rate REAL,
    profile_visits INTEGER,
    followers_total INTEGER,
    follower_delta INTEGER,
    new_follows INTEGER,
    unfollows INTEGER,
    likes INTEGER,
    replies INTEGER,
    reposts INTEGER,
    bookmarks INTEGER,
    source TEXT NOT NULL DEFAULT 'dom_fallback',
    raw_json TEXT
);

CREATE INDEX idx_analytics_period_snaps_lookup
    ON analytics_period_snapshots(profile_id, period_days, captured_at DESC);

-- Upgrade untouched starter KPI widgets in existing databases so 7d / 30d read X period snapshots.
UPDATE dashboard_widgets
SET spec_json = '{"title":"Impressions","view":"v_analytics_account_overview","chart_type":"kpi","group_by":[],"metrics":[{"column":"impressions","agg":"latest","alias":"total_impressions"}],"select_columns":[],"filters":[],"sort":null,"limit":1,"window_days":30,"options":{"subtitle":"Account impressions for selected period","x_field":null,"y_field":null,"series_field":null,"value_field":"total_impressions","format":"number","gallery_mode":false,"columns":[]}}'
WHERE source_prompt IS NULL
  AND title = 'Impressions'
  AND layout_id IN (SELECT id FROM dashboard_layouts WHERE is_starter = 1)
  AND json_extract(spec_json, '$.view') = 'v_analytics_post_performance'
  AND json_extract(spec_json, '$.chart_type') = 'kpi';

UPDATE dashboard_widgets
SET spec_json = '{"title":"Engagement Rate","view":"v_analytics_account_overview","chart_type":"kpi","group_by":[],"metrics":[{"column":"engagement_rate","agg":"latest","alias":"avg_engagement_rate"}],"select_columns":[],"filters":[],"sort":null,"limit":1,"window_days":30,"options":{"subtitle":"Account engagement rate for selected period","x_field":null,"y_field":null,"series_field":null,"value_field":"avg_engagement_rate","format":"percent","gallery_mode":false,"columns":[]}}'
WHERE source_prompt IS NULL
  AND title = 'Engagement Rate'
  AND layout_id IN (SELECT id FROM dashboard_layouts WHERE is_starter = 1)
  AND json_extract(spec_json, '$.view') = 'v_analytics_post_performance'
  AND json_extract(spec_json, '$.chart_type') = 'kpi';

UPDATE dashboard_widgets
SET spec_json = '{"title":"Follower Delta","view":"v_analytics_account_overview","chart_type":"kpi","group_by":[],"metrics":[{"column":"follower_delta","agg":"latest","alias":"net_followers"}],"select_columns":[],"filters":[],"sort":null,"limit":1,"window_days":30,"options":{"subtitle":"Net followers gained in selected period","x_field":null,"y_field":null,"series_field":null,"value_field":"net_followers","format":"signed","gallery_mode":false,"columns":[]}}'
WHERE source_prompt IS NULL
  AND title = 'Follower Delta'
  AND layout_id IN (SELECT id FROM dashboard_layouts WHERE is_starter = 1)
  AND json_extract(spec_json, '$.view') = 'v_analytics_account_overview'
  AND json_extract(spec_json, '$.chart_type') = 'kpi';
