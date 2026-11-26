import { describe, expect, it } from "vitest";
import { canEdit, fillSkeleton, fileName, parsePaths, percent, postLength, similarityNote, slotKeys, syncSlots } from "./creator";

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
