import { describe, expect, it } from "vitest";
import { p3 } from "./api";
import { bundleForMode } from "./logic";

describe("quote publishes as a quote, not a reply", () => {
  it("switching a draft to Quote swaps the reply action for a quote action", async () => {
    const [item] = await p3.scoutQueue();
    const reply = await p3.draftOpen(item.evaluation_id);
    expect(reply.kind).toBe("reply");
    expect(reply.actions).toContain("reply");

    const quote = await p3.draftSetActions(reply.id, bundleForMode("quote"));
    expect(quote.kind).toBe("quote");
    expect(quote.actions).toEqual(["quote"]);

    await p3.publishDraft(quote.id, "Worth reading in full.");
    const done = await p3.draftGet(quote.id);
    expect(done.status).toBe("published");
    expect(done.steps.map((s) => s.action)).toEqual(["quote"]);
  });

  it("switching back to Reply restores the reply bundle", async () => {
    const [, item] = await p3.scoutQueue();
    const d = await p3.draftOpen(item.evaluation_id);
    await p3.draftSetActions(d.id, bundleForMode("quote"));
    const back = await p3.draftSetActions(d.id, bundleForMode("reply"));
    expect(back.kind).toBe("reply");
    expect(back.actions).toEqual(["like", "reply"]);
  });

  it("Repost is the plain engage action and carries no text", async () => {
    const [item] = await p3.scoutQueue();
    const r = await p3.engage(item.post.observed_id, "repost", false);
    expect(r.ok).toBe(true);
  });
});
