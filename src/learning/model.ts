import type { DiffLine, Insight, Patch, PatchOp } from "./types";

export const WIPE_CONFIRMATION = "DELETE";

export const CATEGORY_LABELS: Record<string, string> = {
  anti_pattern: "Phrase to avoid",
  voice: "Voice",
  dismissal: "Scout filter",
  scout_timing: "Timing",
  author_tier: "Author size",
  rubric_evolution: "Scout rubric",
  few_shot_promotion: "Example to keep",
  model_suggestion: "Model suggestion",
};

export function categoryLabel(category: string): string {
  return CATEGORY_LABELS[category] ?? category.replace(/_/g, " ");
}

export function confidenceLabel(confidence: number): "Low" | "Medium" | "High" {
  if (confidence >= 0.75) return "High";
  if (confidence >= 0.5) return "Medium";
  return "Low";
}

export function targetLabel(target: Patch["target"]): string {
  return target === "rubric" ? "Scout rubric" : target === "style" ? "Style guide" : "Example library";
}

export function describeOp(op: PatchOp): string {
  switch (op.op) {
    case "add_criterion":
      return `Add scout criterion: ${op.text}`;
    case "remove_criterion":
      return `Remove scout criterion: ${op.text}`;
    case "set_weight":
      return `Set weight ${op.key} to ${op.value}`;
    case "add_rule":
      return `Add style rule: ${op.text}`;
    case "remove_rule":
      return `Remove style rule: ${op.text}`;
    case "add_banned":
      return `Never write: ${op.pattern}`;
    case "remove_banned":
      return `Allow again: ${op.pattern}`;
    case "promote_example":
      return `Keep as example: ${op.example_text}`;
  }
}

export function opLabel(op: PatchOp): string {
  switch (op.op) {
    case "add_criterion":
    case "remove_criterion":
      return "Scout criterion";
    case "add_rule":
    case "remove_rule":
      return "Style rule";
    case "add_banned":
    case "remove_banned":
      return "Phrase to avoid";
    case "set_weight":
      return "Weight";
    case "promote_example":
      return "Example";
  }
}

/** The editable text of an op, or null when the op is not plain text (weights, examples). */
export function opText(op: PatchOp): string | null {
  switch (op.op) {
    case "add_criterion":
    case "remove_criterion":
    case "add_rule":
    case "remove_rule":
      return op.text;
    case "add_banned":
    case "remove_banned":
      return op.pattern;
    default:
      return null;
  }
}

export function withOpText(op: PatchOp, text: string): PatchOp {
  switch (op.op) {
    case "add_criterion":
    case "remove_criterion":
    case "add_rule":
    case "remove_rule":
      return { ...op, text };
    case "add_banned":
    case "remove_banned":
      return { ...op, pattern: text };
    default:
      return op;
  }
}

/** Applies the user's text edits and drops ops they emptied. Returns null when nothing is left. */
export function editedPatch(patch: Patch, texts: Record<number, string>): Patch | null {
  const ops = patch.ops
    .map((op, i) => (texts[i] === undefined ? op : withOpText(op, texts[i].replace(/\s+/g, " ").trim())))
    .filter((op) => opText(op) === null || opText(op) !== "");
  return ops.length ? { ...patch, ops } : null;
}

export function patchesEqual(a: Patch, b: Patch): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export function diffSummary(diff: DiffLine[]): { added: number; removed: number } {
  return {
    added: diff.filter((d) => d.kind === "add").length,
    removed: diff.filter((d) => d.kind === "remove").length,
  };
}

export function evidenceLine(ev: Insight["evidence"]): string {
  if (ev.total <= 0) return "";
  return ev.support === ev.total && ev.total === 1 ? "1 item" : `${ev.support} of ${ev.total}`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

export function multiplier(n: number | null | undefined): string {
  return n === null || n === undefined ? "n/a" : `${n.toFixed(1)}x`;
}

export function canConfirmWipe(typed: string): boolean {
  return typed.trim() === WIPE_CONFIRMATION;
}

export function sampleNote(published: number, min: number): string | null {
  return published >= min ? null : `${published} of ${min} published replies. Suggestions stay low confidence until you reach ${min}.`;
}
