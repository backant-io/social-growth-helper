import { describe, expect, it } from "vitest";
import { isLocked, isSetupSkipped, markSetupSkipped, clearSetupSkipped, needsSetup } from "./setupSkip";
import { popoverPosition } from "./Spotlight";
import { TOUR_STEPS } from "./tourSteps";

describe("skip setup", () => {
  it("unlocks the screens without acknowledging the rules", () => {
    expect(isLocked(false, false)).toBe(true);
    expect(isLocked(false, true)).toBe(false);
    expect(isLocked(true, false)).toBe(false);
    expect(needsSetup(false)).toBe(true);
  });

  it("remembers the choice until setup is resumed", () => {
    clearSetupSkipped();
    expect(isSetupSkipped()).toBe(false);
    markSetupSkipped();
    expect(isSetupSkipped()).toBe(true);
    clearSetupSkipped();
    expect(isSetupSkipped()).toBe(false);
  });
});

describe("interactive tour", () => {
  it("points every step at a real UI element and screen", () => {
    for (const s of TOUR_STEPS) {
      expect(s.target).toMatch(/^(nav-[a-z]+|main)$/);
      expect(s.section).toBeTruthy();
    }
    expect(new Set(TOUR_STEPS.map((s) => s.target)).size).toBeGreaterThanOrEqual(6);
  });

  it("places the coach mark beside the target, below it, or centred", () => {
    const view = { w: 1100, h: 780 };
    const side = popoverPosition({ top: 100, left: 10, width: 200, height: 40 }, view, 240);
    expect(side.left).toBe(10 + 200 + 18);
    const centred = popoverPosition(null, view, 240);
    expect(centred.left).toBe((1100 - 340) / 2);
    const below = popoverPosition({ top: 100, left: 700, width: 380, height: 40 }, view, 240);
    expect(below.top).toBe(100 + 40 + 18);
    expect(below.left + 340).toBeLessThanOrEqual(1100 - 16);
  });
});
