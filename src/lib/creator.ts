import type { DraftStatus, FormatBlueprint } from "./types";

const URL_RE = /https?:\/\/\S+/g;
const LINK_WEIGHT = 23;

/** Characters X counts for a post: every link is 23, everything else one per character. */
export function postLength(text: string): number {
  let links = 0;
  const rest = text.replace(URL_RE, () => {
    links += 1;
    return "";
  });
  return Array.from(rest).length + links * LINK_WEIGHT;
}

export function slotKeys(skeleton: string): string[] {
  const keys: string[] = [];
  for (const m of skeleton.matchAll(/\{([a-z0-9_]+)\}/g)) if (!keys.includes(m[1])) keys.push(m[1]);
  return keys;
}

/** Fills `{slot}` placeholders; unfilled ones stay visible so nothing half-written reads as finished. */
export function fillSkeleton(skeleton: string, values: Record<string, string>): string {
  return skeleton.replace(/\{([a-z0-9_]+)\}/g, (all, key: string) => (values[key]?.trim() ? values[key].trim() : all));
}

export function hasPlaceholders(text: string): boolean {
  return /\{[a-z0-9_]+\}/.test(text);
}

export function percent(v: number | null): string {
  return v === null ? "n/a" : `${Math.round(v * 100)}%`;
}

/** One absolute path per line, tolerating quotes and file:// prefixes from a file manager. */
export function parsePaths(input: string): string[] {
  const seen = new Set<string>();
  for (const raw of input.split(/\r?\n/)) {
    let p = raw.trim().replace(/^['"]|['"]$/g, "");
    if (p.startsWith("file://")) p = decodeURIComponent(p.slice("file://".length));
    if (p) seen.add(p);
  }
  return [...seen];
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function similarityNote(score: number | null, threshold: number): { tone: "ok" | "warn" | "none"; text: string } {
  if (score === null) return { tone: "none", text: "No source to compare against." };
  const pct = Math.round(score * 100);
  return score >= threshold
    ? { tone: "warn", text: `${pct}% overlap with the viral post it follows. Rewrite it in your own words before posting.` }
    : { tone: "ok", text: `${pct}% overlap with the source post. Original enough.` };
}

const STATUS_TEXT: Record<DraftStatus, string> = {
  variant: "Variant",
  draft: "Draft",
  publishing: "Publishing",
  published: "Published",
  failed: "Not posted",
  unconfirmed: "Check X",
  discarded: "Discarded",
};

export function statusLabel(s: DraftStatus): string {
  return STATUS_TEXT[s];
}

export function canEdit(s: DraftStatus): boolean {
  return s === "variant" || s === "draft" || s === "failed";
}

export function blankBlueprint(): FormatBlueprint {
  return { name: "", archetype: "other", hook_formula: "", structural_pattern: "", skeleton: "", slots: [], media_hint: "" };
}

/** Slots that match the skeleton's placeholders, keeping labels the user already wrote. */
export function syncSlots(skeleton: string, existing: FormatBlueprint["slots"]): FormatBlueprint["slots"] {
  return slotKeys(skeleton).map((key) => existing.find((s) => s.key === key) ?? { key, label: key.replace(/_/g, " "), hint: "" });
}
