import { describe, expect, it } from "vitest";
import { p3 } from "./api";

describe("scoutStop preview lifecycle", () => {
  it("transitions running -> idle immediately and reports whether a session was stopped", async () => {
    await p3.scoutStop();
    expect((await p3.scoutStatus()).running).toBe(false);
    expect(await p3.scoutStop()).toBe(false);

    await p3.scoutStart({
      feed_source: "for_you",
      max_duration_sec: 3600,
      max_posts: 200,
      stop_after_queued: 10,
      threshold: 72,
    });
    expect((await p3.scoutStatus()).running).toBe(true);

    expect(await p3.scoutStop()).toBe(true);
    expect((await p3.scoutStatus()).running).toBe(false);
    expect(await p3.scoutStop()).toBe(false);
  });
});
