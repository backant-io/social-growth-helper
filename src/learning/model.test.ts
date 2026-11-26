import { describe, expect, it } from "vitest";
import { canConfirmWipe, confidenceLabel, describeOp, editedPatch, formatBytes, multiplier, sampleNote } from "./model";
import type { Patch } from "./types";

const style: Patch = {
  target: "style",
  ops: [
    { op: "add_banned", pattern: "great point" },
    { op: "add_rule", text: "Keep it short" },
  ],
};

describe("editedPatch", () => {
  it("applies text edits to the matching ops", () => {
    const p = editedPatch(style, { 0: "  great   point!  " })!;
    expect(p.ops[0]).toEqual({ op: "add_banned", pattern: "great point!" });
    expect(p.ops[1]).toEqual(style.ops[1]);
  });

  it("drops ops the user emptied and returns null when none remain", () => {
    expect(editedPatch(style, { 1: "" })!.ops).toHaveLength(1);
    expect(editedPatch(style, { 0: " ", 1: "" })).toBeNull();
  });

  it("never turns a weight or example op into text", () => {
    const p: Patch = { target: "rubric", ops: [{ op: "set_weight", key: "freshness", value: 1.5 }] };
    expect(editedPatch(p, { 0: "hack" })!.ops[0]).toEqual(p.ops[0]);
  });
});

describe("labels", () => {
  it("buckets confidence", () => {
    expect([0.2, 0.5, 0.74, 0.75, 0.9].map(confidenceLabel)).toEqual(["Low", "Medium", "Medium", "High", "High"]);
  });

  it("describes ops in plain words", () => {
    expect(describeOp({ op: "add_banned", pattern: "love this" })).toBe("Never write: love this");
    expect(describeOp({ op: "set_weight", key: "freshness", value: 2 })).toBe("Set weight freshness to 2");
  });

  it("formats sizes and multipliers", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
    expect(multiplier(1.234)).toBe("1.2x");
    expect(multiplier(null)).toBe("n/a");
  });

  it("explains the minimum sample", () => {
    expect(sampleNote(12, 30)).toContain("12 of 30");
    expect(sampleNote(30, 30)).toBeNull();
  });
});

describe("wipe confirmation", () => {
  it("requires the exact word", () => {
    expect(canConfirmWipe("DELETE")).toBe(true);
    expect(canConfirmWipe(" DELETE ")).toBe(true);
    expect(["delete", "", "DELETE ALL"].some(canConfirmWipe)).toBe(false);
  });
});
