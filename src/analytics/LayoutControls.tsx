import { useState } from "react";
import type { DashboardLayout } from "./types";

interface LayoutControlsProps {
  activeLayout: DashboardLayout;
  layouts: DashboardLayout[];
  windowDays: number;
  customizeMode: boolean;
  syncing: boolean;
  onWindowChange: (days: number) => void;
  onToggleCustomize: () => void;
  onOpenBuilder: () => void;
  onSyncStats: (backfillDays?: number) => void;
  onSwitchLayout: (layoutId: string) => void;
  onSaveVersion: () => void;
  onRenameLayout: (newName: string) => void;
  onDuplicateLayout: (newName: string) => void;
  onDeleteLayout: () => void;
  onResetStarter: () => void;
}

export function LayoutControls({
  activeLayout,
  layouts,
  windowDays,
  customizeMode,
  syncing,
  onWindowChange,
  onToggleCustomize,
  onOpenBuilder,
  onSyncStats,
  onSwitchLayout,
  onSaveVersion,
  onRenameLayout,
  onDuplicateLayout,
  onDeleteLayout,
  onResetStarter,
}: LayoutControlsProps) {
  const [menuOpen, setMenuOpen] = useState(false);

  function promptRename() {
    const next = window.prompt("Rename dashboard layout:", activeLayout.name);
    if (next && next.trim()) onRenameLayout(next.trim());
    setMenuOpen(false);
  }

  function promptDuplicate() {
    const next = window.prompt("Name for duplicated layout:", `${activeLayout.name} Copy`);
    if (next && next.trim()) onDuplicateLayout(next.trim());
    setMenuOpen(false);
  }

  return (
    <div className="flex flex-wrap items-start justify-between gap-3">
      <div>
        <h1 className="text-xl font-semibold tracking-tight">Analytics</h1>
        <p className="mt-1 max-w-2xl text-sm text-zinc-600 dark:text-zinc-400">
          How your posts and replies are doing. Numbers are read from your X window when you sync, and stay on this computer.
        </p>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        {/* Time window selector */}
        <div className="inline-flex rounded-lg border border-zinc-200 dark:border-zinc-800 bg-zinc-50 dark:bg-zinc-900 p-0.5 text-xs">
          {[7, 30, 90].map((d) => (
            <button
              key={d}
              type="button"
              onClick={() => onWindowChange(d)}
              className={`rounded-md px-2.5 py-1 font-medium transition-colors ${
                windowDays === d
                  ? "bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 shadow-xs"
                  : "text-zinc-500 hover:text-zinc-800 dark:hover:text-zinc-200"
              }`}
            >
              {d}d
            </button>
          ))}
        </div>

        {/* Layout & version switcher */}
        <div className="relative flex items-center gap-1">
          <select
            aria-label="Dashboard layout version"
            value={activeLayout.id}
            onChange={(e) => onSwitchLayout(e.target.value)}
            className="rounded-lg border border-zinc-200 dark:border-zinc-800 bg-transparent px-2.5 py-1.5 text-xs font-medium text-zinc-800 dark:text-zinc-200"
          >
            {layouts.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name} (v{l.version})
              </option>
            ))}
          </select>

          <button
            type="button"
            onClick={() => setMenuOpen((v) => !v)}
            className="rounded-lg border border-zinc-200 dark:border-zinc-800 bg-transparent px-2.5 py-1.5 text-xs text-zinc-600 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-800"
          >
            Layouts ▾
          </button>

          {menuOpen && (
            <div className="absolute right-0 top-9 z-30 w-48 rounded-lg border border-zinc-200 dark:border-zinc-800 bg-transparent py-1 text-xs shadow-lg">
              <button
                type="button"
                onClick={() => {
                  onSaveVersion();
                  setMenuOpen(false);
                }}
                className="block w-full px-3 py-1.5 text-left text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                Save New Version (v{activeLayout.version + 1})
              </button>
              <button
                type="button"
                onClick={promptRename}
                className="block w-full px-3 py-1.5 text-left text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                Rename Layout…
              </button>
              <button
                type="button"
                onClick={promptDuplicate}
                className="block w-full px-3 py-1.5 text-left text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                Duplicate Layout…
              </button>
              <button
                type="button"
                onClick={() => {
                  onResetStarter();
                  setMenuOpen(false);
                }}
                className="block w-full px-3 py-1.5 text-left text-zinc-700 dark:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800"
              >
                Reset to Starter Layout
              </button>
              {layouts.length > 1 && (
                <button
                  type="button"
                  onClick={() => {
                    onDeleteLayout();
                    setMenuOpen(false);
                  }}
                  className="block w-full border-t border-zinc-100 dark:border-zinc-800 px-3 py-1.5 text-left text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950"
                >
                  Delete This Layout
                </button>
              )}
            </div>
          )}
        </div>

        <button
          type="button"
          onClick={onToggleCustomize}
          className={`rounded-lg border px-3 py-1.5 text-xs font-medium transition-colors ${
            customizeMode
              ? "border-sky-500 bg-sky-50 text-sky-700 dark:bg-sky-950 dark:text-sky-300"
              : "border-zinc-200 dark:border-zinc-800 bg-transparent text-zinc-700 dark:text-zinc-200 hover:bg-zinc-50 dark:hover:bg-zinc-800"
          }`}
        >
          {customizeMode ? "Done Editing" : "Customize"}
        </button>

        <button
          type="button"
          onClick={onOpenBuilder}
          className="rounded-lg border border-sky-200 dark:border-sky-900 bg-sky-50 dark:bg-sky-950/60 px-3 py-1.5 text-xs font-medium text-sky-700 dark:text-sky-300 hover:bg-sky-100 dark:hover:bg-sky-950"
        >
          + Reprompt Widget
        </button>

        <button
          type="button"
          disabled={syncing}
          onClick={() => onSyncStats(undefined)}
          className="rounded-lg bg-zinc-900 dark:bg-zinc-100 px-3.5 py-1.5 text-xs font-medium text-white dark:text-zinc-900 hover:opacity-90 disabled:opacity-50"
        >
          {syncing ? "Syncing…" : "Sync Stats"}
        </button>

        <button
          type="button"
          disabled={syncing}
          onClick={() => onSyncStats(30)}
          title="Backfill last 30 days of profile posts (flags external posts)"
          className="rounded-lg border border-zinc-200 dark:border-zinc-800 bg-transparent px-2.5 py-1.5 text-xs text-zinc-600 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-800 disabled:opacity-50"
        >
          Backfill 30d
        </button>
      </div>
    </div>
  );
}
