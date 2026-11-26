import { useState } from "react";
import { api } from "../lib/api";
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

  const handleAddCustom = async () => {
    const accountId = addingFor || value.providerAccountId || providers[0]?.id;
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
          className={`min-w-[14rem] rounded-lg border border-zinc-300 bg-transparent px-2.5 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900 ${
            compact ? "text-xs" : "text-sm"
          }`}
          value={encoded}
          onChange={(e) => onChange(decodeModelChoice(e.target.value))}
        >
          {allowUnset && <option value="">{unsetLabel}</option>}
          {providers.map((acct) => (
            <optgroup
              key={acct.id}
              label={`${acct.label} (${acct.billing_lane === "subscription" ? "Subscription" : "API"})`}
            >
              {acct.models.map((m) => {
                const visionBlocked = requireVision && !m.capabilities.vision;
                return (
                  <option key={`${acct.id}::${m.id}`} value={`${acct.id}::${m.id}`} disabled={visionBlocked}>
                    {m.display_name}
                    {m.is_custom ? " [custom]" : ""}
                    {visionBlocked ? " (no vision)" : ""}
                  </option>
                );
              })}
            </optgroup>
          ))}
        </select>

        {providers.length > 0 && onProvidersUpdated && (
          <button
            type="button"
            className="rounded-lg border border-zinc-300 px-2 py-1 text-xs text-zinc-600 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-400 dark:hover:bg-zinc-800"
            onClick={() => setAddingFor(addingFor ? null : value.providerAccountId ?? providers[0].id)}
            title="Add a custom model ID"
          >
            {addingFor ? "Cancel" : "+ Custom ID"}
          </button>
        )}
      </div>

      {selected && (
        <div className="flex flex-wrap items-center gap-1.5 text-[11px] text-zinc-500">
          <span
            className={`rounded px-1.5 py-0.5 font-medium ${
              selected.account.billing_lane === "subscription"
                ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300"
                : "bg-zinc-200 text-zinc-800 dark:bg-zinc-800 dark:text-zinc-200"
            }`}
          >
            {selected.account.billing_lane === "subscription" ? "Subscription" : "API"}
          </span>
          {selected.model.capabilities.vision && (
            <span className="rounded bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">Vision</span>
          )}
          {selected.model.capabilities.json_schema && (
            <span className="rounded bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">JSON Schema</span>
          )}
          {selected.model.capabilities.thinking && (
            <span className="rounded bg-zinc-100 px-1.5 py-0.5 dark:bg-zinc-800">Thinking</span>
          )}
        </div>
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
            className="rounded-lg border border-zinc-300 bg-transparent px-2 py-1 text-xs dark:border-zinc-700 dark:bg-zinc-900"
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
            className="min-w-[12rem] rounded-lg border border-zinc-300 bg-transparent px-2.5 py-1 text-xs dark:border-zinc-700 dark:bg-zinc-900"
            placeholder="e.g. grok-4.20 or claude-opus-4-6"
            value={customModel}
            onChange={(e) => setCustomModel(e.target.value)}
          />
          <button
            type="button"
            disabled={busy || !customModel.trim()}
            onClick={() => void handleAddCustom()}
            className="rounded-lg bg-zinc-900 px-2.5 py-1 text-xs font-medium text-white disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
          >
            Add
          </button>
          {error && <span className="text-xs text-red-600 dark:text-red-400">{error}</span>}
        </div>
      )}
    </div>
  );
}
