import { describe, expect, it } from "vitest";
import { lineDiff } from "../learning/textdiff";
import { previousVersion, styleGuideLines, styleVersionDiff } from "./styleDiff";

const base = { free_text_guide: "Dry and short.\n\nNo hype.", learned_rules: ["Lead with the point"], banned_patterns: ["great point"], max_reply_chars: 220, emoji_policy: "minimal" as const, hashtag_policy: "none" as const };

describe("lineDiff", () => {
  it("marks added, removed and unchanged lines", () => {
    expect(lineDiff(["a", "b", "c"], ["a", "c", "d"])).toEqual([
      { kind: "same", text: "a" },
      { kind: "remove", text: "b" },
      { kind: "same", text: "c" },
      { kind: "add", text: "d" },
    ]);
  });
});

describe("style version diff", () => {
  it("flattens a guide into comparable lines without blank prose lines", () => {
    expect(styleGuideLines(base)).toEqual(["Dry and short.", "No hype.", "rule: Lead with the point", "banned: great point", "max reply chars: 220", "emoji: minimal", "hashtags: none"]);
  });

  it("shows what changed between two versions", () => {
    const next = { ...base, banned_patterns: ["great point", "love this"], max_reply_chars: 180 };
    const d = styleVersionDiff(base, next);
    expect(d.filter((l) => l.kind === "add").map((l) => l.text)).toEqual(["banned: love this", "max reply chars: 180"]);
    expect(d.filter((l) => l.kind === "remove").map((l) => l.text)).toEqual(["max reply chars: 220"]);
  });

  it("treats the first version as all additions", () => {
    expect(styleVersionDiff(null, base).every((l) => l.kind === "add")).toBe(true);
  });

  it("finds the previous version regardless of order", () => {
    const vs = [{ version: 3 }, { version: 1 }, { version: 2 }];
    expect(previousVersion(vs, 3)).toEqual({ version: 2 });
    expect(previousVersion(vs, 1)).toBeNull();
  });
});
