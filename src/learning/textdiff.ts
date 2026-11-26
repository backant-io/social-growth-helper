import type { DiffLine } from "./types";

/** Line-level diff (longest common subsequence); mirrors learning/textdiff.rs so Style and Learning diffs read alike. */
export function lineDiff(before: string[], after: string[]): DiffLine[] {
  const n = before.length;
  const m = after.length;
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] = before[i] === after[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (before[i] === after[j]) {
      out.push({ kind: "same", text: before[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      out.push({ kind: "remove", text: before[i++] });
    } else {
      out.push({ kind: "add", text: after[j++] });
    }
  }
  for (; i < n; i++) out.push({ kind: "remove", text: before[i] });
  for (; j < m; j++) out.push({ kind: "add", text: after[j] });
  return out;
}
