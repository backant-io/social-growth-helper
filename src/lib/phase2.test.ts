import { beforeEach, describe, expect, it } from "vitest";
import { decodeModelChoice, encodeModelChoice, findSelectedModel } from "../components/ModelPicker";
import { api, resetMockState } from "./api";
import { OFFLINE_ACTION_HINT, offlineActionHint, parseLlmError } from "./errors";

describe("Phase 2 LLM error & model picker helpers", () => {
  it("parses structured [llm:code] errors cleanly", () => {
    const unset = parseLlmError(new Error("[llm:unset_route] No model is chosen for Reply Drafting."));
    expect(unset.code).toBe("unset_route");
    expect(unset.isUnsetRoute).toBe(true);
    expect(unset.isOffline).toBe(false);
    expect(unset.message).toBe("No model is chosen for Reply Drafting.");

    const offline = parseLlmError("[llm:offline] cannot reach api.anthropic.com");
    expect(offline.code).toBe("offline");
    expect(offline.isOffline).toBe(true);

    const rate = parseLlmError("[llm:rate_guard_blocked] 60/60 calls used");
    expect(rate.isRateGuard).toBe(true);
  });

  it("encodes, decodes, and resolves model selections", async () => {
    resetMockState();
    const acct = await api.llmSaveApiKeyProvider("anthropic_api", "Anthropic", "sk-ant-test-123456");
    const encoded = encodeModelChoice(acct.id, acct.models[0].id);
    expect(encoded).toBe("anthropic_api::claude-sonnet-4-5");
    expect(decodeModelChoice(encoded)).toEqual({
      providerAccountId: "anthropic_api",
      modelId: "claude-sonnet-4-5",
    });
    expect(decodeModelChoice("")).toEqual({ providerAccountId: null, modelId: null });

    const found = findSelectedModel([acct], {
      providerAccountId: acct.id,
      modelId: acct.models[0].id,
    });
    expect(found?.model.capabilities.vision).toBe(true);
  });
});

describe("Phase 2 browser-preview API round-trips", () => {
  beforeEach(() => {
    resetMockState();
  });

  it("keeps all 7 task routes unset initially and blocks bench until routed", async () => {
    const routes = await api.llmListTaskRoutes();
    expect(routes).toHaveLength(7);
    expect(routes.every((r) => r.provider_account_id === null && r.model_id === null)).toBe(true);

    await expect(
      api.styleRunBench({
        sample_author: "@dev",
        sample_post_text: "Testing unset route guard",
        override_route: null,
        compare_route: null,
        example_limit: 3,
      })
    ).rejects.toThrow("unset_route");
  });

  it("versions style guides, saves/searches examples, and runs bench once routed", async () => {
    const v1 = await api.styleSaveGuideVersion({
      free_text_guide: "Concise engineering voice.",
      learned_rules: ["Cite numbers"],
      banned_patterns: ["game changer"],
      max_reply_chars: 200,
      emoji_policy: "none",
      hashtag_policy: "none",
      note: "v1",
    });
    expect(v1.version).toBe(1);

    const ex = await api.styleUpsertExample({
      id: null,
      kind: "reply",
      source_context_text: "Postgres p99 spikes",
      example_text: "Batching telemetry writes dropped our p99 from 140ms to 19ms.",
      why_it_works: "Concrete metric",
      tags: ["postgres"],
      pinned: true,
      source: "manual",
      performance_score: null,
    });
    expect(ex.pinned).toBe(true);

    const hits = await api.styleSearchExamples("reply", "postgres", 10);
    expect(hits).toHaveLength(1);
    expect(hits[0].example.id).toBe(ex.id);

    const acct = await api.llmConnectClaudeCli("Claude CLI", null);
    await api.llmSaveTaskRoute({
      task_type: "reply_drafting",
      provider_account_id: acct.id,
      model_id: "sonnet",
      reasoning_effort: "medium",
      temperature: 0.6,
      fallback_account_id: null,
      fallback_model_id: null,
      max_calls_per_hour: 30,
    });

    const res = await api.styleRunBench({
      sample_author: "@founder",
      sample_post_text: "How did you lower postgres p99?",
      override_route: null,
      compare_route: null,
      example_limit: 5,
    });
    expect(res.style_guide_version).toBe(1);
    expect(res.primary.variants.length).toBeGreaterThanOrEqual(2);
    expect(res.retrieved_examples).toHaveLength(1);
  });
});

describe("Offline action gating (SHL-24)", () => {
  it("explains why publish/send is disabled only while offline", () => {
    expect(offlineActionHint(true)).toBeUndefined();
    expect(offlineActionHint(false)).toBe(OFFLINE_ACTION_HINT);
  });
});
