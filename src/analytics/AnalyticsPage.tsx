import { useCallback, useEffect, useState } from "react";
import { analyticsApi } from "./api";
import { downloadCsv } from "./csv";
import { LayoutControls } from "./LayoutControls";
import { RepromptBuilderModal } from "./RepromptBuilderModal";
import { SyncHealthBanner } from "./SyncHealthBanner";
import type { DashboardSnapshot, DashboardWidget, WidgetPositionUpdate, WidgetSpec } from "./types";
import { WidgetCard } from "./WidgetCard";

const AUTO_CHECK_INTERVAL_MS = 15 * 60 * 1000;

export function AnalyticsPage() {
  const [snapshot, setSnapshot] = useState<DashboardSnapshot | null>(null);
  const [windowDays, setWindowDays] = useState<number>(30);
  const [customizeMode, setCustomizeMode] = useState(false);
  const [syncing, setSyncing] = useState(false);
  const [autoCheckEnabled, setAutoCheckEnabled] = useState(false);
  const [draggedWidgetId, setDraggedWidgetId] = useState<string | null>(null);
  const [dragOverWidgetId, setDragOverWidgetId] = useState<string | null>(null);
  const [lastSyncNote, setLastSyncNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [builderTarget, setBuilderTarget] = useState<{ open: boolean; widget: DashboardWidget | null }>({
    open: false,
    widget: null,
  });

  const loadDashboard = useCallback(async (days: number) => {
    setError(null);
    try {
      const next = await analyticsApi.getDashboard(days);
      setSnapshot(next);
    } catch (e) {
      setError((e as Error).message);
    }
  }, []);

  useEffect(() => {
    void loadDashboard(windowDays);
  }, [loadDashboard, windowDays]);

  const handleSync = useCallback(
    async (backfillDays?: number) => {
      setSyncing(true);
      setError(null);
      setLastSyncNote(null);
      try {
        const report = await analyticsApi.syncStats(backfillDays ? { backfill_days: backfillDays } : {});
        setLastSyncNote(
          `Synced in ${report.duration_ms}ms${report.used_dedicated_tab ? " (dedicated tab)" : ""}${
            report.health.backfill_posts_added > 0 ? ` · +${report.health.backfill_posts_added} backfilled` : ""
          }`,
        );
        await loadDashboard(windowDays);
      } catch (e) {
        setError((e as Error).message);
      } finally {
        setSyncing(false);
      }
    },
    [loadDashboard, windowDays],
  );

  // Foreground-only opt-in checkpoint polling (ANL-05); strictly off by default and cleared on unmount (SEC-04).
  useEffect(() => {
    if (!autoCheckEnabled) return;
    const id = window.setInterval(() => {
      if (!syncing && snapshot && (snapshot.schedule.posts_due_now > 0 || snapshot.schedule.missed_windows > 0)) {
        void handleSync(undefined);
      }
    }, AUTO_CHECK_INTERVAL_MS);
    return () => window.clearInterval(id);
  }, [autoCheckEnabled, syncing, snapshot, handleSync]);

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
      <div className="space-y-4">
        <div className="h-8 w-48 animate-pulse rounded bg-zinc-200 dark:bg-zinc-800" />
        {error ? (
          <div className="rounded-lg border border-rose-200 dark:border-rose-900 bg-rose-50 dark:bg-rose-950/40 p-4 text-sm text-rose-700 dark:text-rose-300">
            {error}
          </div>
        ) : (
          <div className="grid grid-cols-12 gap-4">
            {[0, 1, 2, 3, 4, 5].map((i) => (
              <div
                key={i}
                className="col-span-12 md:col-span-4 h-24 animate-pulse rounded-xl bg-zinc-100 dark:bg-zinc-900"
              />
            ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-7xl space-y-4">
      <LayoutControls
        activeLayout={snapshot.active_layout}
        layouts={snapshot.layouts}
        windowDays={windowDays}
        customizeMode={customizeMode}
        syncing={syncing}
        onWindowChange={setWindowDays}
        onToggleCustomize={() => setCustomizeMode((v) => !v)}
        onOpenBuilder={() => setBuilderTarget({ open: true, widget: null })}
        onSyncStats={(days) => void handleSync(days)}
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

      <SyncHealthBanner
        health={snapshot.last_health}
        schedule={snapshot.schedule}
        lastSyncNote={lastSyncNote}
        syncing={syncing}
        autoCheckEnabled={autoCheckEnabled}
        onToggleAutoCheck={setAutoCheckEnabled}
        onSyncDue={() => void handleSync(undefined)}
      />

      {customizeMode && (
        <div className="rounded-lg border border-sky-200 dark:border-sky-900 bg-sky-50/70 dark:bg-sky-950/30 px-3.5 py-2 text-xs text-sky-800 dark:text-sky-300">
          Customize mode active: drag cards to reorder, drag the bottom-right corner handle (or pick width/height) to resize, or remove widgets. Changes save immediately.
        </div>
      )}

      {error && (
        <div className="rounded-lg border border-rose-200 dark:border-rose-900 bg-rose-50 dark:bg-rose-950/40 px-4 py-2.5 text-xs text-rose-700 dark:text-rose-300">
          {error}
        </div>
      )}

      {snapshot.widgets.length === 0 ? (
        <div className="rounded-xl border border-dashed border-zinc-300 dark:border-zinc-700 p-10 text-center space-y-3">
          <div className="text-sm font-medium text-zinc-700 dark:text-zinc-200">
            This layout has no widgets yet.
          </div>
          <div className="flex justify-center gap-2">
            <button
              type="button"
              onClick={() => setBuilderTarget({ open: true, widget: null })}
              className="rounded-lg bg-sky-600 px-3.5 py-1.5 text-xs font-medium text-white hover:bg-sky-700"
            >
              + Reprompt Widget
            </button>
            <button
              type="button"
              onClick={() => void handleLayoutAction(() => analyticsApi.resetStarterLayout())}
              className="rounded-lg border border-zinc-300 dark:border-zinc-700 px-3.5 py-1.5 text-xs font-medium text-zinc-700 dark:text-zinc-200"
            >
              Restore Starter Layout
            </button>
          </div>
        </div>
      ) : (
        <div className="grid grid-cols-12 gap-4">
          {snapshot.widgets.map((item, idx) => (
            <WidgetCard
              key={item.widget.id}
              item={item}
              customizeMode={customizeMode}
              isDragging={draggedWidgetId === item.widget.id}
              isDropTarget={dragOverWidgetId === item.widget.id && draggedWidgetId !== item.widget.id}
              canMoveUp={idx > 0}
              canMoveDown={idx < snapshot.widgets.length - 1}
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
          ))}
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
    </div>
  );
}
