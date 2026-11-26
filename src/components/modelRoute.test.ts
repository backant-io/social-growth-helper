import { describe, expect, it } from "vitest";
import { routeIsSet, routeLabel } from "./modelRoute";
import type { ProviderAccountView, TaskModelRoute } from "../lib/types";

const route = (account: string | null, model: string | null) => ({ provider_account_id: account, model_id: model }) as TaskModelRoute;
const providers = [
  { id: "a1", label: "Claude CLI", models: [{ id: "sonnet", display_name: "Sonnet" }] },
] as unknown as ProviderAccountView[];

describe("inline model chip helpers", () => {
  it("treats a route as set only with both an account and a model", () => {
    expect(routeIsSet(route("a1", "sonnet"))).toBe(true);
    expect(routeIsSet(route("a1", null))).toBe(false);
    expect(routeIsSet(route(null, null))).toBe(false);
    expect(routeIsSet(undefined)).toBe(false);
  });

  it("labels the chip with the account and model, or the raw id if the model is gone", () => {
    expect(routeLabel(providers, route("a1", "sonnet"))).toBe("Claude CLI - Sonnet");
    expect(routeLabel(providers, route("a1", "retired-model"))).toBe("retired-model");
    expect(routeLabel(providers, undefined)).toBe("No model");
  });
});
