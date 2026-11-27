export type AllowedView =
  | "v_analytics_account_overview"
  | "v_analytics_post_performance"
  | "v_analytics_time_series"
  | "v_analytics_by_model"
  | "v_analytics_timing_tier"
  | "v_analytics_by_action"
  | "v_analytics_by_format"
  | "v_analytics_winners_flops"
  | "v_analytics_snapshot_history";

export type ChartType =
  | "kpi"
  | "line"
  | "bar"
  | "table"
  | "stacked_bar"
  | "heatmap"
  | "scatter"
  | "funnel";

export type FilterOp = "eq" | "neq" | "gt" | "gte" | "lt" | "lte" | "contains" | "in";

export type AggFn = "sum" | "avg" | "count" | "min" | "max" | "latest";

export type SortDir = "asc" | "desc";

export interface WidgetFilter {
  column: string;
  op: FilterOp;
  value: unknown;
}

export interface WidgetMetric {
  column: string;
  agg: AggFn;
  alias: string;
}

export interface WidgetSort {
  column: string;
  dir: SortDir;
}

export interface WidgetPosition {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface WidgetOptions {
  subtitle?: string | null;
  x_field?: string | null;
  y_field?: string | null;
  series_field?: string | null;
  value_field?: string | null;
  format?: string | null;
  gallery_mode?: boolean;
  columns?: string[];
}

export interface WidgetSpec {
  title: string;
  view: AllowedView;
  chart_type: ChartType;
  group_by: string[];
  metrics: WidgetMetric[];
  select_columns: string[];
  filters: WidgetFilter[];
  sort: WidgetSort | null;
  limit: number;
  window_days: number;
  options: WidgetOptions;
}

export interface DashboardLayout {
  id: string;
  profile_id: string;
  name: string;
  version: number;
  is_active: boolean;
  is_starter: boolean;
  created_at: string;
}

export interface DashboardWidget {
  id: string;
  layout_id: string;
  title: string;
  spec: WidgetSpec;
  position: WidgetPosition;
  source_prompt: string | null;
  created_at: string;
}

export interface WidgetQueryResult {
  columns: string[];
  rows: Array<Record<string, unknown>>;
  row_count: number;
  truncated: boolean;
  window_days: number;
  executed_view: string;
}

export interface EvaluatedWidget {
  widget: DashboardWidget;
  data: WidgetQueryResult | null;
  error: string | null;
}

export type CheckpointState = "captured" | "due_now" | "missed_backfill" | "upcoming";

export interface CheckpointStatus {
  label: string;
  target_hours: number;
  state: CheckpointState;
}

export interface TrackedPostSchedule {
  platform_post_id: string;
  draft_kind: string;
  draft_id: string;
  published_url: string | null;
  published_at: string;
  age_hours: number;
  captured_count: number;
  needs_sync: boolean;
  missed_backfill_count: number;
  checkpoints: CheckpointStatus[];
}

export interface ScheduleSummary {
  tracked_posts: number;
  posts_due_now: number;
  missed_windows: number;
  items: TrackedPostSchedule[];
}

export interface SyncStatus {
  last_update_at: string | null;
  /** Set while X itself needs the user: logged_out, challenge, locked or suspended. */
  blocked: string | null;
}

export interface BuilderRouteStatus {
  configured: boolean;
  provider_type: string | null;
  provider_label: string | null;
  model_id: string | null;
  note: string;
}

export interface RepromptResult {
  spec: WidgetSpec;
  preview: WidgetQueryResult;
  source_prompt: string;
  generated_by: string;
  route_status: BuilderRouteStatus;
  summary: string;
}

export interface ActivitySummary {
  scouted_posts: number;
  evaluated_posts: number;
  queued_posts: number;
  drafted_replies: number;
  published_replies: number;
  drafted_ogc: number;
  published_ogc: number;
  engagement_actions: number;
  likes_given: number;
  bookmarks_saved: number;
  reposts_made: number;
  follows_made: number;
  quotes_made: number;
  synced_post_snapshots: number;
  synced_account_snapshots: number;
  unsynced_published_posts: number;
}

export interface DashboardSnapshot {
  active_layout: DashboardLayout;
  layouts: DashboardLayout[];
  widgets: EvaluatedWidget[];
  schedule: ScheduleSummary;
  sync_status: SyncStatus;
  builder_route: BuilderRouteStatus;
  window_days: number;
  activity_summary?: ActivitySummary;
}

export interface ViewSchemaInfo {
  view: AllowedView;
  sql_name: string;
  description: string;
  columns: string[];
  time_column: string;
}

export interface WidgetPositionUpdate {
  widget_id: string;
  position: WidgetPosition;
}
