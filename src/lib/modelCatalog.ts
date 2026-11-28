import type { ModelInfo, ProviderAccountView, ProviderCapabilities } from "./types";

type RawModel = Partial<ModelInfo> & { label?: string; custom?: boolean };
type RawAccount = Omit<ProviderAccountView, "models"> & { models?: RawModel[] | null };

const NO_CAPS: ProviderCapabilities = { vision: false, json_schema: false, streaming: false, thinking: false };

/** A model's human name, never empty: falls back to its id. */
export function modelLabel(m: Pick<ModelInfo, "id" | "display_name">): string {
  return m.display_name?.trim() || m.id;
}

/** Accepts both the UI shape (`display_name`/`is_custom`) and the older backend shape (`label`/`custom`). */
export function normalizeModel(raw: RawModel): ModelInfo {
  const id = (raw.id ?? "").trim();
  const name = (raw.display_name ?? raw.label ?? "").trim();
  return {
    id,
    display_name: name || id,
    provider_type: raw.provider_type,
    capabilities: { ...NO_CAPS, ...raw.capabilities },
    is_custom: Boolean(raw.is_custom ?? raw.custom),
  };
}

export function normalizeModels(raw: RawModel[] | null | undefined): ModelInfo[] {
  return (raw ?? []).map(normalizeModel).filter((m) => m.id.length > 0);
}

export function normalizeAccount<T extends RawAccount>(raw: T): ProviderAccountView {
  return { ...raw, models: normalizeModels(raw.models).map((m) => ({ ...m, provider_type: m.provider_type ?? raw.provider_type })) };
}

export interface ModelOption {
  value: string;
  label: string;
  disabled: boolean;
}

export interface ModelOptionsInput {
  providers: ProviderAccountView[];
  selected: { providerAccountId: string | null; modelId: string | null };
  requireVision?: boolean;
  unsetLabel?: string | null;
}

/**
 * The single option list for every model dropdown (Default model, per-task, fallback).
 * Labels read "Provider · Model". The only disabled entries are text-only models on a task that sends
 * screenshots (Browser Navigation): that call would fail, so they cannot be picked there.
 * A saved choice that is no longer in the catalog stays listed so the select never shows a wrong row.
 */
export function buildModelOptions({ providers, selected, requireVision = false, unsetLabel = null }: ModelOptionsInput): ModelOption[] {
  const out: ModelOption[] = [];
  const current = selected.providerAccountId && selected.modelId ? `${selected.providerAccountId}::${selected.modelId}` : "";
  if (unsetLabel !== null || !current) out.push({ value: "", label: unsetLabel ?? "Unset (choose a model)", disabled: false });
  for (const acct of providers) {
    for (const m of acct.models) {
      if (!m.id) continue;
      const name = modelLabel(m);
      const custom = m.is_custom && !/\(custom\)$/i.test(name) ? " (custom)" : "";
      const blocked = requireVision && !m.capabilities.vision;
      out.push({ value: `${acct.id}::${m.id}`, label: `${acct.label} · ${name}${custom}${blocked ? " (no vision)" : ""}`, disabled: blocked });
    }
  }
  if (current && !out.some((o) => o.value === current)) {
    const acct = providers.find((p) => p.id === selected.providerAccountId);
    out.push({ value: current, label: `${acct?.label ?? "Disconnected provider"} · ${selected.modelId} (not in list)`, disabled: false });
  }
  return out;
}
