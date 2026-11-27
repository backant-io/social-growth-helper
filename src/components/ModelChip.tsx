import { useState } from "react";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { ModelPicker } from "./ModelPicker";
import { routeLabel, saveRouteSelection } from "./modelRoute";

interface Props {
  taskType: TaskType;
  providers: ProviderAccountView[];
  route: TaskModelRoute | undefined;
  onChanged: () => Promise<void> | void;
}

/** One quiet line showing the model a task uses. Picking another model saves immediately. */
export function ModelChip({ taskType, providers, route, onChanged }: Props) {
  const meta = TASK_METAS.find((t) => t.id === taskType)!;
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  return (
    <div className="text-xs text-zinc-500" role="group" aria-label={`Model for ${meta.label}`}>
      <div className="flex flex-wrap items-center gap-2">
        <span>
          Model <span className="mx-1 text-zinc-300 dark:text-zinc-700">/</span>
          <strong className="font-medium text-zinc-800 dark:text-zinc-200">{routeLabel(providers, route)}</strong>
        </span>
        <button type="button" className="underline-offset-2 hover:text-zinc-950 hover:underline dark:hover:text-zinc-50" onClick={() => setOpen(!open)}>
          {open ? "Close" : "Change"}
        </button>
        {error && <span className="text-red-600 dark:text-red-400">{error}</span>}
      </div>
      {open && (
        <div className="mt-2">
          <ModelPicker
            providers={providers.filter((p) => p.status === "connected")}
            value={{ providerAccountId: route?.provider_account_id ?? null, modelId: route?.model_id ?? null }}
            allowUnset={false}
            requireVision={meta.requiresVision}
            compact
            ariaLabel={`Choose model for ${meta.label}`}
            onProvidersUpdated={onChanged}
            onChange={async (sel) => {
              if (!sel.providerAccountId || !sel.modelId) return;
              setError(null);
              try {
                await saveRouteSelection(taskType, route, sel);
                setOpen(false);
                await onChanged();
              } catch (e) {
                setError((e as Error).message);
              }
            }}
          />
        </div>
      )}
    </div>
  );
}
