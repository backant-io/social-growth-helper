import { useState } from "react";
import { recommendedModel } from "../lib/modelDefaults";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { ModelPicker, type ModelSelection } from "./ModelPicker";
import { saveRouteSelection } from "./modelRoute";
import { btn, btnPrimary, muted } from "./ui";

interface Props {
  taskType: TaskType;
  providers: ProviderAccountView[];
  route: TaskModelRoute | undefined;
  onAssigned: () => Promise<void> | void;
  onGoToSettings?: () => void;
}

/** Shown while a task has no model. One click applies the recommended model; never substitutes silently. */
export function UnsetRouteGuard({ taskType, providers, route, onAssigned, onGoToSettings }: Props) {
  const meta = TASK_METAS.find((t) => t.id === taskType)!;
  const [picking, setPicking] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const rec = recommendedModel(taskType, providers);
  const connected = providers.filter((p) => p.status === "connected");

  const assign = async (sel: ModelSelection) => {
    if (!sel.providerAccountId || !sel.modelId) return;
    setSaving(true);
    setError(null);
    try {
      await saveRouteSelection(taskType, route, sel);
      await onAssigned();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div role="region" aria-label={`Model required for ${meta.label}`} className="border-l-2 border-zinc-950 py-1 pl-4 text-sm dark:border-zinc-100">
      <div className="font-medium">{meta.label} needs an AI model</div>
      <p className={`mt-0.5 text-xs ${muted}`}>{meta.description}</p>
      <div className="mt-3 flex flex-wrap items-center gap-2">
        {connected.length === 0 ? (
          onGoToSettings && (
            <button type="button" className={btnPrimary} onClick={onGoToSettings}>
              Connect an AI
            </button>
          )
        ) : (
          <>
            {rec && (
              <button type="button" className={btnPrimary} disabled={saving} onClick={() => void assign({ providerAccountId: rec.account.id, modelId: rec.model.id })}>
                {saving ? "Saving..." : `Use ${rec.model.display_name}`}
              </button>
            )}
            <button type="button" className={btn} onClick={() => setPicking(!picking)}>
              {picking ? "Cancel" : "Choose another"}
            </button>
          </>
        )}
        {error && <span className="text-xs text-red-600 dark:text-red-400">{error}</span>}
      </div>
      {picking && connected.length > 0 && (
        <div className="mt-3">
          <ModelPicker providers={connected} value={{ providerAccountId: null, modelId: null }} requireVision={meta.requiresVision} compact ariaLabel={`Choose model for ${meta.label}`} onProvidersUpdated={onAssigned} onChange={(sel) => void assign(sel)} />
        </div>
      )}
    </div>
  );
}
