import type { SyncStatus } from "./types";

function ago(iso: string, now: number): string {
  const ms = now - new Date(iso).getTime();
  if (!Number.isFinite(ms) || ms < 90_000) return "just now";
  const min = Math.round(ms / 60_000);
  if (min < 60) return `${min} min ago`;
  const h = Math.round(min / 60);
  if (h < 48) return `${h} h ago`;
  return `${Math.round(h / 24)} d ago`;
}

/** One calm line about the background sync. Failures are never reported here, only a wait that needs the user. */
export function syncStatusLine(status: SyncStatus | null | undefined, now: number = Date.now()): string {
  if (status?.blocked === "logged_out") return "Sign in to X to keep numbers updating.";
  if (status?.blocked) return "X needs a quick look before numbers can update.";
  if (status?.last_update_at) return `Updated ${ago(status.last_update_at, now)}`;
  return "Numbers update on their own.";
}
