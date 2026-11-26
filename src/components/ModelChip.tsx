import { useState } from "react";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { ModelPicker, type ModelSelection } from "./ModelPicker";
import { routeLabel, saveRouteSelection } from "./modelRoute";

interface Props {
  taskType: TaskType;
  providers: ProviderAccountView[];
  route: TaskModelRoute | undefined;
  onChanged: () => Promise<void> | void;
}

const chipBtn =
  "rounded-lg border border-zinc-300 px-2 py-0.5 text-xs text-zinc-700 hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800";

/** Shows the model a task uses with a "Change model" action that updates the route inline (LLM-22). */
export function ModelChip({ taskType, providers, route, onChanged }: Props) {
  const meta = TASK_METAS.find((t) => t.id === taskType)!;
  const [open, setOpen] = useState(false);
  const [selection, setSelection] = useState<ModelSelection>({ providerAccountId: null, modelId: null });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = () => {
    setSelection({ providerAccountId: route?.provider_account_id ?? null, modelId: route?.model_id ?? null });
    setError(null);
    setOpen(true);
  };

  const save = async () => {
    if (!selection.providerAccountId || !selection.modelId) return;
    setSaving(true);
    setError(null);
    try {
      await saveRouteSelection(taskType, route, selection);
      setOpen(false);
      await onChanged();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="space-y-2 text-xs text-zinc-600 dark:text-zinc-400" role="group" aria-label={`Model for ${meta.label}`}>
      <div className="flex flex-wrap items-center gap-2">
        <span>
          {meta.label} model: <strong className="font-medium text-zinc-900 dark:text-zinc-100">{routeLabel(providers, route)}</strong>
        </span>
        {!open && (
          <button type="button" className={chipBtn} onClick={start}>
            Change model
          </button>
        )}
      </div>
      {open && (
        <div className="space-y-2 rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
          <ModelPicker
            providers={providers}
            value={selection}
            onChange={setSelection}
            allowUnset={false}
            requireVision={meta.requiresVision}
            compact
            ariaLabel={`Choose model for ${meta.label}`}
            onProvidersUpdated={onChanged}
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              className="rounded-lg bg-zinc-900 px-3 py-1 text-xs font-medium text-white disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
              disabled={saving || !selection.providerAccountId || !selection.modelId}
              onClick={() => void save()}
            >
              {saving ? "Saving..." : `Use for ${meta.label}`}
            </button>
            <button type="button" className={chipBtn} onClick={() => setOpen(false)}>
              Cancel
            </button>
            {error && <span className="text-red-600 dark:text-red-400">{error}</span>}
          </div>
        </div>
      )}
    </div>
  );
}
