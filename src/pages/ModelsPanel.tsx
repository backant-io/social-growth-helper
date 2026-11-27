import { useEffect, useState, type ReactNode } from "react";
import { ModelPicker, type ModelSelection } from "../components/ModelPicker";
import { saveRouteSelection } from "../components/modelRoute";
import { btn, btnGhost, btnPrimary, input, muted } from "../components/ui";
import { api } from "../lib/api";
import { parseLlmError } from "../lib/errors";
import { autoAssignDefaults, commonRoute, connectedProviders, recommendRoutes, recommendedModel, refreshLiveModels, routesForAll } from "../lib/modelDefaults";
import type { ClaudeCliDetectionResponse, ProviderAccountView, ProviderHealth, ProviderType, TaskModelRoute, TaskType, XaiDeviceChallenge } from "../lib/types";
import { TASK_METAS } from "../lib/types";

interface Props {
  providers: ProviderAccountView[];
  routes: TaskModelRoute[];
  offline: boolean;
  onChanged: () => Promise<void>;
  /** Setup wizard: only the provider list and the default model. */
  compact?: boolean;
}

interface ProviderDef {
  type: ProviderType;
  name: string;
  tagline: string;
  lane: string;
}

const PRIMARY: ProviderDef[] = [
  { type: "anthropic_api", name: "Anthropic", tagline: "Claude through an API key", lane: "API key" },
  { type: "claude_cli", name: "Claude Code", tagline: "Your local claude login, no key needed", lane: "Subscription" },
  { type: "xai_api", name: "xAI", tagline: "Grok through an API key", lane: "API key" },
  { type: "xai_oauth", name: "SuperGrok", tagline: "Sign in with your SuperGrok account", lane: "Subscription" },
];
const EXPERIMENTAL: ProviderDef = { type: "grok_web", name: "Grok in your X window", tagline: "Experimental, text only, slower", lane: "Subscription" };

const STATUS: Record<ProviderHealth, string> = {
  connected: "Connected",
  needs_login: "Needs login",
  expired: "Expired",
  offline: "Offline",
  error: "Error",
  disconnected: "Not connected",
};

function Dot({ status }: { status: ProviderHealth | "none" }) {
  const tone = status === "connected" ? "bg-emerald-600" : status === "none" ? "bg-zinc-300 dark:bg-zinc-700" : "bg-amber-500";
  return <span aria-hidden="true" className={`h-2 w-2 shrink-0 rounded-full ${tone}`} />;
}

function KeyForm({ label, placeholder, stored, disabled, onSave }: { label: string; placeholder: string; stored: boolean; disabled: boolean; onSave: (key: string) => Promise<void> }) {
  const [key, setKey] = useState("");
  return (
    <div className="flex flex-wrap gap-2">
      <input type="password" aria-label={label} className={`${input} min-w-[16rem] flex-1`} placeholder={stored ? "Key saved in your keychain. Paste a new one to replace it." : placeholder} value={key} onChange={(e) => setKey(e.target.value)} />
      <button
        type="button"
        className={btnPrimary}
        disabled={disabled || key.trim().length < 8}
        onClick={async () => {
          await onSave(key);
          setKey("");
        }}
      >
        Connect
      </button>
    </div>
  );
}

function ProviderRow({
  def,
  acct,
  open,
  onToggle,
  children,
}: {
  def: ProviderDef;
  acct: ProviderAccountView | undefined;
  open: boolean;
  onToggle: () => void;
  children: ReactNode;
}) {
  const status = acct?.status ?? "none";
  return (
    <li className="border-b border-zinc-200 py-3 dark:border-zinc-800">
      <button type="button" onClick={onToggle} aria-expanded={open} className="flex w-full items-center gap-3 text-left">
        <Dot status={status} />
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-medium">
            {def.name} <span className="eyebrow ml-1.5 align-middle">{def.lane}</span>
          </span>
          <span className={`block text-xs ${muted}`}>{acct ? `${STATUS[acct.status]}${acct.models.length ? ` / ${acct.models.length} models` : ""}` : def.tagline}</span>
        </span>
        <span className={`text-xs ${open ? "text-zinc-950 dark:text-zinc-50" : "text-zinc-500"}`}>{open ? "Close" : acct ? "Manage" : "Connect"}</span>
      </button>
      {open && <div className="mt-3 space-y-3 pl-5">{children}</div>}
    </li>
  );
}

export function ModelsPanel({ providers, routes, offline, onChanged, compact }: Props) {
  const [open, setOpen] = useState<ProviderType | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [cliPath, setCliPath] = useState("");
  const [cliDetect, setCliDetect] = useState<ClaudeCliDetectionResponse | null>(null);
  const [challenge, setChallenge] = useState<XaiDeviceChallenge | null>(null);
  const [oauthStatus, setOauthStatus] = useState<string | null>(null);
  const [showAll, setShowAll] = useState(false);

  const find = (t: ProviderType) => providers.find((p) => p.provider_type === t);
  const connected = connectedProviders(providers);

  const run = async (tag: string, fn: () => Promise<void>, assignForProvider?: string) => {
    setBusy(tag);
    setError(null);
    setNote(null);
    try {
      await fn();
      if (assignForProvider) {
        setOpen(null);
        const r = await autoAssignDefaults(assignForProvider);
        if (r.assigned > 0) setNote(`Ready. ${r.label ?? "A default model"} is now used for ${r.assigned} task${r.assigned === 1 ? "" : "s"}. Change it below any time.`);
        else setNote("Connected. Models refreshed from the provider.");
      }
      await onChanged();
    } catch (e) {
      setError(parseLlmError(e).message);
    } finally {
      setBusy(null);
    }
  };

  const connectedControls = (acct: ProviderAccountView) => (
    <div className="flex flex-wrap items-center gap-2">
      <button type="button" className={btn} disabled={offline || busy !== null} onClick={() => run(acct.id, async () => void (await api.llmHealthCheckProvider(acct.id)))}>
        Check connection
      </button>
      <button type="button" className={btnGhost} disabled={busy !== null} onClick={() => run(acct.id, () => api.llmDisconnectProvider(acct.id))}>
        Disconnect
      </button>
      {acct.last_error && <span className="text-xs text-red-600 dark:text-red-400">{acct.last_error}</span>}
    </div>
  );

  const rowFor = (def: ProviderDef) => {
    const acct = find(def.type);
    return (
      <ProviderRow key={def.type} def={def} acct={acct} open={open === def.type} onToggle={() => setOpen(open === def.type ? null : def.type)}>
        {def.type === "anthropic_api" && (
          <KeyForm label="Anthropic API key" placeholder="sk-ant-..." stored={!!acct?.has_secret} disabled={offline || busy !== null} onSave={(k) => run("save-anthropic", async () => void (await api.llmSaveApiKeyProvider("anthropic_api", null, k)), "anthropic_api")} />
        )}
        {def.type === "xai_api" && (
          <KeyForm label="xAI API key" placeholder="xai-..." stored={!!acct?.has_secret} disabled={offline || busy !== null} onSave={(k) => run("save-xai", async () => void (await api.llmSaveApiKeyProvider("xai_api", null, k)), "xai_api")} />
        )}
        {def.type === "claude_cli" && (
          <>
            <p className={`text-xs ${muted}`}>
              Uses your own <code>claude</code> login with tools turned off. Personal use only. Never reads session tokens.
            </p>
            <div className="flex flex-wrap gap-2">
              <input aria-label="Claude binary override" className={`${input} min-w-[14rem] flex-1`} placeholder="Optional: path to the claude binary" value={cliPath} onChange={(e) => setCliPath(e.target.value)} />
              <button type="button" className={btn} disabled={busy !== null} onClick={() => run("detect-cli", async () => setCliDetect(await api.llmDetectClaudeCli(cliPath.trim() || null)))}>
                Detect
              </button>
              <button type="button" className={btnPrimary} disabled={offline || busy !== null} onClick={() => run("connect-cli", async () => void (await api.llmConnectClaudeCli(null, cliPath.trim() || null)), "claude_cli")}>
                Connect
              </button>
            </div>
            {cliDetect && (
              <p className={`text-xs ${muted}`}>
                {cliDetect.detection.found ? `Found ${cliDetect.detection.binary_path} (${cliDetect.detection.version ?? "unknown version"})` : (cliDetect.detection.error ?? "claude not found. Install Claude Code and run claude login.")}
              </p>
            )}
          </>
        )}
        {def.type === "xai_oauth" && (
          <>
            {!challenge ? (
              <button
                type="button"
                className={btnPrimary}
                disabled={offline || busy !== null}
                onClick={() =>
                  run("xai-start", async () => {
                    const ch = await api.llmXaiOAuthStart();
                    setChallenge(ch);
                    setOauthStatus(null);
                  })
                }
              >
                Start sign-in
              </button>
            ) : (
              <div className="space-y-2 text-sm">
                <p>
                  Open{" "}
                  <a href={challenge.verification_uri_complete ?? challenge.verification_uri} target="_blank" rel="noreferrer" className="underline">
                    {challenge.verification_uri}
                  </a>{" "}
                  and enter <code className="rounded bg-zinc-100 px-1.5 py-0.5 font-semibold dark:bg-zinc-800">{challenge.user_code}</code>
                </p>
                <button
                  type="button"
                  className={btnPrimary}
                  disabled={offline || busy !== null}
                  onClick={() =>
                    run(
                      "xai-poll",
                      async () => {
                        const res = await api.llmXaiOAuthPoll(challenge.token_endpoint, challenge.device_code, "https://api.x.ai/v1", null);
                        if (res.outcome.state === "authorized") {
                          setChallenge(null);
                          setOauthStatus("Signed in.");
                        } else if (res.outcome.state === "pending") setOauthStatus("Still waiting for approval in your browser.");
                        else {
                          setChallenge(null);
                          setOauthStatus(`Sign-in ${res.outcome.state}. Start again.`);
                        }
                      },
                      "xai_oauth",
                    )
                  }
                >
                  I approved it
                </button>
              </div>
            )}
            {oauthStatus && <p className={`text-xs ${muted}`}>{oauthStatus}</p>}
          </>
        )}
        {def.type === "grok_web" && (
          <>
            <p className={`text-xs ${muted}`}>Drives x.com/i/grok inside your X window. Fragile; prefer an API key or sign-in.</p>
            <button type="button" className={btn} disabled={offline || busy !== null} onClick={() => run("grok-web", async () => void (await api.llmConnectGrokWeb(null)), "grok_web")}>
              {acct ? "Re-check" : "Enable"}
            </button>
          </>
        )}
        {acct && connectedControls(acct)}
      </ProviderRow>
    );
  };

  const shared = commonRoute(routes);
  const sharedSel: ModelSelection = shared ? { providerAccountId: shared.providerAccountId, modelId: shared.modelId } : { providerAccountId: null, modelId: null };
  const anyUnset = routes.some((r) => !r.provider_account_id || !r.model_id);

  const applyToAll = async (sel: ModelSelection) => {
    const account = providers.find((p) => p.id === sel.providerAccountId);
    const model = account?.models.find((m) => m.id === sel.modelId);
    if (!account || !model) return;
    await run("default", async () => {
      for (const patch of routesForAll(account, model, routes)) await api.llmSaveTaskRoute(patch);
      setNote(`${model.display_name} is now the default for every task.`);
    });
  };

  const useRecommended = () =>
    run("recommended", async () => {
      const patches = recommendRoutes(providers, routes.map((r) => ({ ...r, provider_account_id: null, model_id: null })));
      for (const p of patches) await api.llmSaveTaskRoute(p);
      setNote("Recommended models applied.");
    });

  const hasKeyProvider = providers.some((p) => (p.provider_type === "anthropic_api" || p.provider_type === "xai_api") && p.status === "connected");

  return (
    <div className="flex flex-col gap-8">
      {error && (
        <div role="alert" className="order-first rounded-lg border border-red-200 px-3.5 py-2.5 text-sm text-red-800 dark:border-red-900 dark:text-red-200">
          {error}
        </div>
      )}
      {note && (
        <p role="status" data-testid="models-note" className="order-first text-sm text-emerald-700 dark:text-emerald-400">
          {note}
        </p>
      )}

      <section aria-labelledby="prov-h">
        <h2 id="prov-h" className="eyebrow mb-1">
          {compact && connected.length === 0 ? "Pick one" : "Providers"}
        </h2>
        <ul className="border-t border-zinc-200 dark:border-zinc-800">
          {PRIMARY.map(rowFor)}
          {(showAll || find("grok_web")) && rowFor(EXPERIMENTAL)}
        </ul>
        {!showAll && !find("grok_web") && (
          <button type="button" className={`${btnGhost} mt-2`} onClick={() => setShowAll(true)}>
            Show experimental options
          </button>
        )}
      </section>

      {connected.length > 0 && (
        <section aria-labelledby="def-h" className={`space-y-3 ${compact ? "order-first" : ""}`}>
          <div className="flex flex-wrap items-end justify-between gap-2">
            <div>
              <h2 id="def-h" className="eyebrow mb-1">
                Default model
              </h2>
              <p className={`text-sm ${muted}`}>Used for every task unless you pick another below. Sensible defaults are already chosen.</p>
            </div>
            <div className="flex gap-2">
              {hasKeyProvider && (
                <button type="button" className={btn} disabled={offline || busy !== null} onClick={() => run("fetch", async () => {
                  const res = await refreshLiveModels(providers);
                  const n = Object.values(res.fetched).reduce((a, b) => a + b, 0);
                  const failed = Object.values(res.errors)[0];
                  if (failed) throw new Error(failed);
                  setNote(`Fetched ${n} models from your provider APIs.`);
                })}>
                  Fetch live models
                </button>
              )}
              {(anyUnset || !compact) && (
                <button type="button" className={btnGhost} disabled={busy !== null} onClick={() => void useRecommended()}>
                  Use recommended
                </button>
              )}
            </div>
          </div>
          <ModelPicker providers={connected} value={sharedSel} allowUnset={!shared} unsetLabel="Choose a model" ariaLabel="Default model" onChange={(sel) => void applyToAll(sel)} onProvidersUpdated={onChanged} />
        </section>
      )}

      {!compact && connected.length > 0 && <TaskModels providers={connected} routes={routes} onChanged={onChanged} />}
    </div>
  );
}

function TaskModels({ providers, routes, onChanged }: { providers: ProviderAccountView[]; routes: TaskModelRoute[]; onChanged: () => Promise<void> }) {
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState<TaskType | null>(null);

  const change = async (route: TaskModelRoute, sel: ModelSelection) => {
    setSaving(route.task_type);
    setError(null);
    try {
      await saveRouteSelection(route.task_type, route, sel);
      await onChanged();
    } catch (e) {
      setError(parseLlmError(e).message);
    } finally {
      setSaving(null);
    }
  };

  return (
    <details className="group border-t border-zinc-200 pt-4 dark:border-zinc-800">
      <summary className="flex cursor-pointer list-none items-center justify-between text-sm font-medium">
        <span>Choose a model per task</span>
        <span className="text-xs text-zinc-500 group-open:hidden">Optional</span>
      </summary>
      {error && <p role="alert" className="mt-2 text-sm text-red-600 dark:text-red-400">{error}</p>}
      <ul className="mt-3 divide-y divide-zinc-200 dark:divide-zinc-800">
        {TASK_METAS.map((meta) => {
          const r = routes.find((x) => x.task_type === meta.id);
          if (!r) return null;
          const rec = recommendedModel(meta.id, providers);
          return (
            <li key={meta.id} className="grid gap-2 py-3 md:grid-cols-[1fr_auto] md:items-center">
              <div className="min-w-0">
                <div className="text-sm font-medium">
                  {meta.label}
                  {meta.requiresVision && <span className="eyebrow ml-2 align-middle">Vision</span>}
                </div>
                <div className={`text-xs ${muted}`}>{meta.description}</div>
                {(!r.provider_account_id || !r.model_id) && rec && <div className="text-xs text-zinc-500">Suggested: {rec.model.display_name}</div>}
              </div>
              <ModelPicker
                providers={providers}
                value={{ providerAccountId: r.provider_account_id, modelId: r.model_id }}
                requireVision={meta.requiresVision}
                compact
                ariaLabel={`Model for ${meta.label}`}
                onProvidersUpdated={onChanged}
                onChange={(sel) => void change(r, sel)}
              />
              {saving === meta.id && <span className="text-xs text-zinc-500">Saving...</span>}
            </li>
          );
        })}
      </ul>
      <AdvancedRoutes routes={routes} providers={providers} onChanged={onChanged} />
    </details>
  );
}

function AdvancedRoutes({ routes, providers, onChanged }: { routes: TaskModelRoute[]; providers: ProviderAccountView[]; onChanged: () => Promise<void> }) {
  const [drafts, setDrafts] = useState<Record<string, TaskModelRoute>>({});
  const [msg, setMsg] = useState<string | null>(null);
  useEffect(() => {
    const next: Record<string, TaskModelRoute> = {};
    for (const r of routes) next[r.task_type] = r;
    setDrafts(next);
  }, [routes]);
  const upd = (t: TaskType, patch: Partial<TaskModelRoute>) => setDrafts((d) => ({ ...d, [t]: { ...d[t], ...patch } }));
  const save = async (t: TaskType) => {
    const r = drafts[t];
    await api.llmSaveTaskRoute({
      task_type: t,
      provider_account_id: r.provider_account_id,
      model_id: r.model_id,
      reasoning_effort: r.reasoning_effort,
      temperature: r.temperature,
      fallback_account_id: r.fallback_account_id,
      fallback_model_id: r.fallback_model_id,
      max_calls_per_hour: r.max_calls_per_hour,
    });
    setMsg("Saved.");
    await onChanged();
  };
  return (
    <details className="mt-4">
      <summary className="cursor-pointer text-xs text-zinc-500 hover:text-zinc-950 dark:hover:text-zinc-50">Advanced: fallbacks, reasoning, temperature, call limits</summary>
      <p className={`mt-2 text-xs ${muted}`}>A fallback only runs when you set one here. Nothing falls back silently.</p>
      {msg && <p role="status" className="text-xs text-emerald-700 dark:text-emerald-400">{msg}</p>}
      <ul className="mt-2 space-y-4">
        {TASK_METAS.map((meta) => {
          const r = drafts[meta.id];
          if (!r) return null;
          return (
            <li key={meta.id} className="space-y-2 border-t border-zinc-200 pt-3 text-xs dark:border-zinc-800">
              <div className="text-sm font-medium">{meta.label}</div>
              <div className="flex flex-wrap items-center gap-3">
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Reasoning</span>
                  <select className={input} value={r.reasoning_effort ?? ""} onChange={(e) => upd(meta.id, { reasoning_effort: e.target.value || null })}>
                    <option value="">Default</option>
                    <option value="low">low</option>
                    <option value="medium">medium</option>
                    <option value="high">high</option>
                  </select>
                </label>
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Temp</span>
                  <input type="number" step="0.1" min="0" max="2" className={`${input} w-20`} placeholder="0.7" value={r.temperature ?? ""} onChange={(e) => upd(meta.id, { temperature: e.target.value === "" ? null : Number(e.target.value) })} />
                </label>
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Max calls / hr</span>
                  <input type="number" min="1" max="10000" className={`${input} w-24`} placeholder="No cap" value={r.max_calls_per_hour ?? ""} onChange={(e) => upd(meta.id, { max_calls_per_hour: e.target.value === "" ? null : Number(e.target.value) })} />
                </label>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <span className="text-zinc-500">Fallback</span>
                <ModelPicker
                  providers={providers}
                  value={{ providerAccountId: r.fallback_account_id, modelId: r.fallback_model_id }}
                  requireVision={meta.requiresVision}
                  unsetLabel="No fallback (stop on failure)"
                  compact
                  ariaLabel={`Fallback model for ${meta.label}`}
                  onChange={(sel) => upd(meta.id, { fallback_account_id: sel.providerAccountId, fallback_model_id: sel.modelId })}
                />
                <button type="button" className={btn} onClick={() => void save(meta.id)}>
                  Save
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </details>
  );
}
