import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { UnsetRouteGuard } from "./UnsetRouteGuard";

interface Props {
  taskType: TaskType;
  /** Called after a model was assigned so the host can refresh its own "is configured" state. */
  onAssigned: () => Promise<void> | void;
  onGoToSettings?: () => void;
  hint?: string;
}

/** Loads providers and the route for one task and shows the "Choose a model" prompt (LLM-21, LLM-22). */
export function InlineModelGuard({ taskType, onAssigned, onGoToSettings, hint }: Props) {
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [route, setRoute] = useState<TaskModelRoute | undefined>(undefined);

  const load = useCallback(async () => {
    const [p, r] = await Promise.all([api.llmListProviders(), api.llmListTaskRoutes()]);
    setProviders(p);
    setRoute(r.find((x) => x.task_type === taskType));
  }, [taskType]);

  useEffect(() => {
    void load().catch(() => undefined);
  }, [load]);

  return (
    <div className="space-y-1">
      <UnsetRouteGuard
        taskType={taskType}
        providers={providers}
        route={route}
        onAssigned={async () => {
          await load();
          await onAssigned();
        }}
        onGoToSettings={onGoToSettings}
      />
      {hint && <p className="px-1 text-xs text-zinc-500">{hint}</p>}
    </div>
  );
}
