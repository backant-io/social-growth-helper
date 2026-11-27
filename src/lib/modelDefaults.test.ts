import { afterEach, describe, expect, it, vi } from "vitest";
import { api } from "./api";
import { commonRoute, recommendRoutes, recommendedModel, refreshLiveModels } from "./modelDefaults";
import type { ModelInfo, ProviderAccountView, ProviderType, TaskModelRoute, TaskType } from "./types";
import { TASK_METAS } from "./types";

const caps = (vision: boolean) => ({ vision, json_schema: true, streaming: true, thinking: false });
const model = (id: string, type: ProviderType, vision = true): ModelInfo => ({ id, display_name: id, provider_type: type, capabilities: caps(vision), is_custom: false });
const acct = (type: ProviderType, models: ModelInfo[], status: ProviderAccountView["status"] = "connected"): ProviderAccountView => ({
  id: type, provider_type: type, label: type, billing_lane: "api", status, last_verified_at: null, last_error: null, has_secret: true,
  config: { custom_models: [], endpoint_override: null, cli_binary_path: null }, models, personal_use_notice: null,
});
const unset = (t: TaskType): TaskModelRoute => ({ task_type: t, provider_account_id: null, model_id: null, reasoning_effort: null, temperature: null, fallback_account_id: null, fallback_model_id: null, max_calls_per_hour: null, updated_at: "" });

const anthropic = acct("anthropic_api", [model("claude-opus-4-1", "anthropic_api"), model("claude-sonnet-4-5", "anthropic_api"), model("claude-haiku-4-5", "anthropic_api")]);

describe("recommended models", () => {
  it("prefers a balanced Sonnet and a fast Haiku for scoring", () => {
    expect(recommendedModel("reply_drafting", [anthropic])?.model.id).toBe("claude-sonnet-4-5");
    expect(recommendedModel("post_scoring", [anthropic])?.model.id).toBe("claude-haiku-4-5");
  });

  it("picks from live-fetched ids that are not in the built-in list", () => {
    const live = acct("anthropic_api", [model("claude-opus-9", "anthropic_api"), model("claude-sonnet-9-20300101", "anthropic_api")]);
    expect(recommendedModel("reply_drafting", [live])?.model.id).toBe("claude-sonnet-9-20300101");
  });

  it("ignores providers that are not connected and returns null when nothing can run the task", () => {
    expect(recommendedModel("reply_drafting", [acct("anthropic_api", anthropic.models, "needs_login")])).toBeNull();
    const textOnly = acct("grok_web", [model("grok-web", "grok_web", false)]);
    expect(recommendedModel("browser_navigation", [textOnly])).toBeNull();
  });

  it("prefers API-key Claude over the experimental Grok tab", () => {
    const grok = acct("grok_web", [model("grok-web", "grok_web", false)]);
    expect(recommendedModel("reply_drafting", [grok, anthropic])?.account.provider_type).toBe("anthropic_api");
  });
});

describe("filling routes", () => {
  it("fills only unset tasks and never overwrites an existing choice", () => {
    const routes = TASK_METAS.map((t) => unset(t.id));
    routes[1] = { ...routes[1], provider_account_id: "anthropic_api", model_id: "claude-opus-4-1" };
    const patches = recommendRoutes([anthropic], routes);
    expect(patches).toHaveLength(routes.length - 1);
    expect(patches.some((p) => p.task_type === routes[1].task_type)).toBe(false);
    expect(patches.every((p) => p.provider_account_id === "anthropic_api" && p.model_id)).toBe(true);
  });

  it("finds the model most tasks share", () => {
    const routes = [unset("post_scoring"), unset("reply_drafting"), unset("dashboard_builder")].map((r, i) => ({ ...r, provider_account_id: "a", model_id: i < 2 ? "m1" : "m2" }));
    expect(commonRoute(routes)).toEqual({ providerAccountId: "a", modelId: "m1" });
    expect(commonRoute([unset("post_scoring")])).toBeNull();
  });
});

describe("refreshLiveModels scoping", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("only refreshes the specified provider so connecting another provider does not touch unrelated API keys", async () => {
    const xai = acct("xai_api", [model("grok-4.6", "xai_api")]);
    const spy = vi.spyOn(api, "llmRefreshProviderModels").mockResolvedValue([]);

    await refreshLiveModels([anthropic, xai], "claude_cli");
    expect(spy).not.toHaveBeenCalled();

    await refreshLiveModels([anthropic, xai], "xai_api");
    expect(spy).toHaveBeenCalledTimes(1);
    expect(spy).toHaveBeenCalledWith("xai_api");
  });
});
