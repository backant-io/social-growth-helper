import { btn, btnGhost } from "../components/ui";
import type { ActivitySummary, ScheduleSummary, SyncHealthReport } from "./types";

interface SyncHealthBannerProps {
  health: SyncHealthReport | null;
  schedule: ScheduleSummary;
  activity?: ActivitySummary;
  syncing: boolean;
  onSyncDue: () => void;
  onBackfill30d?: () => void;
}

/** Surfaces sync issues, due checkpoints, or local-only published posts waiting for X metrics. */
export function SyncHealthBanner({
  health,
  schedule,
  activity,
  syncing,
  onSyncDue,
  onBackfill30d,
}: SyncHealthBannerProps) {
  const issue = health?.issues[0];
  const due = schedule.posts_due_now + schedule.missed_windows;
  const unsyncedPublished = activity?.unsynced_published_posts ?? 0;
  const emptyLastSync = health !== null && !health.account_synced && health.posts_synced === 0;

  if (!issue && due === 0 && unsyncedPublished === 0 && !emptyLastSync) {
    return null;
  }

  let toneClass = "border-zinc-400";
  let content: React.ReactNode;
  let primaryLabel = "Sync stats";
  let showBackfill = false;

  if (issue) {
    toneClass = "border-amber-500";
    primaryLabel = "Try again";
    showBackfill = issue.code === "account_extraction_empty" || issue.code === "missing_handle_for_backfill";
    content = (
      <>
        {issue.message} <span className="text-zinc-500">{issue.hint}</span>
      </>
    );
  } else if (due > 0) {
    primaryLabel = "Sync now";
    content = `${due} post${due === 1 ? "" : "s"} ready for a stats check from X.`;
  } else if (unsyncedPublished > 0) {
    showBackfill = true;
    content = (
      <>
        Showing {unsyncedPublished} published post{unsyncedPublished === 1 ? "" : "s"} from local activity.{" "}
        <span className="text-zinc-500">
          Sync with X to pull impressions, engagement rate, and follower counts.
        </span>
      </>
    );
  } else {
    toneClass = "border-amber-500";
    showBackfill = true;
    content = (
      <>
        Last sync did not find any X posts or account metrics yet.{" "}
        <span className="text-zinc-500">
          Make sure you are signed in to X, or run a 30-day backfill to import recent posts.
        </span>
      </>
    );
  }

  return (
    <div
      role="status"
      className={`flex flex-wrap items-center justify-between gap-3 border-l-2 bg-zinc-100/70 py-2 pr-3 pl-3.5 text-sm dark:bg-zinc-900/60 ${toneClass}`}
    >
      <span>{content}</span>
      <div className="flex items-center gap-2">
        {showBackfill && onBackfill30d && (
          <button type="button" disabled={syncing} onClick={onBackfill30d} className={btnGhost}>
            Backfill 30 days
          </button>
        )}
        <button type="button" disabled={syncing} onClick={onSyncDue} className={btn}>
          {syncing ? "Syncing..." : primaryLabel}
        </button>
      </div>
    </div>
  );
}
