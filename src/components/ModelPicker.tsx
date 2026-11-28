import { useState } from "react";
import { api } from "../lib/api";
import { buildModelOptions } from "../lib/modelCatalog";
import { btnPrimary, input, muted } from "./ui";
import type { ModelInfo, ProviderAccountView } from "../lib/types";

export interface ModelSelection {
  providerAccountId: string | null;
  modelId: string | null;
}

interface Props {
  providers: ProviderAccountView[];
  value: ModelSelection;
  onChange: (next: ModelSelection) => void;
  requireVision?: boolean;
  allowUnset?: boolean;
  unsetLabel?: string;
  compact?: boolean;
  ariaLabel?: string;
  onProvidersUpdated?: () => Promise<void> | void;
}

export function findSelectedModel(
  providers: ProviderAccountView[],
  value: ModelSelection
): { account: ProviderAccountView; model: ModelInfo } | null {
  if (!value.providerAccountId || !value.modelId) return null;
  const account = providers.find((p) => p.id === value.providerAccountId);
  if (!account) return null;
  const model = account.models.find((m) => m.id === value.modelId);
  if (!model) return null;
  return { account, model };
}

export function encodeModelChoice(providerAccountId: string | null, modelId: string | null): string {
  if (!providerAccountId || !modelId) return "";
  return `${providerAccountId}::${modelId}`;
}

export function decodeModelChoice(raw: string): ModelSelection {
  if (!raw) return { providerAccountId: null, modelId: null };
  const idx = raw.indexOf("::");
  if (idx <= 0) return { providerAccountId: null, modelId: null };
  return {
    providerAccountId: raw.slice(0, idx),
    modelId: raw.slice(idx + 2),
  };
}

export function ModelPicker({
  providers,
  value,
  onChange,
  requireVision = false,
  allowUnset = true,
  unsetLabel = "Unset (choose a model)",
  compact = false,
  ariaLabel = "Choose model",
  onProvidersUpdated,
}: Props) {
  const [customModel, setCustomModel] = useState("");
  const [addingFor, setAddingFor] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selected = findSelectedModel(providers, value);
  const encoded = encodeModelChoice(value.providerAccountId, value.modelId);
  const options = buildModelOptions({ providers, selected: value, requireVision, unsetLabel: allowUnset ? unsetLabel : null });

  const handleAddCustom = async () => {
    const accountId = addingFor || providers[0]?.id;
    if (!accountId || !customModel.trim()) return;
    setBusy(true);
    setError(null);
    try {
      await api.llmAddCustomModel(accountId, customModel.trim());
      const addedId = customModel.trim();
      setCustomModel("");
      setAddingFor(null);
      await onProvidersUpdated?.();
      onChange({ providerAccountId: accountId, modelId: addedId });
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={compact ? "space-y-1" : "space-y-1.5"}>
      <div className="flex flex-wrap items-center gap-1.5">
        <select
          aria-label={ariaLabel}
          className={`${input} min-w-[15rem] ${compact ? "py-1.5 text-xs" : ""}`}
          value={encoded}
          onChange={(e) => onChange(decodeModelChoice(e.target.value))}
        >
          {options.map((o) => (
            <option key={o.value || "unset"} value={o.value} disabled={o.disabled}>
              {o.label}
            </option>
          ))}
        </select>

        {providers.length > 0 && onProvidersUpdated && (
          <button
            type="button"
            className={`px-1.5 py-1 text-xs ${muted} hover:text-zinc-950 dark:hover:text-zinc-50`}
            onClick={() => setAddingFor(addingFor ? null : (providers.find((p) => p.id === value.providerAccountId) ?? providers[0]).id)}
            title="Add a custom model ID"
          >
            {addingFor ? "Cancel" : "Custom ID"}
          </button>
        )}
      </div>

      {selected && !compact && (
        <p className={`text-xs ${muted}`}>
          {selected.account.billing_lane === "subscription" ? "Subscription" : "API billing"}
          {selected.model.capabilities.vision ? " / Vision" : ""}
          {selected.model.capabilities.thinking ? " / Thinking" : ""}
        </p>
      )}

      {requireVision && selected && !selected.model.capabilities.vision && (
        <p className="text-xs text-red-600 dark:text-red-400">
          This task requires a vision-capable model. Pick one marked with Vision.
        </p>
      )}

      {addingFor && (
        <div className="flex flex-wrap items-center gap-1.5 pt-1">
          <select
            aria-label="Provider for custom model"
            className={`${input} py-1 text-xs`}
            value={addingFor}
            onChange={(e) => setAddingFor(e.target.value)}
          >
            {providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.label}
              </option>
            ))}
          </select>
          <input
            aria-label="Custom model ID"
            className={`${input} min-w-[12rem] py-1 text-xs`}
            placeholder="e.g. grok-4.20 or claude-opus-4-6"
            value={customModel}
            onChange={(e) => setCustomModel(e.target.value)}
          />
          <button
            type="button"
            disabled={busy || !customModel.trim()}
            onClick={() => void handleAddCustom()}
            className={`${btnPrimary} py-1 text-xs`}
          >
            Add
          </button>
          {error && <span className="text-xs text-red-600 dark:text-red-400">{error}</span>}
        </div>
      )}
    </div>
  );
}
