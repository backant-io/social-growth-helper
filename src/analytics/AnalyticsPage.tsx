import { useCallback, useEffect, useRef, useState } from "react";
import { btn, btnGhost, btnPrimary, Disclosure, EmptyBlock, LoadingBlock, Notice, Page, PageHeader } from "../components/ui";
import { listen } from "../lib/api";
import type { SectionId } from "../lib/types";
import { analyticsApi } from "./api";
import { formatCompact } from "./charts/ChartPrimitives";
import { downloadCsv } from "./csv";
import { LayoutControls } from "./LayoutControls";
import { RepromptBuilderModal } from "./RepromptBuilderModal";
import { syncStatusLine } from "./syncStatus";
import type {
  ActivitySummary,
  DashboardSnapshot,
  DashboardWidget,
  EvaluatedWidget,
  PeriodOverview,
  WidgetPositionUpdate,
  WidgetSpec,
} from "./types";
import { WidgetCard } from "./WidgetCard";

interface AnalyticsPageProps {
  onNavigate?: (section: SectionId) => void;
}

export function hasPerformanceData(snapshot: DashboardSnapshot): boolean {
  const a = snapshot.activity_summary;
  if (a) {
    if (
      a.synced_post_snapshots > 0 ||
      a.synced_account_snapshots > 0 ||
      a.published_replies > 0 ||
      a.published_ogc > 0
    ) {
      return true;
    }
    return false;
  }
  if (snapshot.period_overview) return true;
  return snapshot.widgets.some((item) => {
    if (!item.data || item.data.rows.length === 0) return false;
    if (item.widget.spec.chart_type !== "kpi") return true;
    const row = item.data.rows[0];
    return Object.values(row).some((v) => typeof v === "number" && v !== 0);
  });
}

function hasLocalActivity(a?: ActivitySummary): boolean {
  if (!a) return false;
  return (
    a.scouted_posts > 0 ||
    a.evaluated_posts > 0 ||
    a.drafted_replies > 0 ||
    a.drafted_ogc > 0 ||
    a.engagement_actions > 0
  );
}

function CollectedActivity({ activity }: { activity: ActivitySummary }) {
  const items = [
    { label: "Scouted posts", value: activity.scouted_posts },
    { label: "Evaluated", value: activity.evaluated_posts },
    { label: "Queued", value: activity.queued_posts },
    { label: "Reply drafts", value: activity.drafted_replies },
    { label: "Creator drafts", value: activity.drafted_ogc },
    { label: "Likes", value: activity.likes_given },
    { label: "Saved", value: activity.bookmarks_saved },
    { label: "Reposts", value: activity.reposts_made },
    { label: "Follows", value: activity.follows_made },
  ];

  return (
    <Disclosure summary="Your activity">
      <dl className="grid gap-x-10 sm:grid-cols-2">
        {items.map((stat) => (
          <div
            key={stat.label}
            className="flex items-baseline justify-between border-b border-zinc-200 py-2 text-sm dark:border-zinc-800"
          >
            <dt className="text-zinc-600 dark:text-zinc-400">{stat.label}</dt>
            <dd className="font-mono tabular-nums">{stat.value.toLocaleString()}</dd>
          </div>
        ))}
      </dl>
    </Disclosure>
  );
}

/** One quiet line under the title: X period totals, then when the numbers last refreshed. */
export function headerLine(
  overview: PeriodOverview | null | undefined,
  hasData: boolean,
  syncLine: string | null,
): string {
  const parts: Array<string | null> = [];
  if (hasData && overview) {
    if (overview.engagements != null) parts.push(`${formatCompact(overview.engagements)} engagements`);
    if (overview.profile_visits != null) parts.push(`${formatCompact(overview.profile_visits)} profile visits`);
  }
  parts.push(syncLine);
  return parts.filter(Boolean).join(" · ");
}

type WidgetRun =
  | { kind: "kpis"; items: Array<{ item: EvaluatedWidget; index: number }> }
  | { kind: "chart"; item: EvaluatedWidget; index: number }
  | { kind: "waiting"; key: string };

/** KPI tiles that read X's account-level period totals (the `analytics_period_snapshots` row). */
export function isPeriodTotalKpi(item: EvaluatedWidget): boolean {
  return item.widget.spec.chart_type === "kpi" && item.widget.spec.view === "v_analytics_account_overview";
}

/**
 * Groups consecutive KPI widgets so they read as one row of numbers, keeping the saved order.
 * Without a period snapshot for the window, the period-total KPIs collapse into one waiting run.
 */
export function groupWidgets(widgets: EvaluatedWidget[], hasPeriodSnapshot = true): WidgetRun[] {
  const runs: WidgetRun[] = [];
  let waitingAdded = false;
  widgets.forEach((item, index) => {
    if (!hasPeriodSnapshot && isPeriodTotalKpi(item)) {
      if (!waitingAdded) {
        runs.push({ kind: "waiting", key: item.widget.id });
        waitingAdded = true;
      }
      return;
    }
    if (item.widget.spec.chart_type === "kpi") {
      const last = runs[runs.length - 1];
      if (last && last.kind === "kpis") last.items.push({ item, index });
      else runs.push({ kind: "kpis", items: [{ item, index }] });
    } else {
      runs.push({ kind: "chart", item, index });
    }
  });
  return runs;
}

function WaitingForCapture({ windowDays }: { windowDays: number }) {
  return (
    <div className="col-span-12 space-y-1" role="status">
      <p className="text-sm font-medium">Waiting for first X Analytics capture</p>
      <p className="text-sm text-zinc-500">
        Your {windowDays}-day totals appear here on their own. Nothing to do.
      </p>
    </div>
  );
}

export function AnalyticsPage({ onNavigate }: AnalyticsPageProps) {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [windowDays, setWindowDays] = useState<number>(30);
  const [customizeMode, setCustomizeMode] = useState(false);
  const [showEmptyWidgets, setShowEmptyWidgets] = useState(false);
  const [draggedWidgetId, setDraggedWidgetId] = useState<string | null>(null);
  const [dragOverWidgetId, setDragOverWidgetId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [builderTarget, setBuilderTarget] = useState<{ open: boolean; widget: DashboardWidget | null }>({
    open: false,
    widget: null,
  });

  const requestIdRef = useRef(0);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const loadDashboard = useCallback(async (days: number) => {
    const reqId = ++requestIdRef.current;
    setError(null);
    try {
      const next = await analyticsApi.getDashboard(days);
      if (!mountedRef.current || reqId !== requestIdRef.current) return;
      setSnapshot(next);
    } catch (e) {
      if (!mountedRef.current || reqId !== requestIdRef.current) return;
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void loadDashboard(windowDays);
  }, [loadDashboard, windowDays]);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const unlisteners: Array<() => void> = [];
    const refresh = () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        if (active) void loadDashboard(windowDays);
      }, 800);
    };
    for (const name of ["publish:update", "analytics:updated"]) {
      void listen(name, refresh).then((fn) => {
        if (active) unlisteners.push(fn);
        else fn();
      });
    }
    return () => {
      active = false;
      if (timer) clearTimeout(timer);
      unlisteners.forEach((fn) => fn());
    };
  }, [loadDashboard, windowDays]);

  async function handleAcceptWidget(spec: WidgetSpec, sourcePrompt: string) {
    if (!snapshot) return;
    if (builderTarget.widget) {
      await analyticsApi.updateWidget(
        snapshot.active_layout.id,
        builderTarget.widget.id,
        spec,
        builderTarget.widget.position,
        sourcePrompt,
      );
    } else {
      await analyticsApi.addWidget(snapshot.active_layout.id, spec, undefined, sourcePrompt);
    }
    setShowEmptyWidgets(true);
    await loadDashboard(windowDays);
  }

  async function handleExportCsv(widget: DashboardWidget) {
    try {
      const csv = await analyticsApi.exportWidgetCsv(widget.spec, windowDays);
      downloadCsv(`${widget.title}-${windowDays}d`, csv);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  async function handleRemoveWidget(widgetId: string) {
    if (!snapshot) return;
    try {
      await analyticsApi.removeWidget(snapshot.active_layout.id, widgetId);
      await loadDashboard(windowDays);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  async function persistOrder(reordered: DashboardWidget[]) {
    if (!snapshot) return;
    const updates: WidgetPositionUpdate[] = reordered.map((w, idx) => ({
      widget_id: w.id,
      position: { ...w.position, y: idx, x: 0 },
    }));
    try {
      await analyticsApi.updatePositions(snapshot.active_layout.id, updates);
      await loadDashboard(windowDays);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  async function handleMoveWidget(index: number, delta: -1 | 1) {
    if (!snapshot) return;
    const target = index + delta;
    if (target < 0 || target >= snapshot.widgets.length) return;
    const reordered = snapshot.widgets.map((w) => w.widget);
    const [moved] = reordered.splice(index, 1);
    reordered.splice(target, 0, moved);
    await persistOrder(reordered);
  }

  async function handleDropWidget(targetWidgetId: string) {
    const fromId = draggedWidgetId;
    setDraggedWidgetId(null);
    setDragOverWidgetId(null);
    if (!snapshot || !fromId || fromId === targetWidgetId) return;
    const list = snapshot.widgets.map((w) => w.widget);
    const fromIdx = list.findIndex((w) => w.id === fromId);
    const toIdx = list.findIndex((w) => w.id === targetWidgetId);
    if (fromIdx < 0 || toIdx < 0) return;
    const [moved] = list.splice(fromIdx, 1);
    list.splice(toIdx, 0, moved);
    await persistOrder(list);
  }

  async function handleResizeWidget(widget: DashboardWidget, w: number, h: number) {
    if (!snapshot) return;
    try {
      await analyticsApi.updatePositions(snapshot.active_layout.id, [
        { widget_id: widget.id, position: { ...widget.position, w, h } },
      ]);
      await loadDashboard(windowDays);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  async function handleLayoutAction(fn: () => Promise<unknown>) {
    setError(null);
    try {
      await fn();
      await loadDashboard(windowDays);
    } catch (e) {
      setError((e as Error).message);
    }
  }

  if (!snapshot) {
    return (
      <Page wide>
        <PageHeader title="Analytics" />
        {error ? (
          <div className="space-y-3">
            <Notice tone="error">{error}</Notice>
            <button type="button" onClick={() => void loadDashboard(windowDays)} className={btn}>
              Retry
            </button>
          </div>
        ) : (
          <LoadingBlock label="Loading analytics" rows={4} />
        )}
      </Page>
    );
  }

  function renderWidget(item: EvaluatedWidget, idx: number, tile: boolean) {
    return (
      <WidgetCard
        key={item.widget.id}
        item={item}
        customizeMode={customizeMode}
        tile={tile}
        isDragging={draggedWidgetId === item.widget.id}
        isDropTarget={dragOverWidgetId === item.widget.id && draggedWidgetId !== item.widget.id}
        canMoveUp={idx > 0}
        canMoveDown={idx < snapshot!.widgets.length - 1}
        onReprompt={() => setBuilderTarget({ open: true, widget: item.widget })}
        onExportCsv={() => void handleExportCsv(item.widget)}
        onRemove={() => void handleRemoveWidget(item.widget.id)}
        onMove={(delta) => void handleMoveWidget(idx, delta)}
        onResize={(w, h) => void handleResizeWidget(item.widget, w, h)}
        onDragStart={() => setDraggedWidgetId(item.widget.id)}
        onDragOver={() => setDragOverWidgetId(item.widget.id)}
        onDrop={() => void handleDropWidget(item.widget.id)}
        onDragEnd={() => {
          setDraggedWidgetId(null);
          setDragOverWidgetId(null);
        }}
      />
    );
  }

  const hasData = hasPerformanceData(snapshot);
  const localActivity = hasLocalActivity(snapshot.activity_summary);
  const showGettingStarted = !hasData && !customizeMode && !showEmptyWidgets;
  const hasPeriodSnapshot = snapshot.period_overview != null;
  const syncBlocked = Boolean(snapshot.sync_status?.blocked);
  const description = headerLine(
    snapshot.period_overview,
    hasData,
    syncBlocked ? null : syncStatusLine(snapshot.sync_status),
  );

  return (
    <Page wide>
      <LayoutControls
        activeLayout={snapshot.active_layout}
        layouts={snapshot.layouts}
        windowDays={windowDays}
        customizeMode={customizeMode}
        description={description || undefined}
        onWindowChange={setWindowDays}
        onToggleCustomize={() => setCustomizeMode((v) => !v)}
        onOpenBuilder={() => setBuilderTarget({ open: true, widget: null })}
        onSwitchLayout={(id) => void handleLayoutAction(() => analyticsApi.switchLayout(id))}
        onSaveVersion={() =>
          void handleLayoutAction(() => analyticsApi.saveLayoutVersion(snapshot.active_layout.id))
        }
        onRenameLayout={(name) =>
          void handleLayoutAction(() => analyticsApi.renameLayout(snapshot.active_layout.id, name))
        }
        onDuplicateLayout={(name) =>
          void handleLayoutAction(() => analyticsApi.duplicateLayout(snapshot.active_layout.id, name))
        }
        onDeleteLayout={() =>
          void handleLayoutAction(() => analyticsApi.deleteLayout(snapshot.active_layout.id))
        }
        onResetStarter={() => void handleLayoutAction(() => analyticsApi.resetStarterLayout())}
      />

      {syncBlocked && <Notice tone="warn">{syncStatusLine(snapshot.sync_status)}</Notice>}

      {customizeMode && (
        <Notice tone="info">Drag to reorder or resize. Changes save as you go.</Notice>
      )}

      {error && <Notice tone="error">{error}</Notice>}

      {showGettingStarted ? (
        <div className="space-y-6">
          <EmptyBlock
            title="Nothing here yet"
            action={
              <div className="flex flex-wrap items-center justify-center gap-2">
                {onNavigate && (
                  <>
                    <button type="button" onClick={() => onNavigate("scout")} className={btnPrimary}>
                      Open Scout
                    </button>
                    <button type="button" onClick={() => onNavigate("creator")} className={btn}>
                      Open Creator
                    </button>
                  </>
                )}
                <button type="button" onClick={() => setShowEmptyWidgets(true)} className={btnGhost}>
                  Show starter widgets
                </button>
              </div>
            }
          >
            Your numbers appear here after your first reply or post.
          </EmptyBlock>
          {localActivity && snapshot.activity_summary && (
            <CollectedActivity activity={snapshot.activity_summary} />
          )}
        </div>
      ) : snapshot.widgets.length === 0 ? (
        <EmptyBlock
          title="No widgets yet"
          action={
            <>
              <button
                type="button"
                onClick={() => setBuilderTarget({ open: true, widget: null })}
                className={btnPrimary}
              >
                Add a widget
              </button>
              <button
                type="button"
                onClick={() => void handleLayoutAction(() => analyticsApi.resetStarterLayout())}
                className={btn}
              >
                Restore starter layout
              </button>
            </>
          }
        >
          Add a widget to see your numbers here.
        </EmptyBlock>
      ) : (
        <div className="space-y-4">
          {!hasData && showEmptyWidgets && !customizeMode && (
            <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-zinc-500">
              <span>Previewing the starter layout.</span>
              <button
                type="button"
                onClick={() => setShowEmptyWidgets(false)}
                className="underline hover:text-zinc-900 dark:hover:text-zinc-100"
              >
                Back
              </button>
            </div>
          )}

          <div className="grid grid-cols-12 gap-x-10 gap-y-10 [&>*]:min-w-0">
            {(customizeMode
              ? snapshot.widgets.map((item, index) => ({ kind: "chart" as const, item, index }))
              : groupWidgets(snapshot.widgets, hasPeriodSnapshot)
            ).map((run) =>
              run.kind === "waiting" ? (
                <WaitingForCapture key={run.key} windowDays={windowDays} />
              ) : run.kind === "kpis" ? (
                <div
                  key={run.items[0].item.widget.id}
                  className="col-span-12 grid grid-cols-2 gap-x-6 gap-y-8 sm:grid-cols-3 sm:gap-x-10"
                >
                  {run.items.map(({ item, index }) => renderWidget(item, index, true))}
                </div>
              ) : (
                renderWidget(run.item, run.index, false)
              ),
            )}
          </div>
        </div>
      )}

      {!showGettingStarted && !customizeMode && localActivity && snapshot.activity_summary && (
        <CollectedActivity activity={snapshot.activity_summary} />
      )}

      {!customizeMode && (
        <div className="flex justify-end">
          <button type="button" onClick={() => setCustomizeMode(true)} className={btnGhost}>
            Customize
          </button>
        </div>
      )}

      {builderTarget.open && (
        <RepromptBuilderModal
          existingWidget={builderTarget.widget}
          windowDays={windowDays}
          builderRoute={snapshot.builder_route}
          onAccept={handleAcceptWidget}
          onClose={() => setBuilderTarget({ open: false, widget: null })}
        />
      )}
    </Page>
  );
}
