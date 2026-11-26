import { api } from "../lib/api";
import type { ProviderAccountView, TaskModelRoute, TaskType } from "../lib/types";
import { findSelectedModel, type ModelSelection } from "./ModelPicker";

export function routeIsSet(route: Pick<TaskModelRoute, "provider_account_id" | "model_id"> | undefined): boolean {
  return Boolean(route?.provider_account_id && route?.model_id);
}

/** "Account - Model" for the chip, falling back to the raw model id when the account or model is gone. */
export function routeLabel(providers: ProviderAccountView[], route: TaskModelRoute | undefined): string {
  if (!route || !routeIsSet(route)) return "No model";
  const found = findSelectedModel(providers, { providerAccountId: route.provider_account_id, modelId: route.model_id });
  return found ? `${found.account.label} - ${found.model.display_name}` : (route.model_id ?? "No model");
}

/** Saves a model for one task, keeping the route's other settings (LLM-22). Takes effect on the next call. */
export function saveRouteSelection(taskType: TaskType, route: TaskModelRoute | undefined, selection: ModelSelection) {
  return api.llmSaveTaskRoute({
    task_type: taskType,
    provider_account_id: selection.providerAccountId,
    model_id: selection.modelId,
    reasoning_effort: route?.reasoning_effort ?? null,
    temperature: route?.temperature ?? null,
    fallback_account_id: route?.fallback_account_id ?? null,
    fallback_model_id: route?.fallback_model_id ?? null,
    max_calls_per_hour: route?.max_calls_per_hour ?? null,
  });
}
