import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { ModelPicker } from "../components/ModelPicker";
import { buildModelOptions, normalizeAccount, normalizeModel } from "./modelCatalog";
import type { ProviderAccountView, ProviderType } from "./types";

const caps = (vision: boolean) => ({ vision, json_schema: true, streaming: true, thinking: true });

/** Mirrors the built-in catalog in `src-tauri/src/llm/catalog.rs`. */
const RUST_CATALOG: Record<string, { id: string; label: string; vision: boolean }[]> = {
  claude_cli: [
    { id: "sonnet", label: "Claude Sonnet (CLI alias)", vision: true },
    { id: "opus", label: "Claude Opus (CLI alias)", vision: true },
    { id: "haiku", label: "Claude Haiku (CLI alias)", vision: true },
  ],
  anthropic_api: [
    { id: "claude-sonnet-4-5", label: "Claude Sonnet 4.5", vision: true },
    { id: "claude-opus-4-5", label: "Claude Opus 4.5", vision: true },
    { id: "claude-haiku-4-5", label: "Claude Haiku 4.5", vision: true },
  ],
  xai_api: [
    { id: "grok-4.6", label: "Grok 4.6", vision: true },
    { id: "grok-4.3", label: "Grok 4.3", vision: true },
    { id: "grok-3-mini", label: "Grok 3 Mini", vision: false },
  ],
};

function account(pt: string, label: string, models: unknown[]): ProviderAccountView {
  return normalizeAccount({
    id: `acct-${pt}`,
    provider_type: pt as ProviderType,
    label,
    billing_lane: pt.endsWith("_api") ? "api" : "subscription",
    status: "connected",
    last_verified_at: null,
    last_error: null,
    has_secret: true,
    config: { custom_models: [], endpoint_override: null, cli_binary_path: null },
    models: models as never,
    personal_use_notice: null,
  });
}

const legacyShape = (pt: string) => RUST_CATALOG[pt].map((m) => ({ id: m.id, label: m.label, capabilities: caps(m.vision), custom: false }));
const uiShape = (pt: string) => RUST_CATALOG[pt].map((m) => ({ id: m.id, display_name: m.label, capabilities: caps(m.vision), is_custom: false }));

const providers = [
  account("claude_cli", "Claude Subscription CLI", legacyShape("claude_cli")),
  account("anthropic_api", "Anthropic API", uiShape("anthropic_api")),
  account("xai_api", "xAI API", legacyShape("xai_api")),
];
const none = { providerAccountId: null, modelId: null };
const catalogCount = Object.values(RUST_CATALOG).flat().length;

describe("model option labels", () => {
  it("gives every catalog entry a non-empty 'Provider · Model' label, from either wire shape", () => {
    const opts = buildModelOptions({ providers, selected: none, unsetLabel: "Unset" }).filter((o) => o.value);
    expect(opts).toHaveLength(catalogCount);
    for (const o of opts) {
      expect(o.label.trim()).not.toBe("");
      expect(o.label).toMatch(/^.+ · \S.*$/);
    }
    expect(opts.map((o) => o.label)).toContain("Claude Subscription CLI · Claude Sonnet (CLI alias)");
    expect(opts.map((o) => o.label)).toContain("xAI API · Grok 3 Mini");
  });

  it("never disables a catalog model unless the task needs vision and the model has none", () => {
    expect(buildModelOptions({ providers, selected: none }).filter((o) => o.disabled)).toEqual([]);
    const vision = buildModelOptions({ providers, selected: none, requireVision: true }).filter((o) => o.disabled);
    expect(vision.map((o) => o.value)).toEqual(["acct-xai_api::grok-3-mini"]);
  });

  it("falls back to the model id when a name is blank", () => {
    expect(normalizeModel({ id: "grok-x", display_name: "  " }).display_name).toBe("grok-x");
    expect(normalizeModel({ id: "grok-y" }).display_name).toBe("grok-y");
  });

  it("keeps Unset available and lists a saved model that left the catalog instead of a blank row", () => {
    const opts = buildModelOptions({ providers, selected: { providerAccountId: "acct-xai_api", modelId: "grok-2-old" }, unsetLabel: "Unset" });
    expect(opts[0]).toEqual({ value: "", label: "Unset", disabled: false });
    expect(opts.at(-1)).toEqual({ value: "acct-xai_api::grok-2-old", label: "xAI API · grok-2-old (not in list)", disabled: false });
  });

  it("shows a placeholder only while nothing is chosen when Unset is not offered", () => {
    expect(buildModelOptions({ providers, selected: none })[0].value).toBe("");
    expect(buildModelOptions({ providers, selected: { providerAccountId: "acct-claude_cli", modelId: "opus" } })[0].value).not.toBe("");
  });

  it("renders the selected model with its label in the picker, with no blank or grouped rows", () => {
    const html = renderToStaticMarkup(
      <ModelPicker providers={providers} value={{ providerAccountId: "acct-claude_cli", modelId: "sonnet" }} ariaLabel="Model for Post Scoring" onChange={() => undefined} />,
    );
    expect(html).not.toContain("<optgroup");
    expect(html).not.toMatch(/<option[^>]*>\s*<\/option>/);
    expect(html).toMatch(/<option value="acct-claude_cli::sonnet" selected="">Claude Subscription CLI · Claude Sonnet \(CLI alias\)<\/option>/);
    expect(html).toContain(">Unset (choose a model)</option>");
  });
});
