import { inTauri } from "../lib/api";
import { serializeWidgetCsv } from "./csv";
import type {
  AllowedView,
  ChartType,
  DashboardLayout,
  DashboardSnapshot,
  DashboardWidget,
  EvaluatedWidget,
  RepromptResult,
  SyncStatus,
  ViewSchemaInfo,
  WidgetPosition,
  WidgetPositionUpdate,
  WidgetQueryResult,
  WidgetSpec,
} from "./types";

async function invokeCmd<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

const PREVIEW_STORAGE_KEY = "sgh-analytics-preview-v1";

interface PreviewPostRow {
  platform_post_id: string;
  draft_kind: "reply" | "quote" | "ogc" | "external";
  day: string;
  week: string;
  post_text: string;
  context_text: string;
  context_author: string;
  model_family: "Claude" | "Grok" | "External";
  actions_bundle: "like+reply" | "reply-only" | "quote" | "ogc" | "external";
  viral_format_name: string;
  archetype: string;
  edit_distance: number;
  timing_bucket: "<15m" | "15-60m" | "1-3h" | ">3h" | "unknown";
  follower_tier: "<1k" | "1k-10k" | "10k-100k" | "100k+" | "unknown";
  impressions: number;
  likes: number;
  replies: number;
  reposts: number;
  bookmarks: number;
  engagement_rate: number;
  is_external: number;
}

const SAMPLE_POSTS: PreviewPostRow[] = [
  {
    platform_post_id: "1001",
    draft_kind: "reply",
    day: "2026-11-20",
    week: "2026-W47",
    post_text: "Shipping local-first beats waiting for another cloud SaaS auth migration.",
    context_text: "Why are desktop tools suddenly feeling faster than web apps again?",
    context_author: "patio11",
    model_family: "Claude",
    actions_bundle: "like+reply",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 4,
    timing_bucket: "<15m",
    follower_tier: "100k+",
    impressions: 14200,
    likes: 340,
    replies: 28,
    reposts: 19,
    bookmarks: 64,
    engagement_rate: 3.18,
    is_external: 0,
  },
  {
    platform_post_id: "1002",
    draft_kind: "reply",
    day: "2026-11-21",
    week: "2026-W47",
    post_text: "Half of AI growth advice is just repackaged spam. Good replies still look human.",
    context_text: "Unpopular opinion: automated engagement kills accounts faster than silence.",
    context_author: "levelsio",
    model_family: "Grok",
    actions_bundle: "like+reply",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 11,
    timing_bucket: "15-60m",
    follower_tier: "100k+",
    impressions: 9400,
    likes: 215,
    replies: 17,
    reposts: 12,
    bookmarks: 39,
    engagement_rate: 3.01,
    is_external: 0,
  },
  {
    platform_post_id: "1003",
    draft_kind: "ogc",
    day: "2026-11-22",
    week: "2026-W47",
    post_text: "3 lessons after replacing 4 SaaS dashboards with one local SQLite file:\n1. Zero latency\n2. Own your schema\n3. Reprompt any chart live.",
    context_text: "",
    context_author: "",
    model_family: "Claude",
    actions_bundle: "ogc",
    viral_format_name: "3-Line Proof + Punchline",
    archetype: "mini_breakdown",
    edit_distance: 6,
    timing_bucket: "unknown",
    follower_tier: "unknown",
    impressions: 18600,
    likes: 490,
    replies: 41,
    reposts: 52,
    bookmarks: 210,
    engagement_rate: 4.26,
    is_external: 0,
  },
  {
    platform_post_id: "1004",
    draft_kind: "reply",
    day: "2026-11-23",
    week: "2026-W48",
    post_text: "The real moat is taste plus edit distance — not prompt length.",
    context_text: "Everyone has access to the exact same frontier models now. What still separates winners?",
    context_author: "swyx",
    model_family: "Claude",
    actions_bundle: "like+reply",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 3,
    timing_bucket: "<15m",
    follower_tier: "10k-100k",
    impressions: 11300,
    likes: 280,
    replies: 19,
    reposts: 15,
    bookmarks: 58,
    engagement_rate: 3.29,
    is_external: 0,
  },
  {
    platform_post_id: "1005",
    draft_kind: "reply",
    day: "2026-11-24",
    week: "2026-W48",
    post_text: "Agreed, though I'd still keep a manual kill switch handy.",
    context_text: "Should creator tools ever post while you sleep?",
    context_author: "dan_abramov",
    model_family: "Grok",
    actions_bundle: "reply-only",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 15,
    timing_bucket: "1-3h",
    follower_tier: "10k-100k",
    impressions: 1150,
    likes: 18,
    replies: 2,
    reposts: 0,
    bookmarks: 3,
    engagement_rate: 2.0,
    is_external: 0,
  },
  {
    platform_post_id: "1006",
    draft_kind: "ogc",
    day: "2026-11-25",
    week: "2026-W48",
    post_text: "Hot take: if you cannot inspect why a recommendation fired, it is not a learning loop — it is drift.",
    context_text: "",
    context_author: "",
    model_family: "Grok",
    actions_bundle: "ogc",
    viral_format_name: "Contrarian Truth",
    archetype: "contrarian_take",
    edit_distance: 9,
    timing_bucket: "unknown",
    follower_tier: "unknown",
    impressions: 7800,
    likes: 164,
    replies: 22,
    reposts: 14,
    bookmarks: 47,
    engagement_rate: 3.17,
    is_external: 0,
  },
  {
    platform_post_id: "1007",
    draft_kind: "external",
    day: "2026-11-25",
    week: "2026-W48",
    post_text: "Quick coffee walk before diving into Phase 4 charts.",
    context_text: "",
    context_author: "",
    model_family: "External",
    actions_bundle: "external",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 0,
    timing_bucket: "unknown",
    follower_tier: "unknown",
    impressions: 620,
    likes: 11,
    replies: 1,
    reposts: 0,
    bookmarks: 0,
    engagement_rate: 1.94,
    is_external: 1,
  },
  {
    platform_post_id: "1008",
    draft_kind: "reply",
    day: "2026-11-26",
    week: "2026-W48",
    post_text: "We benchmarked replying under 15m vs 3h later — 5x impression lift on the same voice.",
    context_text: "Does reply timing actually matter on X anymore?",
    context_author: "gregisenberg",
    model_family: "Claude",
    actions_bundle: "like+reply",
    viral_format_name: "None",
    archetype: "none",
    edit_distance: 5,
    timing_bucket: "15-60m",
    follower_tier: "10k-100k",
    impressions: 8450,
    likes: 192,
    replies: 24,
    reposts: 11,
    bookmarks: 69,
    engagement_rate: 3.5,
    is_external: 0,
  },
];

interface PreviewStore {
  layouts: DashboardLayout[];
  widgetsByLayout: Record<string, DashboardWidget[]>;
  syncStatus: SyncStatus;
}

const ALLOWED_VIEWS: AllowedView[] = [
  "v_analytics_account_overview",
  "v_analytics_post_performance",
  "v_analytics_time_series",
  "v_analytics_by_model",
  "v_analytics_timing_tier",
  "v_analytics_by_action",
  "v_analytics_by_format",
  "v_analytics_winners_flops",
  "v_analytics_snapshot_history",
];

function starterWidgetSpecs(): Array<{ title: string; spec: WidgetSpec; position: WidgetPosition }> {
  return [
    {
      title: "Impressions",
      position: { x: 0, y: 0, w: 4, h: 1 },
      spec: {
        title: "Impressions",
        view: "v_analytics_post_performance",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "impressions", agg: "sum", alias: "total_impressions" }],
        select_columns: [],
        filters: [],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Total post impressions in window", value_field: "total_impressions", format: "number" },
      },
    },
    {
      title: "Engagement Rate",
      position: { x: 4, y: 0, w: 4, h: 1 },
      spec: {
        title: "Engagement Rate",
        view: "v_analytics_post_performance",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "engagement_rate", agg: "avg", alias: "avg_engagement_rate" }],
        select_columns: [],
        filters: [],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Mean engagement rate across posts", value_field: "avg_engagement_rate", format: "percent" },
      },
    },
    {
      title: "Follower Delta",
      position: { x: 8, y: 0, w: 4, h: 1 },
      spec: {
        title: "Follower Delta",
        view: "v_analytics_account_overview",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "follower_delta", agg: "sum", alias: "net_followers" }],
        select_columns: [],
        filters: [],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Net followers gained across account syncs", value_field: "net_followers", format: "signed" },
      },
    },
    {
      title: "Replies Published",
      position: { x: 0, y: 1, w: 4, h: 1 },
      spec: {
        title: "Replies Published",
        view: "v_analytics_post_performance",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "platform_post_id", agg: "count", alias: "reply_count" }],
        select_columns: [],
        filters: [{ column: "draft_kind", op: "in", value: ["reply", "quote"] }],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Tracked replies & quotes", value_field: "reply_count", format: "number" },
      },
    },
    {
      title: "OGC Published",
      position: { x: 4, y: 1, w: 4, h: 1 },
      spec: {
        title: "OGC Published",
        view: "v_analytics_post_performance",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "platform_post_id", agg: "count", alias: "ogc_count" }],
        select_columns: [],
        filters: [{ column: "draft_kind", op: "eq", value: "ogc" }],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Original posts tracked", value_field: "ogc_count", format: "number" },
      },
    },
    {
      title: "Avg Impressions / Reply",
      position: { x: 8, y: 1, w: 4, h: 1 },
      spec: {
        title: "Avg Impressions / Reply",
        view: "v_analytics_post_performance",
        chart_type: "kpi",
        group_by: [],
        metrics: [{ column: "impressions", agg: "avg", alias: "avg_reply_impressions" }],
        select_columns: [],
        filters: [{ column: "draft_kind", op: "in", value: ["reply", "quote"] }],
        sort: null,
        limit: 1,
        window_days: 30,
        options: { subtitle: "Average reach per published reply", value_field: "avg_reply_impressions", format: "number" },
      },
    },
    {
      title: "Impressions Trend by Model",
      position: { x: 0, y: 2, w: 8, h: 2 },
      spec: {
        title: "Impressions Trend by Model",
        view: "v_analytics_time_series",
        chart_type: "line",
        group_by: ["day", "model_family"],
        metrics: [{ column: "impressions", agg: "sum", alias: "impressions" }],
        select_columns: [],
        filters: [],
        sort: { column: "day", dir: "asc" },
        limit: 120,
        window_days: 30,
        options: { subtitle: "Daily impressions split by model family", x_field: "day", y_field: "impressions", series_field: "model_family", format: "number" },
      },
    },
    {
      title: "Claude vs Grok Performance",
      position: { x: 8, y: 2, w: 4, h: 2 },
      spec: {
        title: "Claude vs Grok Performance",
        view: "v_analytics_by_model",
        chart_type: "bar",
        group_by: ["model_family"],
        metrics: [
          { column: "impressions", agg: "avg", alias: "avg_impressions" },
          { column: "engagement_rate", agg: "avg", alias: "avg_engagement" },
          { column: "edit_distance", agg: "avg", alias: "avg_edit_distance" },
          { column: "platform_post_id", agg: "count", alias: "posts" },
        ],
        select_columns: [],
        filters: [],
        sort: { column: "avg_impressions", dir: "desc" },
        limit: 10,
        window_days: 30,
        options: { subtitle: "Avg impressions, engagement & edit distance", x_field: "model_family", y_field: "avg_impressions", format: "number" },
      },
    },
    {
      title: "Reply Timing × Target Tier",
      position: { x: 0, y: 4, w: 6, h: 2 },
      spec: {
        title: "Reply Timing × Target Tier",
        view: "v_analytics_timing_tier",
        chart_type: "heatmap",
        group_by: ["timing_bucket", "follower_tier"],
        metrics: [
          { column: "impressions", agg: "avg", alias: "avg_impressions" },
          { column: "platform_post_id", agg: "count", alias: "posts" },
        ],
        select_columns: [],
        filters: [],
        sort: { column: "avg_impressions", dir: "desc" },
        limit: 40,
        window_days: 30,
        options: { subtitle: "Avg impressions by reply age and author follower bucket", x_field: "timing_bucket", y_field: "follower_tier", value_field: "avg_impressions", format: "number" },
      },
    },
    {
      title: "By Action Bundle & Kind",
      position: { x: 6, y: 4, w: 6, h: 2 },
      spec: {
        title: "By Action Bundle & Kind",
        view: "v_analytics_by_action",
        chart_type: "bar",
        group_by: ["actions_bundle"],
        metrics: [
          { column: "impressions", agg: "avg", alias: "avg_impressions" },
          { column: "engagement_rate", agg: "avg", alias: "avg_engagement" },
          { column: "platform_post_id", agg: "count", alias: "posts" },
        ],
        select_columns: [],
        filters: [],
        sort: { column: "avg_impressions", dir: "desc" },
        limit: 12,
        window_days: 30,
        options: { subtitle: "Replies vs OGC, like+reply vs reply-only, external", x_field: "actions_bundle", y_field: "avg_impressions", format: "number" },
      },
    },
    {
      title: "OGC Performance by Viral Format",
      position: { x: 0, y: 6, w: 12, h: 2 },
      spec: {
        title: "OGC Performance by Viral Format",
        view: "v_analytics_by_format",
        chart_type: "table",
        group_by: ["viral_format_name", "archetype"],
        metrics: [
          { column: "platform_post_id", agg: "count", alias: "posts" },
          { column: "impressions", agg: "avg", alias: "avg_impressions" },
          { column: "engagement_rate", agg: "avg", alias: "avg_engagement" },
          { column: "bookmarks", agg: "sum", alias: "bookmarks" },
        ],
        select_columns: [],
        filters: [],
        sort: { column: "avg_impressions", dir: "desc" },
        limit: 20,
        window_days: 30,
        options: { subtitle: "Which OGC templates resonate best" },
      },
    },
    {
      title: "Winners & Flops Gallery",
      position: { x: 0, y: 8, w: 12, h: 3 },
      spec: {
        title: "Winners & Flops Gallery",
        view: "v_analytics_winners_flops",
        chart_type: "table",
        group_by: [],
        metrics: [],
        select_columns: [
          "tier",
          "draft_kind",
          "model_family",
          "post_text",
          "context_author",
          "context_text",
          "impressions",
          "engagement_rate",
          "edit_distance",
          "is_external",
        ],
        filters: [{ column: "tier", op: "in", value: ["winner", "flop"] }],
        sort: { column: "impressions", dir: "desc" },
        limit: 20,
        window_days: 30,
        options: { subtitle: "Top 10% and bottom 10% posts with text & context", gallery_mode: true },
      },
    },
  ];
}

function makeId(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 10)}`;
}

function seedPreviewStore(): PreviewStore {
  const layoutId = "starter-v1";
  const now = new Date().toISOString();
  const layout: DashboardLayout = {
    id: layoutId,
    profile_id: "preview",
    name: "Starter Overview",
    version: 1,
    is_active: true,
    is_starter: true,
    created_at: now,
  };
  const widgets: DashboardWidget[] = starterWidgetSpecs().map((w, i) => ({
    id: `w-starter-${i + 1}`,
    layout_id: layoutId,
    title: w.title,
    spec: w.spec,
    position: w.position,
    source_prompt: null,
    created_at: now,
  }));
  return {
    layouts: [layout],
    widgetsByLayout: { [layoutId]: widgets },
    syncStatus: { last_update_at: now, blocked: null },
  };
}

let memoryStore: PreviewStore | null = null;

export function resetPreviewStore(): void {
  memoryStore = null;
  if (typeof localStorage !== "undefined") {
    localStorage.removeItem(PREVIEW_STORAGE_KEY);
  }
}

function loadStore(): PreviewStore {
  if (typeof localStorage === "undefined") {
    if (!memoryStore) memoryStore = seedPreviewStore();
    return memoryStore;
  }
  const raw = localStorage.getItem(PREVIEW_STORAGE_KEY);
  if (!raw) {
    const seeded = seedPreviewStore();
    saveStore(seeded);
    return seeded;
  }
  try {
    return JSON.parse(raw) as PreviewStore;
  } catch {
    const seeded = seedPreviewStore();
    saveStore(seeded);
    return seeded;
  }
}

function saveStore(store: PreviewStore): void {
  if (typeof localStorage !== "undefined") {
    localStorage.setItem(PREVIEW_STORAGE_KEY, JSON.stringify(store));
  } else {
    memoryStore = store;
  }
}

function rowsForPreviewView(view: AllowedView): Array<Record<string, unknown>> {
  if (view === "v_analytics_account_overview") {
    return [
      { day: "2026-11-20", impressions: 22100, engagement_rate: 3.1, profile_visits: 310, followers_total: 4210, follower_delta: 42 },
      { day: "2026-11-23", impressions: 39400, engagement_rate: 3.5, profile_visits: 520, followers_total: 4338, follower_delta: 128 },
      { day: "2026-11-26", impressions: 71520, engagement_rate: 3.4, profile_visits: 790, followers_total: 4465, follower_delta: 127 },
    ];
  }
  if (view === "v_analytics_timing_tier") {
    return SAMPLE_POSTS.filter((p) => p.draft_kind === "reply" || p.draft_kind === "quote").map((p) => ({ ...p }));
  }
  if (view === "v_analytics_by_format") {
    return SAMPLE_POSTS.filter((p) => p.draft_kind === "ogc").map((p) => ({
      ...p,
      observed_win_rate: 0.34,
      personal_win_rate: 0.42,
    }));
  }
  if (view === "v_analytics_winners_flops") {
    const sorted = [...SAMPLE_POSTS].sort((a, b) => b.impressions - a.impressions);
    const k = Math.max(1, Math.ceil(sorted.length * 0.25));
    return sorted.map((p, idx) => ({
      ...p,
      tier: idx < k ? "winner" : idx >= sorted.length - k ? "flop" : "middle",
      rank_desc: idx + 1,
      total_posts: sorted.length,
    }));
  }
  return SAMPLE_POSTS.map((p) => ({ ...p }));
}

export function validatePreviewSpec(rawJson: string): WidgetSpec {
  const lower = rawJson.toLowerCase();
  for (const tok of ["<script", "eval(", "drop table", "delete from", "insert into", "provider_accounts", "app_settings", "sqlite_master"]) {
    if (lower.includes(tok)) {
      throw new Error(`sandbox blocked forbidden token "${tok}" — widgets are read-only and restricted to analytics views`);
    }
  }
  const parsed = JSON.parse(rawJson) as Record<string, unknown>;
  for (const forbidden of ["sql", "query", "raw_sql", "script", "code", "eval", "fetch"]) {
    if (forbidden in parsed) {
      throw new Error(`sandbox rejected field "${forbidden}": widgets may only read allowlisted views`);
    }
  }
  if (!ALLOWED_VIEWS.includes(parsed.view as AllowedView)) {
    throw new Error(`sandbox rejected view "${String(parsed.view)}": only allowlisted read-only analytics views are permitted`);
  }
  const spec = parsed as unknown as WidgetSpec;
  if (!spec.title?.trim()) throw new Error("widget title is required");
  if ((spec.limit ?? 50) > 500 || (spec.limit ?? 50) < 1) throw new Error("widget row limit must be 1..500");
  if ((spec.window_days ?? 30) > 365 || (spec.window_days ?? 30) < 1) throw new Error("widget window_days must be 1..365");
  return spec;
}

export function evaluatePreviewSpec(spec: WidgetSpec, windowDays?: number): WidgetQueryResult {
  const effWindow = windowDays ?? spec.window_days ?? 30;
  let rows = rowsForPreviewView(spec.view);
  if (effWindow <= 7) {
    rows = rows.filter((r) => String(r.day ?? "2026-11-26") >= "2026-11-20");
  }
  for (const f of spec.filters ?? []) {
    rows = rows.filter((r) => {
      const v = r[f.column];
      switch (f.op) {
        case "eq":
          return v === f.value;
        case "neq":
          return v !== f.value;
        case "gt":
          return Number(v) > Number(f.value);
        case "gte":
          return Number(v) >= Number(f.value);
        case "lt":
          return Number(v) < Number(f.value);
        case "lte":
          return Number(v) <= Number(f.value);
        case "contains":
          return String(v ?? "").toLowerCase().includes(String(f.value ?? "").toLowerCase());
        case "in":
          return Array.isArray(f.value) && f.value.includes(v);
        default:
          return true;
      }
    });
  }

  let outRows: Array<Record<string, unknown>> = [];
  let columns: string[] = [];

  if ((spec.group_by?.length ?? 0) > 0 || (spec.metrics?.length ?? 0) > 0) {
    const gcols = spec.group_by ?? [];
    const groups = new Map<string, Array<Record<string, unknown>>>();
    for (const r of rows) {
      const key = gcols.map((g) => String(r[g] ?? "")).join("\0");
      const list = groups.get(key) ?? [];
      list.push(r);
      groups.set(key, list);
    }
    if (groups.size === 0 && gcols.length === 0) {
      groups.set("", []);
    }
    columns = [...gcols, ...(spec.metrics ?? []).map((m) => m.alias)];
    for (const list of groups.values()) {
      const obj: Record<string, unknown> = {};
      for (const g of gcols) obj[g] = list[0]?.[g] ?? null;
      for (const m of spec.metrics ?? []) {
        const nums = list.map((r) => Number(r[m.column] ?? 0)).filter((n) => !Number.isNaN(n));
        if (m.agg === "count") obj[m.alias] = list.length;
        else if (m.agg === "sum") obj[m.alias] = Math.round(nums.reduce((a, b) => a + b, 0) * 100) / 100;
        else if (m.agg === "avg") obj[m.alias] = nums.length ? Math.round((nums.reduce((a, b) => a + b, 0) / nums.length) * 100) / 100 : 0;
        else if (m.agg === "min") obj[m.alias] = nums.length ? Math.min(...nums) : 0;
        else obj[m.alias] = nums.length ? Math.max(...nums) : 0;
      }
      outRows.push(obj);
    }
  } else {
    columns = spec.select_columns;
    outRows = rows.map((r) => {
      const obj: Record<string, unknown> = {};
      for (const c of columns) obj[c] = r[c] ?? null;
      return obj;
    });
  }

  if (spec.sort) {
    const { column, dir } = spec.sort;
    outRows.sort((a, b) => {
      const av = a[column];
      const bv = b[column];
      const cmp =
        typeof av === "number" && typeof bv === "number"
          ? av - bv
          : String(av ?? "").localeCompare(String(bv ?? ""));
      return dir === "asc" ? cmp : -cmp;
    });
  }

  const lim = spec.limit ?? 50;
  const truncated = outRows.length > lim;
  outRows = outRows.slice(0, lim);

  return {
    columns,
    rows: outRows,
    row_count: outRows.length,
    truncated,
    window_days: effWindow,
    executed_view: spec.view,
  };
}

function previewReprompt(prompt: string, currentSpec: WidgetSpec | null, windowDays?: number): RepromptResult {
  const trimmed = prompt.trim();
  if (!trimmed) throw new Error("enter a prompt describing the widget you want");
  const lower = trimmed.toLowerCase();
  for (const tok of ["drop table", "delete from", "provider_accounts", "keychain", "api key", "<script", "eval("]) {
    if (lower.includes(tok)) {
      throw new Error(`sandbox rejected unsafe prompt containing "${tok}": dashboard widgets are strictly read-only`);
    }
  }

  const detectChart = (): ChartType | null => {
    if (lower.includes("stacked")) return "stacked_bar";
    if (lower.includes("heatmap")) return "heatmap";
    if (lower.includes("scatter")) return "scatter";
    if (lower.includes("funnel")) return "funnel";
    if (lower.includes("line")) return "line";
    if (lower.includes("bar")) return "bar";
    if (lower.includes("table")) return "table";
    if (lower.includes("kpi")) return "kpi";
    return null;
  };

  let spec: WidgetSpec;
  if (currentSpec) {
    spec = JSON.parse(JSON.stringify(currentSpec)) as WidgetSpec;
    const ct = detectChart();
    if (ct) spec.chart_type = ct;
    if (lower.includes("7 day") || lower.includes("7d")) spec.window_days = 7;
    if (lower.includes("30 day") || lower.includes("30d")) spec.window_days = 30;
    if (lower.includes("claude only") || lower.includes("filter to claude")) {
      spec.filters = [...spec.filters.filter((f) => f.column !== "model_family"), { column: "model_family", op: "eq", value: "Claude" }];
    }
    if (lower.includes("rename to ")) {
      spec.title = trimmed.slice(lower.indexOf("rename to ") + 10).trim();
    }
  } else if (lower.includes("scatter")) {
    spec = {
      title: "Edit Distance vs Impressions",
      view: "v_analytics_post_performance",
      chart_type: "scatter",
      group_by: [],
      metrics: [],
      select_columns: ["platform_post_id", "model_family", "edit_distance", "impressions"],
      filters: [],
      sort: { column: "impressions", dir: "desc" },
      limit: 50,
      window_days: windowDays ?? 30,
      options: { x_field: "edit_distance", y_field: "impressions", series_field: "model_family", subtitle: "Each dot is a published post" },
    };
  } else if (lower.includes("funnel")) {
    spec = {
      title: "Action Bundle Funnel",
      view: "v_analytics_by_action",
      chart_type: "funnel",
      group_by: ["actions_bundle"],
      metrics: [{ column: "impressions", agg: "sum", alias: "impressions" }],
      select_columns: [],
      filters: [],
      sort: { column: "impressions", dir: "desc" },
      limit: 10,
      window_days: windowDays ?? 30,
      options: { x_field: "actions_bundle", y_field: "impressions", value_field: "impressions" },
    };
  } else if (lower.includes("week") || lower.includes("trend") || lower.includes("over time")) {
    const timeCol = lower.includes("week") ? "week" : "day";
    spec = {
      title: trimmed.charAt(0).toUpperCase() + trimmed.slice(1, 68),
      view: "v_analytics_time_series",
      chart_type: detectChart() ?? "stacked_bar",
      group_by: [timeCol, "model_family"],
      metrics: [{ column: "impressions", agg: "sum", alias: "impressions" }],
      select_columns: [],
      filters: lower.includes("reply") ? [{ column: "draft_kind", op: "eq", value: "reply" }] : [],
      sort: { column: timeCol, dir: "asc" },
      limit: 120,
      window_days: windowDays ?? 30,
      options: { x_field: timeCol, y_field: "impressions", series_field: "model_family", subtitle: `Grouped by ${timeCol} and model` },
    };
  } else {
    spec = {
      title: trimmed.charAt(0).toUpperCase() + trimmed.slice(1, 68),
      view: "v_analytics_by_model",
      chart_type: detectChart() ?? "bar",
      group_by: ["model_family"],
      metrics: [
        { column: "impressions", agg: "avg", alias: "avg_impressions" },
        { column: "edit_distance", agg: "avg", alias: "avg_edit_distance" },
      ],
      select_columns: [],
      filters: [],
      sort: { column: "avg_impressions", dir: "desc" },
      limit: 10,
      window_days: windowDays ?? 30,
      options: { x_field: "model_family", y_field: "avg_impressions" },
    };
  }

  return {
    spec,
    preview: evaluatePreviewSpec(spec, windowDays),
    source_prompt: trimmed,
    generated_by: "local_planner",
    route_status: {
      configured: false,
      provider_type: null,
      provider_label: null,
      model_id: null,
      note: "No model is assigned to dashboard_builder yet (Settings → AI models). Using built-in deterministic widget planner.",
    },
    summary: `Preview ready (${spec.chart_type} over ${spec.view}).`,
  };
}

export const analyticsApi = {
  async getDashboard(windowDays = 30): Promise<DashboardSnapshot> {
    if (inTauri()) return invokeCmd("analytics_get_dashboard", { windowDays });
    const store = loadStore();
    const active = store.layouts.find((l) => l.is_active) ?? store.layouts[0];
    const widgets = (store.widgetsByLayout[active.id] ?? []).sort(
      (a, b) => a.position.y - b.position.y || a.position.x - b.position.x,
    );
    const evaluated: EvaluatedWidget[] = widgets.map((widget) => {
      try {
        return { widget, data: evaluatePreviewSpec(widget.spec, windowDays), error: null };
      } catch (e) {
        return { widget, data: null, error: (e as Error).message };
      }
    });
    return {
      active_layout: active,
      layouts: store.layouts,
      widgets: evaluated,
      schedule: {
        tracked_posts: 7,
        posts_due_now: 2,
        missed_windows: 1,
        items: [
          {
            platform_post_id: "1008",
            draft_kind: "reply",
            draft_id: "draft-1008",
            published_url: "https://x.com/demo/status/1008",
            published_at: "2026-11-26T12:00:00Z",
            age_hours: 6.2,
            captured_count: 1,
            needs_sync: true,
            missed_backfill_count: 0,
            checkpoints: [
              { label: "1h", target_hours: 1, state: "captured" },
              { label: "6h", target_hours: 6, state: "due_now" },
              { label: "24h", target_hours: 24, state: "upcoming" },
              { label: "72h", target_hours: 72, state: "upcoming" },
              { label: "7d", target_hours: 168, state: "upcoming" },
            ],
          },
          {
            platform_post_id: "1003",
            draft_kind: "ogc",
            draft_id: "draft-1003",
            published_url: "https://x.com/demo/status/1003",
            published_at: "2026-11-22T12:00:00Z",
            age_hours: 96,
            captured_count: 2,
            needs_sync: true,
            missed_backfill_count: 1,
            checkpoints: [
              { label: "1h", target_hours: 1, state: "captured" },
              { label: "6h", target_hours: 6, state: "captured" },
              { label: "24h", target_hours: 24, state: "missed_backfill" },
              { label: "72h", target_hours: 72, state: "due_now" },
              { label: "7d", target_hours: 168, state: "upcoming" },
            ],
          },
        ],
      },
      sync_status: store.syncStatus ?? { last_update_at: null, blocked: null },
      builder_route: {
        configured: false,
        provider_type: null,
        provider_label: null,
        model_id: null,
        note: "No model is assigned to dashboard_builder yet (Settings → AI models). Using built-in deterministic widget planner.",
      },
      window_days: windowDays,
      activity_summary: {
        scouted_posts: 24,
        evaluated_posts: 24,
        queued_posts: 6,
        drafted_replies: 6,
        published_replies: SAMPLE_POSTS.filter((p) => p.draft_kind === "reply" || p.draft_kind === "quote").length,
        drafted_ogc: 3,
        published_ogc: SAMPLE_POSTS.filter((p) => p.draft_kind === "ogc").length,
        engagement_actions: 14,
        likes_given: 9,
        bookmarks_saved: 3,
        reposts_made: 1,
        follows_made: 1,
        quotes_made: 0,
        synced_post_snapshots: SAMPLE_POSTS.length,
        synced_account_snapshots: 3,
        unsynced_published_posts: 0,
      },
    };
  },

  async listViews(): Promise<ViewSchemaInfo[]> {
    if (inTauri()) return invokeCmd("analytics_list_views");
    return ALLOWED_VIEWS.map((v) => ({ view: v, sql_name: v, description: v, columns: [], time_column: "captured_at" }));
  },

  async switchLayout(layoutId: string): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_switch_layout", { layoutId });
    const store = loadStore();
    store.layouts = store.layouts.map((l) => ({ ...l, is_active: l.id === layoutId }));
    saveStore(store);
    return store.layouts.find((l) => l.id === layoutId)!;
  },

  async saveLayoutVersion(layoutId: string, newName?: string): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_save_layout_version", { layoutId, newName: newName ?? null });
    const store = loadStore();
    const src = store.layouts.find((l) => l.id === layoutId)!;
    const name = newName?.trim() || src.name;
    const nextVer = Math.max(0, ...store.layouts.filter((l) => l.name === name).map((l) => l.version)) + 1;
    const id = makeId("layout");
    const created: DashboardLayout = {
      id,
      profile_id: "preview",
      name,
      version: nextVer,
      is_active: true,
      is_starter: false,
      created_at: new Date().toISOString(),
    };
    store.layouts = [...store.layouts.map((l) => ({ ...l, is_active: false })), created];
    store.widgetsByLayout[id] = (store.widgetsByLayout[layoutId] ?? []).map((w) => ({
      ...w,
      id: makeId("w"),
      layout_id: id,
    }));
    saveStore(store);
    return created;
  },

  async renameLayout(layoutId: string, newName: string): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_rename_layout", { layoutId, newName });
    const store = loadStore();
    store.layouts = store.layouts.map((l) => (l.id === layoutId ? { ...l, name: newName.trim() } : l));
    saveStore(store);
    return store.layouts.find((l) => l.id === layoutId)!;
  },

  async duplicateLayout(layoutId: string, newName: string): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_duplicate_layout", { layoutId, newName });
    return this.saveLayoutVersion(layoutId, newName);
  },

  async deleteLayout(layoutId: string): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_delete_layout", { layoutId });
    const store = loadStore();
    if (store.layouts.length <= 1) throw new Error("cannot delete the only remaining dashboard layout");
    store.layouts = store.layouts.filter((l) => l.id !== layoutId);
    delete store.widgetsByLayout[layoutId];
    if (!store.layouts.some((l) => l.is_active)) {
      store.layouts[0].is_active = true;
    }
    saveStore(store);
    return store.layouts.find((l) => l.is_active)!;
  },

  async resetStarterLayout(): Promise<DashboardLayout> {
    if (inTauri()) return invokeCmd("analytics_reset_starter_layout");
    const store = loadStore();
    const nextVer = Math.max(0, ...store.layouts.filter((l) => l.name === "Starter Overview").map((l) => l.version)) + 1;
    const id = makeId("starter");
    const now = new Date().toISOString();
    const created: DashboardLayout = {
      id,
      profile_id: "preview",
      name: "Starter Overview",
      version: nextVer,
      is_active: true,
      is_starter: true,
      created_at: now,
    };
    store.layouts = [...store.layouts.map((l) => ({ ...l, is_active: false })), created];
    store.widgetsByLayout[id] = starterWidgetSpecs().map((w) => ({
      id: makeId("w"),
      layout_id: id,
      title: w.title,
      spec: w.spec,
      position: w.position,
      source_prompt: null,
      created_at: now,
    }));
    saveStore(store);
    return created;
  },

  async addWidget(layoutId: string, spec: WidgetSpec, position?: WidgetPosition, sourcePrompt?: string): Promise<DashboardWidget> {
    if (inTauri()) {
      return invokeCmd("analytics_add_widget", {
        layoutId,
        spec,
        position: position ?? null,
        sourcePrompt: sourcePrompt ?? null,
      });
    }
    const store = loadStore();
    const existing = store.widgetsByLayout[layoutId] ?? [];
    const maxY = existing.reduce((m, w) => Math.max(m, w.position.y + w.position.h), 0);
    const widget: DashboardWidget = {
      id: makeId("w"),
      layout_id: layoutId,
      title: spec.title,
      spec,
      position: position ?? { x: 0, y: maxY, w: 6, h: 2 },
      source_prompt: sourcePrompt ?? null,
      created_at: new Date().toISOString(),
    };
    store.widgetsByLayout[layoutId] = [...existing, widget];
    saveStore(store);
    return widget;
  },

  async updateWidget(
    layoutId: string,
    widgetId: string,
    spec: WidgetSpec,
    position?: WidgetPosition,
    sourcePrompt?: string,
  ): Promise<DashboardWidget> {
    if (inTauri()) {
      return invokeCmd("analytics_update_widget", {
        layoutId,
        widgetId,
        spec,
        position: position ?? null,
        sourcePrompt: sourcePrompt ?? null,
      });
    }
    const store = loadStore();
    const list = store.widgetsByLayout[layoutId] ?? [];
    let updated: DashboardWidget | null = null;
    store.widgetsByLayout[layoutId] = list.map((w) => {
      if (w.id !== widgetId) return w;
      updated = {
        ...w,
        title: spec.title,
        spec,
        position: position ?? w.position,
        source_prompt: sourcePrompt ?? w.source_prompt,
      };
      return updated;
    });
    saveStore(store);
    if (!updated) throw new Error("widget not found");
    return updated;
  },

  async removeWidget(layoutId: string, widgetId: string): Promise<void> {
    if (inTauri()) return invokeCmd("analytics_remove_widget", { layoutId, widgetId });
    const store = loadStore();
    store.widgetsByLayout[layoutId] = (store.widgetsByLayout[layoutId] ?? []).filter((w) => w.id !== widgetId);
    saveStore(store);
  },

  async updatePositions(layoutId: string, updates: WidgetPositionUpdate[]): Promise<DashboardWidget[]> {
    if (inTauri()) return invokeCmd("analytics_update_widget_positions", { layoutId, updates });
    const store = loadStore();
    const byId = new Map(updates.map((u) => [u.widget_id, u.position]));
    store.widgetsByLayout[layoutId] = (store.widgetsByLayout[layoutId] ?? []).map((w) =>
      byId.has(w.id) ? { ...w, position: byId.get(w.id)! } : w,
    );
    saveStore(store);
    return store.widgetsByLayout[layoutId];
  },

  async previewSpecJson(specJson: string, windowDays?: number): Promise<[WidgetSpec, WidgetQueryResult]> {
    if (inTauri()) return invokeCmd("analytics_preview_widget_spec", { specJson, windowDays: windowDays ?? null });
    const spec = validatePreviewSpec(specJson);
    return [spec, evaluatePreviewSpec(spec, windowDays)];
  },

  async repromptWidget(prompt: string, currentSpec?: WidgetSpec | null, windowDays?: number): Promise<RepromptResult> {
    if (inTauri()) {
      return invokeCmd("analytics_reprompt_widget", {
        prompt,
        currentSpec: currentSpec ?? null,
        windowDays: windowDays ?? null,
      });
    }
    return previewReprompt(prompt, currentSpec ?? null, windowDays);
  },

  async exportWidgetCsv(spec: WidgetSpec, windowDays?: number): Promise<string> {
    if (inTauri()) return invokeCmd("analytics_export_widget_csv", { spec, windowDays: windowDays ?? null });
    return serializeWidgetCsv(evaluatePreviewSpec(spec, windowDays));
  },
};
