import { btn } from "../components/ui";
import type { ScheduleSummary, SyncHealthReport } from "./types";

interface SyncHealthBannerProps {
  health: SyncHealthReport | null;
  schedule: ScheduleSummary;
  syncing: boolean;
  onSyncDue: () => void;
}

/** Only speaks up when stats need attention: a sync problem, or checkpoints that are waiting. */
export function SyncHealthBanner({ health, schedule, syncing, onSyncDue }: SyncHealthBannerProps) {
  const issue = health?.issues[0];
  const due = schedule.posts_due_now + schedule.missed_windows;
  if (!issue && due === 0) return null;

  return (
    <div role="status" className={`flex flex-wrap items-center justify-between gap-3 border-l-2 bg-zinc-100/70 py-2 pr-3 pl-3.5 text-sm dark:bg-zinc-900/60 ${issue ? "border-amber-500" : "border-zinc-400"}`}>
      <span>
        {issue ? (
          <>
            {issue.message} <span className="text-zinc-500">{issue.hint}</span>
          </>
        ) : (
          `${due} post${due === 1 ? "" : "s"} ready for a stats check.`
        )}
      </span>
      {due > 0 && (
        <button type="button" disabled={syncing} onClick={onSyncDue} className={btn}>
          Sync now
        </button>
      )}
    </div>
  );
}
