import { describe, expect, it } from "vitest";
import { TOUR_STEPS, wizardOrder } from "./tourSteps";

describe("wizard order", () => {
  it("shows the tour, then preferences, then the rules acknowledgement last", () => {
    const order = wizardOrder();
    expect(order.at(-1)?.kind).toBe("rules");
    expect(order.at(-2)?.kind).toBe("preferences");
    expect(order.filter((s) => s.kind === "tour")).toHaveLength(TOUR_STEPS.length);
    expect(order.slice(0, TOUR_STEPS.length).every((s) => s.kind === "tour")).toBe(true);
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
