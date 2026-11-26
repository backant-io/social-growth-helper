import { describe, expect, it } from "vitest";
import { browserNotices } from "./browserNotices";
import type { BrowserInfo } from "./types";

const base: BrowserInfo = {
  install: null,
  problem: null,
  override_path: null,
  running: false,
  version_check: null,
  display_server: "x11",
  placement_hint: null,
};

describe("browserNotices", () => {
  it("is empty for a current Chrome on X11 or macOS", () => {
    const ok = { status: "ok" as const, version: "157.0.1.2", major: 157, min_tested_major: 140, warning: null };
    expect(browserNotices({ ...base, version_check: ok })).toEqual([]);
  });

  it("warns when Chrome is older than the minimum tested version", () => {
    const old = { status: "too_old" as const, version: "120.0.1.2", major: 120, min_tested_major: 140, warning: "Chrome 120.0.1.2 is older" };
    expect(browserNotices({ ...base, version_check: old })).toEqual([{ id: "chrome-version", text: "Chrome 120.0.1.2 is older" }]);
  });

  it("shows the Wayland hint without treating it as an error", () => {
    const n = browserNotices({ ...base, display_server: "wayland", placement_hint: "You are on Wayland" });
    expect(n).toEqual([{ id: "window-placement", text: "You are on Wayland" }]);
  });
});
