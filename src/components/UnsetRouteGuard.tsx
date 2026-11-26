import { useState } from "react";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { ModelPicker, type ModelSelection } from "./ModelPicker";
import { saveRouteSelection } from "./modelRoute";

interface Props {
  taskType: TaskType;
  providers: ProviderAccountView[];
  route: TaskModelRoute | undefined;
  onAssigned: () => Promise<void> | void;
  onGoToSettings?: () => void;
}

/**
 * Explicit unset-route guard (LLM-21). Never silently substitutes a model; instead asks
 * the user to pick a provider + model inline or in Settings.
 */
export function UnsetRouteGuard({ taskType, providers, route, onAssigned, onGoToSettings }: Props) {
  const meta = TASK_METAS.find((t) => t.id === taskType)!;
  const [selection, setSelection] = useState<ModelSelection>({
    providerAccountId: route?.provider_account_id ?? null,
    modelId: route?.model_id ?? null,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const saveDefault = async () => {
    if (!selection.providerAccountId || !selection.modelId) return;
    setSaving(true);
    setError(null);
    try {
      await saveRouteSelection(taskType, route, selection);
      await onAssigned();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div
      role="region"
      aria-label={`Model required for ${meta.label}`}
      className="rounded-xl border border-amber-300 bg-amber-50/70 p-4 text-sm dark:border-amber-800 dark:bg-amber-950/30"
    >
      <div className="font-medium text-amber-900 dark:text-amber-200">
        No model is assigned for {meta.label}
      </div>
      <p className="mt-1 text-xs text-amber-800 dark:text-amber-300">
        {meta.description} No silent fallback is used—choose a model below to enable this feature.
      </p>

      {providers.length === 0 ? (
        <div className="mt-3 flex items-center gap-3">
          <span className="text-xs text-zinc-600 dark:text-zinc-400">
            Connect at least one AI provider in Settings first.
          </span>
          {onGoToSettings && (
            <button
              type="button"
              onClick={onGoToSettings}
              className="rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white dark:bg-zinc-100 dark:text-zinc-900"
            >
              Open Settings
            </button>
          )}
        </div>
      ) : (
        <div className="mt-3 space-y-2">
          <ModelPicker
            providers={providers}
            value={selection}
            onChange={setSelection}
            requireVision={meta.requiresVision}
            ariaLabel={`Choose model for ${meta.label}`}
            onProvidersUpdated={onAssigned}
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              disabled={saving || !selection.providerAccountId || !selection.modelId}
              onClick={() => void saveDefault()}
              className="rounded-lg bg-zinc-900 px-3 py-1.5 text-xs font-medium text-white disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
            >
              {saving ? "Saving..." : `Use for ${meta.label}`}
            </button>
            {error && <span className="text-xs text-red-600 dark:text-red-400">{error}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
