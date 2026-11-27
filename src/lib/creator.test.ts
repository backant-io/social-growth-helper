import { describe, expect, it } from "vitest";
import { xPostUrl, candidateInsights, canEdit, fillSkeleton, fileName, parsePaths, percent, postLength, similarityNote, slotKeys, syncSlots } from "./creator";

describe("postLength", () => {
  it("counts every link as 23 characters", () => {
    expect(postLength("hi https://example.com/a/very/long/path?x=1 there")).toBe("hi  there".length + 23);
  });
  it("counts emoji as one character each", () => {
    expect(postLength("a😀b")).toBe(3);
  });
});

describe("skeletons", () => {
  it("lists unique slot keys in order", () => {
    expect(slotKeys("{a} then {b} then {a}")).toEqual(["a", "b"]);
  });
  it("fills what is provided and leaves the rest visible", () => {
    expect(fillSkeleton("{belief} is wrong. {reason}", { belief: " hustle ", reason: "" })).toBe("hustle is wrong. {reason}");
  });
  it("keeps existing slot labels when the skeleton changes", () => {
    const out = syncSlots("{a} {c}", [{ key: "a", label: "Alpha", hint: "h" }, { key: "b", label: "Beta", hint: "" }]);
    expect(out).toEqual([{ key: "a", label: "Alpha", hint: "h" }, { key: "c", label: "c", hint: "" }]);
  });
});

describe("parsePaths", () => {
  it("accepts quoted paths, file urls and duplicates", () => {
    expect(parsePaths(`"/a/b.png"\nfile:///c/d%20e.jpg\n/a/b.png\n\n`)).toEqual(["/a/b.png", "/c/d e.jpg"]);
  });
  it("takes the last segment as the file name", () => {
    expect(fileName("/a/b/c.png")).toBe("c.png");
    expect(fileName("C:\\a\\c.png")).toBe("c.png");
  });
});

describe("labels", () => {
  it("formats percentages", () => {
    expect(percent(0.256)).toBe("26%");
    expect(percent(null)).toBe("n/a");
  });
  it("warns at or above the threshold", () => {
    expect(similarityNote(0.5, 0.5).tone).toBe("warn");
    expect(similarityNote(0.49, 0.5).tone).toBe("ok");
    expect(similarityNote(null, 0.5).tone).toBe("none");
  });
  it("only lets unposted drafts be edited", () => {
    expect(["variant", "draft", "failed"].every((s) => canEdit(s as never))).toBe(true);
    expect(["publishing", "published", "unconfirmed", "discarded"].some((s) => canEdit(s as never))).toBe(false);
  });
});

describe("candidateInsights", () => {
  const base = { author_followers: 4200, virality_score: 74.4, metrics: { likes: 1800, reposts: 210, replies: 95, bookmarks: 420, views: 190000 } };
  it("reports virality, reach, engagement and saves per like", () => {
    const got = Object.fromEntries(candidateInsights(base).map((i) => [i.label, i.value]));
    expect(got).toEqual({ Virality: "74", Reach: "45x followers", Engagement: "1.3%", "Saves per like": "23%" });
  });
  it("leaves out what the data cannot support", () => {
    const got = candidateInsights({ author_followers: null, virality_score: 50, metrics: { likes: 0, reposts: 1, replies: 0, bookmarks: 0, views: null } });
    expect(got.map((i) => i.label)).toEqual(["Virality"]);
  });
});

describe("xPostUrl", () => {
  it("builds a permalink and falls back to /i/status for odd handles", () => {
    expect(xPostUrl("@bob", "2")).toBe("https://x.com/bob/status/2");
    expect(xPostUrl("", "2")).toBe("https://x.com/i/status/2");
    expect(xPostUrl("not a handle", "2")).toBe("https://x.com/i/status/2");
  });
});
