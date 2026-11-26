import { useState } from "react";
import type { ScheduleSummary, SyncHealthReport } from "./types";

interface SyncHealthBannerProps {
  health: SyncHealthReport | null;
  schedule: ScheduleSummary;
  lastSyncNote: string | null;
  syncing: boolean;
  autoCheckEnabled: boolean;
  onToggleAutoCheck: (enabled: boolean) => void;
  onSyncDue: () => void;
}

export function SyncHealthBanner({
  health,
  schedule,
  lastSyncNote,
  syncing,
  autoCheckEnabled,
  onToggleAutoCheck,
  onSyncDue,
}: SyncHealthBannerProps) {
  const [expanded, setExpanded] = useState(false);

  const dot = !health ? "bg-zinc-300 dark:bg-zinc-700" : health.status === "ok" ? "bg-emerald-600" : health.status === "warning" ? "bg-amber-500" : "bg-red-600";

  const hasDetails = (health?.issues.length ?? 0) > 0 || schedule.items.length > 0;
  const hasDue = schedule.posts_due_now > 0 || schedule.missed_windows > 0;

  return (
    <div className="text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap items-center gap-2">
          <span className="inline-flex items-center gap-1.5 font-medium text-zinc-900 dark:text-zinc-100">
            <span aria-hidden="true" className={`h-2 w-2 rounded-full ${dot}`} />
            {!health ? "Not synced yet" : health.status === "ok" ? "Ingest healthy" : `Ingest ${health.status}`}
          </span>

          {health && (
            <span className="text-zinc-600 dark:text-zinc-400">
              Mode: <strong className="font-medium text-zinc-800 dark:text-zinc-200">{health.extraction_mode}</strong>
              {" · "}
              Tier: <strong className="font-medium text-zinc-800 dark:text-zinc-200">{health.tier_mode}</strong>
              {" · "}
              {health.posts_synced} posts synced
              {health.external_posts_matched > 0 ? ` (${health.external_posts_matched} external)` : ""}
            </span>
          )}

          {lastSyncNote && <span className="text-zinc-500">{lastSyncNote}</span>}
        </div>

        <div className="flex flex-wrap items-center gap-3 text-zinc-500">
          <span>
            Checkpoints:{" "}
            <strong className="text-zinc-800 dark:text-zinc-200">{schedule.posts_due_now}</strong> due
            {schedule.missed_windows > 0 && (
              <span className="ml-1 text-amber-600 dark:text-amber-400">
                · {schedule.missed_windows} missed backfill
              </span>
            )}
          </span>

          {hasDue && (
            <button
              type="button"
              disabled={syncing}
              onClick={onSyncDue}
              className="rounded border border-zinc-300 px-2 py-0.5 font-medium hover:border-zinc-900 disabled:opacity-40 dark:border-zinc-700"
            >
              Sync Due
            </button>
          )}

          <label
            className="inline-flex items-center gap-1.5 cursor-pointer select-none"
            title="Optional foreground checkpoint sync while this window is open (never runs in background when app is closed)"
          >
            <input
              type="checkbox"
              checked={autoCheckEnabled}
              onChange={(e) => onToggleAutoCheck(e.target.checked)}
              className="rounded border-zinc-300 dark:border-zinc-700"
            />
            <span>Auto-sync due while open</span>
          </label>

          {hasDetails && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="underline-offset-2 hover:text-zinc-950 hover:underline dark:hover:text-zinc-50"
            >
              {expanded ? "Hide details" : `Details${health?.issues.length ? ` (${health.issues.length})` : ""}`}
            </button>
          )}
        </div>
      </div>

      {expanded && (
        <div className="mt-3 space-y-2 border-t border-zinc-200 dark:border-zinc-800 pt-2.5">
          {health?.issues.map((issue, idx) => (
            <div
              key={`${issue.code}-${idx}`}
              className="rounded-md border border-amber-200 dark:border-amber-900/60 bg-amber-50/70 dark:bg-amber-950/30 px-3 py-2"
            >
              <div className="font-medium text-amber-900 dark:text-amber-200">
                [{issue.code}] {issue.message}
              </div>
              <div className="mt-0.5 text-amber-800/80 dark:text-amber-300/80">{issue.hint}</div>
            </div>
          ))}

          {schedule.items.length > 0 && (
            <div className="space-y-1">
              <div className="font-medium text-zinc-600 dark:text-zinc-300">
                Tracked Post Checkpoints (1h / 6h / 24h / 72h / 7d)
              </div>
              <div className="grid gap-1.5 sm:grid-cols-2">
                {schedule.items.slice(0, 6).map((item) => (
                  <div
                    key={item.platform_post_id}
                    className="flex items-center justify-between rounded border border-zinc-200 dark:border-zinc-800 bg-transparent px-2.5 py-1.5"
                  >
                    <span className="font-mono text-[11px] text-zinc-700 dark:text-zinc-300 truncate">
                      {item.draft_kind} #{item.platform_post_id}
                    </span>
                    <div className="flex items-center gap-1">
                      {item.checkpoints.map((cp) => (
                        <span
                          key={cp.label}
                          title={`${cp.label}: ${cp.state}`}
                          className={`rounded px-1.5 py-0.5 text-[10px] ${
                            cp.state === "captured"
                              ? "bg-emerald-100 text-emerald-700 dark:bg-emerald-950 dark:text-emerald-300"
                              : cp.state === "due_now"
                                ? "bg-sky-100 text-sky-700 dark:bg-sky-950 dark:text-sky-300"
                                : cp.state === "missed_backfill"
                                  ? "bg-amber-100 text-amber-700 dark:bg-amber-950 dark:text-amber-300"
                                  : "bg-zinc-100 text-zinc-400 dark:bg-zinc-800"
                          }`}
                        >
                          {cp.label}
                        </span>
                      ))}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
