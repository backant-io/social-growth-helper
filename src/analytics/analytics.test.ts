import { beforeEach, describe, expect, it } from "vitest";
import { analyticsApi, evaluatePreviewSpec, resetPreviewStore, validatePreviewSpec } from "./api";
import { formatCompact, formatMetricValue } from "./charts/ChartPrimitives";
import { serializeWidgetCsv } from "./csv";

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
});
