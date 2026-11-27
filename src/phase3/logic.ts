import type { ActionName, StepView } from "./types";

export const DURATION_PRESETS = [5, 10, 15, 30, 60];
export const POST_PRESETS = [25, 50, 100, 200];
export const MAX_CHARS = 280;
export const EXEC_ORDER: ActionName[] = ["like", "bookmark", "follow", "repost", "reply", "quote"];

export const ACTION_META: Record<ActionName, { label: string; done: string; undo: string | null; text: boolean }> = {
  like: { label: "Like", done: "Liked", undo: "Unlike", text: false },
  reply: { label: "Reply", done: "Replied", undo: null, text: true },
  repost: { label: "Repost", done: "Reposted", undo: "Undo repost", text: false },
  quote: { label: "Quote", done: "Quoted", undo: null, text: true },
  bookmark: { label: "Bookmark", done: "Bookmarked", undo: "Remove bookmark", text: false },
  follow: { label: "Follow", done: "Following", undo: "Unfollow", text: false },
};

export const ACTION_ORDER: ActionName[] = ["like", "reply", "repost", "quote", "bookmark", "follow"];

export function isTextAction(a: ActionName): boolean {
  return ACTION_META[a].text;
}

/** Mirrors the backend rule: at most one text action, fixed execution order. */
export function normalizeBundle(actions: ActionName[]): ActionName[] {
  const set = new Set(actions);
  if (set.has("reply") && set.has("quote")) set.delete("quote");
  return EXEC_ORDER.filter((a) => set.has(a));
}

export function toggleInBundle(actions: ActionName[], a: ActionName): ActionName[] {
  if (actions.includes(a)) return actions.filter((x) => x !== a);
  const next = actions.filter((x) => !(isTextAction(a) && isTextAction(x)));
  return normalizeBundle([...next, a]);
}

export type ComposeMode = "reply" | "quote";

export const REPLY_BUNDLE: ActionName[] = ["like", "reply"];
export const QUOTE_BUNDLE: ActionName[] = ["quote"];

/** The action bundle a compose box publishes: a reply goes under the post, a quote is a new post that embeds it. */
export function bundleForMode(mode: ComposeMode): ActionName[] {
  return mode === "quote" ? QUOTE_BUNDLE : REPLY_BUNDLE;
}

export function modeOfBundle(actions: ActionName[]): ComposeMode {
  return actions.includes("quote") ? "quote" : "reply";
}

export type RepostChoice = "repost" | "quote";

/** The two entries X shows when the repost icon is pressed. */
export function repostMenuOptions(reposted: boolean): Array<{ id: RepostChoice; label: string }> {
  return [
    { id: "repost", label: reposted ? "Undo repost" : "Repost" },
    { id: "quote", label: "Quote" },
  ];
}

export const NOT_CONFIRMED = "Not confirmed yet";

export interface PublishLine {
  text: string;
  /** The single follow-up action, if any. */
  action: "Post again" | "Try again" | null;
}

/** One short, calm line for a publish outcome. No advice about checking X: just what happened and one next step. */
export function publishLine(status: string | null | undefined, errorMessage?: string | null): PublishLine | null {
  if (status === "published") return { text: "Posted", action: null };
  if (status === "queued_for_publish" || status === "publishing") return { text: "Checking…", action: null };
  if (status === "unconfirmed" || (status === "failed" && (errorMessage ?? "").startsWith(NOT_CONFIRMED))) return { text: NOT_CONFIRMED, action: "Post again" };
  if (status === "failed") return { text: "Didn't post", action: "Try again" };
  return null;
}

export function bundleText(actions: ActionName[]): ActionName | null {
  return actions.find(isTextAction) ?? null;
}

export function charInfo(text: string, max = MAX_CHARS) {
  const count = [...text].length;
  return { count, max, over: count > max, remaining: max - count };
}

export function editDistance(a: string, b: string): number {
  const x = [...a];
  const y = [...b];
  let prev = Array.from({ length: y.length + 1 }, (_, i) => i);
  for (let i = 1; i <= x.length; i++) {
    const cur = [i];
    for (let j = 1; j <= y.length; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (x[i - 1] === y[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[y.length];
}

export function formatDuration(sec: number): string {
  if (sec < 60) return `${Math.round(sec)}s`;
  const m = Math.floor(sec / 60);
  const s = Math.round(sec % 60);
  if (m < 60) return s ? `${m}m ${s}s` : `${m}m`;
  const h = Math.floor(m / 60);
  return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
}

export function scoreTone(score: number): "high" | "mid" | "low" {
  return score >= 85 ? "high" : score >= 72 ? "mid" : "low";
}

/** "high_saves" / "3.4x-reach" -> "High saves" / "3.4x reach". */
export function factorLabel(flag: string): string {
  const t = flag.replace(/[_-]+/g, " ").trim();
  return t ? t.charAt(0).toUpperCase() + t.slice(1) : "";
}

/** The quiet line under a post: the score and, when known, the strongest signals behind it. */
export function scoreLine(score: number, flags: string[], maxFactors = 2): string {
  const factors = flags.map(factorLabel).filter(Boolean).slice(0, maxFactors);
  return [`Score ${Math.round(score)}`, ...factors].join(" · ");
}

export function compactCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10_000 ? 0 : 1)}K`;
  return String(n);
}

/** X-style age: "now", "5m", "3h", then "Nov 26" (adds the year when it is not the current one). */
export function relativeTime(iso: string | null, nowMs: number = Date.now()): string {
  if (!iso) return "";
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return "";
  const sec = Math.max(0, Math.round((nowMs - t) / 1000));
  if (sec < 45) return "now";
  if (sec < 3600) return `${Math.max(1, Math.round(sec / 60))}m`;
  if (sec < 86_400) return `${Math.round(sec / 3600)}h`;
  const d = new Date(t);
  const month = d.toLocaleString("en-US", { month: "short", timeZone: "UTC" });
  const sameYear = d.getUTCFullYear() === new Date(nowMs).getUTCFullYear();
  return sameYear ? `${month} ${d.getUTCDate()}` : `${month} ${d.getUTCDate()}, ${d.getUTCFullYear()}`;
}

export function stepSummary(steps: StepView[]): { done: number; total: number; blocked: StepView | null } {
  return {
    done: steps.filter((s) => s.status === "done" || s.status === "skipped").length,
    total: steps.length,
    blocked: steps.find((s) => s.status === "failed" || s.status === "uncertain") ?? null,
  };
}

export const REASONS: Array<{ id: string; label: string }> = [
  { id: "off_topic", label: "Off topic" },
  { id: "low_quality", label: "Low quality" },
  { id: "not_my_voice", label: "Not my voice" },
  { id: "already_replied", label: "Already replied" },
  { id: "too_late", label: "Too late" },
  { id: "other", label: "Other" },
];

export interface Shortcut {
  keys: string;
  label: string;
}

export const SHORTCUTS: Shortcut[] = [
  { keys: "Cmd/Ctrl+K", label: "Open the command palette" },
  { keys: "Cmd/Ctrl+Enter", label: "Send the reply for the current post" },
  { keys: "J / K", label: "Next / previous queue item or draft" },
  { keys: "D", label: "Dismiss the current queue item" },
  { keys: "Enter", label: "Expand the selected queue item and draft a reply" },
  { keys: "?", label: "Show this list" },
  { keys: "Esc", label: "Close a dialog or the palette" },
];

export interface KeyLike {
  key: string;
  metaKey?: boolean;
  ctrlKey?: boolean;
  shiftKey?: boolean;
  target?: EventTarget | { tagName?: string; isContentEditable?: boolean } | null;
}

export type ShortcutAction = "palette" | "publish" | "next" | "prev" | "dismiss" | "open" | "help";

function typing(target: KeyLike["target"]): boolean {
  const t = target as { tagName?: string; isContentEditable?: boolean } | null | undefined;
  const tag = (t?.tagName ?? "").toUpperCase();
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT" || !!t?.isContentEditable;
}

/** Maps a key event to an app shortcut. Plain keys never fire while the user is typing. */
export function matchShortcut(e: KeyLike): ShortcutAction | null {
  const mod = !!(e.metaKey || e.ctrlKey);
  const k = e.key.toLowerCase();
  if (mod && k === "k") return "palette";
  if (mod && e.key === "Enter") return "publish";
  if (mod || typing(e.target)) return null;
  if (k === "j") return "next";
  if (k === "k") return "prev";
  if (k === "d") return "dismiss";
  if (e.key === "Enter") return "open";
  if (e.key === "?") return "help";
  return null;
}

export interface PaletteCommand {
  id: string;
  label: string;
  hint?: string;
}

export function filterCommands(commands: PaletteCommand[], query: string): PaletteCommand[] {
  const q = query.trim().toLowerCase();
  if (!q) return commands;
  const parts = q.split(/\s+/);
  return commands.filter((c) => parts.every((p) => c.label.toLowerCase().includes(p)));
}
