import { useState } from "react";
import {
  btn,
  btnGhost,
  input,
  PageHeader,
  Segmented,
} from "../components/ui";
import type { DashboardLayout } from "./types";

interface LayoutControlsProps {
  activeLayout: DashboardLayout;
  layouts: DashboardLayout[];
  windowDays: number;
  customizeMode: boolean;
  onWindowChange: (days: number) => void;
  onToggleCustomize: () => void;
  onOpenBuilder: () => void;
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
  onWindowChange,
  onToggleCustomize,
  onOpenBuilder,
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
    const next = window.prompt(
      "Name for duplicated layout:",
      `${activeLayout.name} Copy`,
    );
    if (next && next.trim()) onDuplicateLayout(next.trim());
    setMenuOpen(false);
  }

  const item =
    "block w-full px-3 py-1.5 text-left hover:bg-zinc-100 dark:hover:bg-zinc-900";
  return (
    <div className="space-y-4">
      <PageHeader
        title="Analytics"
        description="How your posts and replies are doing."
        actions={
          <>
            <Segmented
              label="Time window"
              value={windowDays}
              options={[7, 30].map((d) => ({ value: d, label: `${d}d` }))}
              onChange={onWindowChange}
            />
            <button
              type="button"
              onClick={onToggleCustomize}
              aria-pressed={customizeMode}
              className={customizeMode ? btn : btnGhost}
            >
              {customizeMode ? "Done" : "Customize"}
            </button>
          </>
        }
      />

      {customizeMode && (
        <div className="flex flex-wrap items-center gap-2 border-y border-zinc-200 py-2 dark:border-zinc-800">
          <select
            aria-label="Dashboard layout version"
            value={activeLayout.id}
            onChange={(e) => onSwitchLayout(e.target.value)}
            className={`${input} py-1 text-xs`}
          >
            {layouts.map((l) => (
              <option key={l.id} value={l.id}>
                {l.name} (v{l.version})
              </option>
            ))}
          </select>
          <button type="button" onClick={onOpenBuilder} className={btnGhost}>
            Add widget
          </button>
          <div className="relative ml-auto">
            <button
              type="button"
              aria-expanded={menuOpen}
              onClick={() => setMenuOpen((v) => !v)}
              className={btnGhost}
            >
              More
            </button>
            {menuOpen && (
              <div className="absolute right-0 top-9 z-30 w-52 overflow-hidden rounded-lg border border-zinc-200 bg-[var(--bg)] py-1 text-xs shadow-xl dark:border-zinc-800">
                <button
                  type="button"
                  className={item}
                  onClick={() => {
                    onSaveVersion();
                    setMenuOpen(false);
                  }}
                >
                  Save new version (v{activeLayout.version + 1})
                </button>
                <button type="button" className={item} onClick={promptRename}>
                  Rename layout
                </button>
                <button
                  type="button"
                  className={item}
                  onClick={promptDuplicate}
                >
                  Duplicate layout
                </button>
                <button
                  type="button"
                  className={item}
                  onClick={() => {
                    onResetStarter();
                    setMenuOpen(false);
                  }}
                >
                  Reset to starter layout
                </button>
                {layouts.length > 1 && (
                  <button
                    type="button"
                    className={`${item} border-t border-zinc-200 text-red-600 dark:border-zinc-800 dark:text-red-400`}
                    onClick={() => {
                      onDeleteLayout();
                      setMenuOpen(false);
                    }}
                  >
                    Delete this layout
                  </button>
                )}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
