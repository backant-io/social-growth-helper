import { describe, expect, it } from "vitest";
import { bannerFor, sameStatus, type BrowserStatus } from "./browserState";

const st = (over: Partial<BrowserStatus>): BrowserStatus => ({ state: "ready", message: "m", job: null, can_restart_job: false, ...over });

describe("bannerFor", () => {
  it("shows nothing when X is closed on purpose, ready, or unknown", () => {
    expect(bannerFor(null)).toBeNull();
    expect(bannerFor(st({ state: "closed" }))).toBeNull();
    expect(bannerFor(st({ state: "ready" }))).toBeNull();
    expect(bannerFor(st({ state: "ready", job: "publish" }))).toBeNull();
  });

  it("offers focus-X and a re-check for a persistent logged-out or locked state, without Abort", () => {
    for (const state of ["logged_out", "locked", "challenge"] as const) {
      const b = bannerFor(st({ state }))!;
      expect(b.tone).toBe("warn");
      expect(b.actions).toEqual(["focus", "resume"]);
      expect(b.resumeLabel).toBe("Re-check");
    }
  });

  it("offers focus-X, Resume and Abort when a challenge stopped a job", () => {
    const b = bannerFor(st({ state: "challenge", job: "scout", can_restart_job: true }))!;
    expect(b.actions).toEqual(["focus", "resume", "abort"]);
    expect(b.resumeLabel).toBe("Resume Scout");
    expect(bannerFor(st({ state: "challenge", job: "publish" }))!.resumeLabel).toBe("Resume");
  });

  it("treats a suspended account as an error", () => {
    expect(bannerFor(st({ state: "suspended" }))!.tone).toBe("error");
  });

  it("offers a relaunch after a crash, plus Abort when a job was stopped", () => {
    expect(bannerFor(st({ state: "crashed" }))!.actions).toEqual(["relaunch"]);
    expect(bannerFor(st({ state: "crashed", job: "scout" }))!.actions).toEqual(["relaunch", "abort"]);
  });

  it("offers to resume Scout once X is clear again", () => {
    const b = bannerFor(st({ state: "ready", job: "scout", can_restart_job: true }))!;
    expect(b.tone).toBe("info");
    expect(b.actions).toEqual(["resume", "abort"]);
  });
});

describe("sameStatus", () => {
  it("compares by content", () => {
    expect(sameStatus(st({}), st({}))).toBe(true);
    expect(sameStatus(st({}), st({ state: "locked" }))).toBe(false);
    expect(sameStatus(null, st({}))).toBe(false);
  });
});
