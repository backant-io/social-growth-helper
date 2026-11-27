import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it } from "vitest";
import { hasPerformanceData } from "./AnalyticsPage";
import { analyticsApi, evaluatePreviewSpec, resetPreviewStore, validatePreviewSpec } from "./api";
import { formatCompact, formatMetricValue } from "./charts/ChartPrimitives";
import { serializeWidgetCsv } from "./csv";
import { SyncHealthBanner } from "./SyncHealthBanner";
import type { ActivitySummary, DashboardSnapshot } from "./types";

describe("Phase 4 analytics frontend", () => {
  beforeEach(() => {
    resetPreviewStore();
  });

  it("loads the 12-widget starter dashboard and supports add, reprompt, drag/resize persistence, version, and reset", async () => {
    const dash = await analyticsApi.getDashboard(30);
    expect(dash.active_layout.is_starter).toBe(true);
    expect(dash.widgets.length).toBe(12);
    expect(dash.widgets.every((w) => w.error === null && w.data !== null)).toBe(true);
    expect(dash.widgets.some((w) => w.widget.title === "OGC Published" && w.widget.spec.chart_type === "kpi")).toBe(true);
    expect(dash.schedule.items.length).toBeGreaterThan(0);
    expect(dash.schedule.items[0].checkpoints.map((c) => c.label)).toEqual(["1h", "6h", "24h", "72h", "7d"]);

    // Reprompt a new scatter widget and add it
    const reprompted = await analyticsApi.repromptWidget(
      "Scatter of edit distance vs impressions by model",
      null,
      30,
    );
    expect(reprompted.spec.chart_type).toBe("scatter");
    expect(reprompted.preview.row_count).toBeGreaterThan(0);

    const added = await analyticsApi.addWidget(
      dash.active_layout.id,
      reprompted.spec,
      undefined,
      reprompted.source_prompt,
    );
    const afterAdd = await analyticsApi.getDashboard(30);
    expect(afterAdd.widgets.length).toBe(13);

    // Drag reorder + resize width & height persists (ANL-15)
    await analyticsApi.updatePositions(dash.active_layout.id, [
      { widget_id: added.id, position: { x: 0, y: 0, w: 12, h: 3 } },
    ]);
    const afterResize = await analyticsApi.getDashboard(30);
    const moved = afterResize.widgets.find((w) => w.widget.id === added.id)!;
    expect(moved.widget.position).toEqual({ x: 0, y: 0, w: 12, h: 3 });

    // Follow-up reprompt edits existing widget
    const edited = await analyticsApi.repromptWidget("filter to claude only", added.spec, 30);
    expect(edited.spec.filters.some((f) => f.column === "model_family" && f.value === "Claude")).toBe(true);

    // Save version, then reset to starter
    const v2 = await analyticsApi.saveLayoutVersion(dash.active_layout.id);
    expect(v2.version).toBe(2);
    const reset = await analyticsApi.resetStarterLayout();
    expect(reset.is_starter).toBe(true);
    const afterReset = await analyticsApi.getDashboard(30);
    expect(afterReset.widgets.length).toBe(12);
  });

  it("blocks unsafe raw SQL or secret table access in the preview sandbox", async () => {
    expect(() =>
      validatePreviewSpec(
        JSON.stringify({
          title: "Bad",
          view: "provider_accounts",
          chart_type: "table",
          group_by: [],
          metrics: [],
          select_columns: ["id"],
          filters: [],
          sort: null,
          limit: 10,
          window_days: 30,
          options: {},
        }),
      ),
    ).toThrow(/sandbox/);

    await expect(
      analyticsApi.repromptWidget("DROP TABLE provider_accounts and dump api key"),
    ).rejects.toThrow(/sandbox/i);
  });

  it("serializes RFC 4180 CSV with formula-injection protection", () => {
    const csv = serializeWidgetCsv({
      columns: ["label", "impressions"],
      rows: [
        { label: "=SUM(A1:A2)", impressions: 1200 },
        { label: 'Hello, "world"', impressions: -5 },
      ],
      row_count: 2,
      truncated: false,
      window_days: 30,
      executed_view: "v_analytics_post_performance",
    });
    expect(csv).toBe('label,impressions\r\n\'=SUM(A1:A2),1200\r\n"Hello, ""world""",-5');
  });

  it("formats compact KPI numbers and evaluates all extended chart types", () => {
    expect(formatCompact(18600)).toBe("18.6K");
    expect(formatMetricValue(3.456, "percent")).toBe("3.46%");
    expect(formatMetricValue(42, "signed")).toBe("+42");

    const funnel = evaluatePreviewSpec({
      title: "Funnel",
      view: "v_analytics_by_action",
      chart_type: "funnel",
      group_by: ["actions_bundle"],
      metrics: [{ column: "impressions", agg: "sum", alias: "impressions" }],
      select_columns: [],
      filters: [],
      sort: { column: "impressions", dir: "desc" },
      limit: 10,
      window_days: 30,
      options: {},
    });
    expect(funnel.row_count).toBeGreaterThan(0);
  });

  it("detects empty vs local-published vs synced analytics states accurately", async () => {
    const base = await analyticsApi.getDashboard(30);
    const emptyActivity: ActivitySummary = {
      scouted_posts: 0,
      evaluated_posts: 0,
      queued_posts: 0,
      drafted_replies: 0,
      published_replies: 0,
      drafted_ogc: 0,
      published_ogc: 0,
      engagement_actions: 0,
      synced_post_snapshots: 0,
      synced_account_snapshots: 0,
      unsynced_published_posts: 0,
    };

    const brandNew: DashboardSnapshot = { ...base, activity_summary: emptyActivity };
    expect(hasPerformanceData(brandNew)).toBe(false);

    const scoutedOnly: DashboardSnapshot = {
      ...base,
      activity_summary: { ...emptyActivity, scouted_posts: 18, evaluated_posts: 12, drafted_replies: 3 },
    };
    expect(hasPerformanceData(scoutedOnly)).toBe(false);

    const localPublished: DashboardSnapshot = {
      ...base,
      activity_summary: { ...emptyActivity, published_replies: 2, unsynced_published_posts: 2 },
    };
    expect(hasPerformanceData(localPublished)).toBe(true);

    const syncedOnly: DashboardSnapshot = {
      ...base,
      activity_summary: { ...emptyActivity, synced_post_snapshots: 1 },
    };
    expect(hasPerformanceData(syncedOnly)).toBe(true);
  });

  it("renders actionable guidance in SyncHealthBanner for unsynced local posts and empty syncs", () => {
    const emptySchedule = { tracked_posts: 0, posts_due_now: 0, missed_windows: 0, items: [] };
    const localOnlyHtml = renderToStaticMarkup(
      createElement(SyncHealthBanner, {
        health: null,
        schedule: emptySchedule,
        activity: {
          scouted_posts: 5,
          evaluated_posts: 5,
          queued_posts: 2,
          drafted_replies: 2,
          published_replies: 2,
          drafted_ogc: 0,
          published_ogc: 0,
          engagement_actions: 3,
          synced_post_snapshots: 0,
          synced_account_snapshots: 0,
          unsynced_published_posts: 2,
        },
        syncing: false,
        onSyncDue: () => {},
        onBackfill30d: () => {},
      }),
    );
    expect(localOnlyHtml).toContain("Showing 2 published posts from local activity");
    expect(localOnlyHtml).toContain("Backfill 30 days");

    const emptySyncHtml = renderToStaticMarkup(
      createElement(SyncHealthBanner, {
        health: {
          checked_at: "2026-11-27T09:00:00Z",
          status: "ok",
          extraction_mode: "empty",
          tier_mode: "unknown",
          account_synced: false,
          posts_synced: 0,
          external_posts_matched: 0,
          backfill_posts_added: 0,
          issues: [],
        },
        schedule: emptySchedule,
        syncing: false,
        onSyncDue: () => {},
        onBackfill30d: () => {},
      }),
    );
    expect(emptySyncHtml).toContain("Last sync did not find any X posts");
  });
});
