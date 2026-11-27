import { describe, expect, it } from "vitest";
import { charInfo, compactCount, editDistance, factorLabel, filterCommands, formatDuration, matchShortcut, normalizeBundle, bundleForMode, modeOfBundle, repostMenuOptions, publishLine, relativeTime, scoreLine, stepSummary, toggleInBundle } from "./logic";

describe("bundles", () => {
  it("orders actions and keeps one text action", () => {
    expect(normalizeBundle(["reply", "like", "follow"])).toEqual(["like", "follow", "reply"]);
    expect(normalizeBundle(["quote", "reply"])).toEqual(["reply"]);
  });

  it("selecting a text action replaces the other one", () => {
    expect(toggleInBundle(["like", "reply"], "quote")).toEqual(["like", "quote"]);
    expect(toggleInBundle(["like", "quote"], "quote")).toEqual(["like"]);
    expect(toggleInBundle([], "bookmark")).toEqual(["bookmark"]);
  });
});

describe("text helpers", () => {
  it("counts code points and flags overflow", () => {
    expect(charInfo("hi 👋")).toMatchObject({ count: 4, over: false });
    expect(charInfo("x".repeat(281)).over).toBe(true);
    expect(charInfo("abc", 5).remaining).toBe(2);
  });

  it("computes edit distance", () => {
    expect(editDistance("kitten", "sitting")).toBe(3);
    expect(editDistance("", "abc")).toBe(3);
    expect(editDistance("same", "same")).toBe(0);
  });

  it("formats durations and counts", () => {
    expect(formatDuration(45)).toBe("45s");
    expect(formatDuration(600)).toBe("10m");
    expect(formatDuration(3900)).toBe("1h 5m");
    expect(compactCount(1250)).toBe("1.3K");
    expect(compactCount(12)).toBe("12");
  });
});

describe("relativeTime", () => {
  const now = Date.parse("2026-11-27T12:00:00Z");
  it("formats the age like X does", () => {
    expect(relativeTime("2026-11-27T11:59:50Z", now)).toBe("now");
    expect(relativeTime("2026-11-27T11:48:00Z", now)).toBe("12m");
    expect(relativeTime("2026-11-27T09:00:00Z", now)).toBe("3h");
    expect(relativeTime("2026-11-20T09:00:00Z", now)).toBe("Nov 20");
    expect(relativeTime("2025-03-02T09:00:00Z", now)).toBe("Mar 2, 2025");
    expect(relativeTime(null, now)).toBe("");
    expect(relativeTime("nope", now)).toBe("");
  });
});

describe("shortcuts", () => {
  it("recognises the palette and publish combos anywhere", () => {
    expect(matchShortcut({ key: "k", metaKey: true })).toBe("palette");
    expect(matchShortcut({ key: "Enter", ctrlKey: true, target: { tagName: "TEXTAREA" } })).toBe("publish");
  });

  it("ignores plain keys while typing", () => {
    expect(matchShortcut({ key: "j", target: { tagName: "TEXTAREA" } })).toBeNull();
    expect(matchShortcut({ key: "d", target: { tagName: "INPUT" } })).toBeNull();
    expect(matchShortcut({ key: "j", target: { tagName: "BODY" } })).toBe("next");
    expect(matchShortcut({ key: "d" })).toBe("dismiss");
  });
});

describe("palette and steps", () => {
  const cmds = [
    { id: "a", label: "Start scout session" },
    { id: "b", label: "Open Scout" },
  ];
  it("filters by every word", () => {
    expect(filterCommands(cmds, "open scout").map((c) => c.id)).toEqual(["b"]);
    expect(filterCommands(cmds, "")).toHaveLength(2);
    expect(filterCommands(cmds, "zzz")).toHaveLength(0);
  });

  it("summarises publish progress", () => {
    const s = stepSummary([
      { action: "like", status: "done", detail: null },
      { action: "reply", status: "uncertain", detail: "Not confirmed yet" },
    ]);
    expect(s).toMatchObject({ done: 1, total: 2 });
    expect(s.blocked?.action).toBe("reply");
  });
});

describe("scoreLine", () => {
  it("reads as a quiet line: score then up to two humanised signals", () => {
    expect(factorLabel("high_saves")).toBe("High saves");
    expect(factorLabel("3.4x-reach")).toBe("3.4x reach");
    expect(scoreLine(90.6, ["small_account", "breakout", "early_wave"])).toBe("Score 91 · Small account · Breakout");
    expect(scoreLine(72, [])).toBe("Score 72");
  });
});

describe("quote vs reply", () => {
  it("a quote bundle holds the quote action and never the reply action", () => {
    expect(bundleForMode("quote")).toEqual(["quote"]);
    expect(bundleForMode("quote")).not.toContain("reply");
    expect(bundleForMode("reply")).toContain("reply");
    expect(bundleForMode("reply")).not.toContain("quote");
  });

  it("maps bundles back to a compose mode", () => {
    expect(modeOfBundle(["quote"])).toBe("quote");
    expect(modeOfBundle(["like", "reply"])).toBe("reply");
    expect(modeOfBundle(normalizeBundle(["reply", "quote"]))).toBe("reply");
  });
});

describe("repost menu", () => {
  it("offers exactly Repost and Quote, like X", () => {
    expect(repostMenuOptions(false)).toEqual([
      { id: "repost", label: "Repost" },
      { id: "quote", label: "Quote" },
    ]);
  });

  it("turns Repost into Undo repost once reposted", () => {
    expect(repostMenuOptions(true).map((o) => o.label)).toEqual(["Undo repost", "Quote"]);
  });
});

describe("publishLine", () => {
  it("uses only short calm status lines", () => {
    expect(publishLine("published")).toEqual({ text: "Posted", action: null });
    expect(publishLine("publishing")).toEqual({ text: "Checking…", action: null });
    expect(publishLine("queued_for_publish")).toEqual({ text: "Checking…", action: null });
    expect(publishLine("unconfirmed")).toEqual({ text: "Not confirmed yet", action: "Post again" });
    expect(publishLine("failed", "Not confirmed yet")).toEqual({ text: "Not confirmed yet", action: "Post again" });
    expect(publishLine("failed", "the composer did not open")).toEqual({ text: "Didn't post", action: "Try again" });
    expect(publishLine("draft")).toBeNull();
  });

  it("never tells the user to go check X", () => {
    const all = [publishLine("published"), publishLine("publishing"), publishLine("unconfirmed"), publishLine("failed", "x")];
    for (const l of all) expect(`${l?.text} ${l?.action}`).not.toMatch(/check (x|your)|profile|look at/i);
  });
});
