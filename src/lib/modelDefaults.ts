import { api } from "./api";
import type { ModelInfo, ProviderAccountView, ProviderType, TaskModelRoute, TaskModelRoutePatch, TaskType } from "./types";
import { TASK_METAS } from "./types";

/** Which connected provider to prefer when several are connected. Experimental Grok tab is last and text-only. */
const PROVIDER_ORDER: ProviderType[] = ["anthropic_api", "claude_cli", "xai_api", "xai_oauth", "grok_web"];

/** Ordered id patterns per provider: the first model that matches is the balanced default. */
const BALANCED: Record<ProviderType, RegExp[]> = {
  anthropic_api: [/^claude-sonnet-4-5$/, /^claude-sonnet/, /sonnet/],
  claude_cli: [/^sonnet$/, /sonnet/],
  xai_api: [/^grok-4\.6$/, /^grok-4/, /^grok/],
  xai_oauth: [/^grok-4\.6$/, /^grok-4/, /^grok/],
  grok_web: [/./],
};

/** Cheap and fast, for high-volume scoring. */
const FAST: Record<ProviderType, RegExp[]> = {
  anthropic_api: [/^claude-haiku-4-5$/, /haiku/],
  claude_cli: [/^haiku$/, /haiku/],
  xai_api: [/mini/],
  xai_oauth: [/mini/],
  grok_web: [],
};

const FAST_TASKS: TaskType[] = ["post_scoring"];

function pick(models: ModelInfo[], patterns: RegExp[], needVision: boolean): ModelInfo | null {
  const pool = models.filter((m) => !needVision || m.capabilities.vision);
  for (const re of patterns) {
    const hit = pool.find((m) => re.test(m.id));
    if (hit) return hit;
  }
  return null;
}

export function connectedProviders(providers: ProviderAccountView[]): ProviderAccountView[] {
  return providers
    .filter((p) => p.status === "connected" && p.models.length > 0)
    .sort((a, b) => PROVIDER_ORDER.indexOf(a.provider_type) - PROVIDER_ORDER.indexOf(b.provider_type));
}

/** The model the app would choose for a task, or null when nothing connected can run it. */
export function recommendedModel(task: TaskType, providers: ProviderAccountView[]): { account: ProviderAccountView; model: ModelInfo } | null {
  const needVision = TASK_METAS.find((t) => t.id === task)?.requiresVision ?? false;
  for (const account of connectedProviders(providers)) {
    const type = account.provider_type;
    if (FAST_TASKS.includes(task)) {
      const fast = pick(account.models, FAST[type], needVision);
      if (fast) return { account, model: fast };
    }
    const balanced = pick(account.models, BALANCED[type], needVision);
    if (balanced) return { account, model: balanced };
  }
  return null;
}

export function isUnset(r: TaskModelRoute): boolean {
  return !r.provider_account_id || !r.model_id;
}

/** Patches that fill only the tasks with no model yet. Existing choices are never overwritten. */
export function recommendRoutes(providers: ProviderAccountView[], routes: TaskModelRoute[]): TaskModelRoutePatch[] {
  const patches: TaskModelRoutePatch[] = [];
  for (const r of routes) {
    if (!isUnset(r)) continue;
    const rec = recommendedModel(r.task_type, providers);
    if (!rec) continue;
    patches.push({
      task_type: r.task_type,
      provider_account_id: rec.account.id,
      model_id: rec.model.id,
      reasoning_effort: r.reasoning_effort,
      temperature: r.temperature,
      fallback_account_id: r.fallback_account_id,
      fallback_model_id: r.fallback_model_id,
      max_calls_per_hour: r.max_calls_per_hour,
    });
  }
  return patches;
}

/** Same model for every task that can use it (vision tasks keep a vision-capable model). */
export function routesForAll(account: ProviderAccountView, model: ModelInfo, routes: TaskModelRoute[]): TaskModelRoutePatch[] {
  return routes.map((r) => {
    const needVision = TASK_METAS.find((t) => t.id === r.task_type)?.requiresVision ?? false;
    const usable = !needVision || model.capabilities.vision;
    const rec = usable ? null : recommendedModel(r.task_type, [account]);
    const target = usable ? { account, model } : rec;
    return {
      task_type: r.task_type,
      provider_account_id: target ? target.account.id : r.provider_account_id,
      model_id: target ? target.model.id : r.model_id,
      reasoning_effort: r.reasoning_effort,
      temperature: r.temperature,
      fallback_account_id: r.fallback_account_id,
      fallback_model_id: r.fallback_model_id,
      max_calls_per_hour: r.max_calls_per_hour,
    };
  });
}

/** The model most tasks share, for the single "Default model" picker. */
export function commonRoute(routes: TaskModelRoute[]): { providerAccountId: string; modelId: string } | null {
  const counts = new Map<string, number>();
  for (const r of routes) {
    if (isUnset(r)) continue;
    const key = `${r.provider_account_id}::${r.model_id}`;
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  let best: string | null = null;
  for (const [k, n] of counts) if (best === null || n > (counts.get(best) ?? 0)) best = k;
  if (!best) return null;
  const i = best.indexOf("::");
  return { providerAccountId: best.slice(0, i), modelId: best.slice(i + 2) };
}

/** Pull live model lists from connected API-key providers (or just one provider when `onlyProviderId` is given). A failure keeps the built-in list and is reported, not thrown. */
export async function refreshLiveModels(
  providers: ProviderAccountView[],
  onlyProviderId?: string,
): Promise<{ fetched: Record<string, number>; errors: Record<string, string> }> {
  const fetched: Record<string, number> = {};
  const errors: Record<string, string> = {};
  await Promise.all(
    providers
      .filter(
        (p) =>
          (p.provider_type === "anthropic_api" || p.provider_type === "xai_api") &&
          p.status === "connected" &&
          (!onlyProviderId || p.id === onlyProviderId),
      )
      .map(async (p) => {
        try {
          fetched[p.id] = (await api.llmRefreshProviderModels(p.id)).length;
        } catch (e) {
          errors[p.id] = (e as Error).message;
        }
      }),
  );
  return { fetched, errors };
}

/** Called after a provider connects: refresh live models for that provider only, then give every unset task a sensible model. */
export async function autoAssignDefaults(onlyProviderId?: string): Promise<{ assigned: number; label: string | null }> {
  let providers = await api.llmListProviders();
  if (onlyProviderId) {
    await refreshLiveModels(providers, onlyProviderId);
    providers = await api.llmListProviders();
  }
  const routes = await api.llmListTaskRoutes();
  const patches = recommendRoutes(providers, routes);
  for (const p of patches) await api.llmSaveTaskRoute(p);
  const first = patches[0] ? providers.flatMap((a) => a.models.map((m) => ({ a, m }))).find((x) => x.a.id === patches[0].provider_account_id && x.m.id === patches[0].model_id) : null;
  return { assigned: patches.length, label: first ? `${first.m.display_name}` : null };
}
