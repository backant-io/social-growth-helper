import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { ModelChip } from "./ModelChip";
import { routeIsSet } from "./modelRoute";
import { UnsetRouteGuard } from "./UnsetRouteGuard";

interface Props {
  taskType: TaskType;
  /** Called after a model was assigned or changed so the host can refresh its own "is configured" state. */
  onAssigned: () => Promise<void> | void;
  onGoToSettings?: () => void;
  /** Shown under the "Choose a model" prompt while the route is unset. */
  hint?: string;
  /** Show the model chip with "Change model" once a model is set. Defaults to off: model choice lives in Settings. */
  showChip?: boolean;
}

/** The inline model chooser for one task: "Choose a model" while unset (LLM-21), "Change model" once set (LLM-22). */
export function InlineModelGuard({ taskType, onAssigned, onGoToSettings, hint, showChip = false }: Props) {
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [route, setRoute] = useState<TaskModelRoute | undefined>(undefined);
  const [loaded, setLoaded] = useState(false);

  const load = useCallback(async () => {
    const [p, r] = await Promise.all([api.llmListProviders(), api.llmListTaskRoutes()]);
    setProviders(p);
    setRoute(r.find((x) => x.task_type === taskType));
    setLoaded(true);
  }, [taskType]);

  useEffect(() => {
    void load().catch(() => undefined);
  }, [load]);

  if (!loaded) return null;

  const refreshed = async () => {
    await load();
    await onAssigned();
  };

  if (routeIsSet(route)) {
    return showChip ? <ModelChip taskType={taskType} providers={providers} route={route} onChanged={refreshed} /> : null;
  }

  return (
    <div className="space-y-1">
      <UnsetRouteGuard taskType={taskType} providers={providers} route={route} onAssigned={refreshed} onGoToSettings={onGoToSettings} />
      {hint && <p className="px-1 text-xs text-zinc-500">{hint}</p>}
    </div>
  );
}
