import { describe, expect, it } from "vitest";
import { browserHint, providerHint, SETUP_STEP_COPY, TOUR_STEPS, wizardOrder } from "./tourSteps";

describe("wizard order", () => {
  it("shows the tour, then the rules acknowledgement, before any setup step", () => {
    const order = wizardOrder();
    expect(order[TOUR_STEPS.length].kind).toBe("rules");
    expect(order.at(-1)?.kind).toBe("style");
    expect(order.filter((s) => s.kind === "tour")).toHaveLength(TOUR_STEPS.length);
    expect(order.slice(0, TOUR_STEPS.length).every((s) => s.kind === "tour")).toBe(true);
  });

  it("leaves out the rules step when it was already acknowledged", () => {
    expect(wizardOrder(true).some((s) => s.kind === "rules")).toBe(false);
    expect(wizardOrder(true)[TOUR_STEPS.length].kind).toBe("preferences");
  });
});

describe("tour content", () => {
  it("covers every surface the product promises", () => {
    expect(TOUR_STEPS.map((s) => s.id)).toEqual(["scout", "reply", "publish", "creator", "dashboard", "learning", "models"]);
  });

  it("describes the six-action bar and that the user clicks each action", () => {
    const reply = TOUR_STEPS.find((s) => s.id === "reply")!;
    for (const a of ["reply", "quote", "like", "repost", "bookmark", "follow"]) expect(reply.body).toContain(a);
  });

  it("never claims anything is posted automatically", () => {
    const text = TOUR_STEPS.map((s) => s.body).join(" ").toLowerCase();
    expect(text).not.toContain("auto-post");
    expect(text).not.toContain("automatically post");
  });
});

describe("guided setup steps", () => {
  it("walks the rules, preferences, Chrome and X login, one AI connection and the style guide", () => {
    const kinds = wizardOrder().map((s) => s.kind).filter((k) => k !== "tour");
    expect(kinds).toEqual(["rules", "preferences", "browser", "providers", "style"]);
    expect(Object.keys(SETUP_STEP_COPY)).toEqual(["browser", "providers", "style"]);
  });

  it("reports Chrome detection and when login unlocks", () => {
    expect(browserHint(null, false).done).toBe(false);
    expect(browserHint({ install: null, running: false }, false)).toMatchObject({ done: false });
    expect(browserHint({ install: {}, running: false }, false).text).toContain("acknowledge the rules");
    expect(browserHint({ install: {}, running: false }, true).text).toContain("Open the X window");
    expect(browserHint({ install: {}, running: true }, true).text).toContain("Log in to X");
  });

  it("reports provider progress", () => {
    expect(providerHint([{ status: "disconnected" }]).done).toBe(false);
    expect(providerHint([{ status: "connected" }, { status: "connected" }]).text).toContain("2 providers connected");
  });
});
