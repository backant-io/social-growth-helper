import { useCallback, useEffect, useState } from "react";
import { ModelPicker, type ModelSelection } from "../components/ModelPicker";
import { api } from "../lib/api";
import { browserNotices } from "../lib/browserNotices";
import { parseLlmError } from "../lib/errors";
import type {
  AppInfo,
  BrowserInfo,
  ClaudeCliDetectionResponse,
  LayoutStatus,
  PromptTemplate,
  ProviderAccountView,
  ProviderHealth,
  ProviderType,
  RulesStatus,
  SecretsStatus,
  TaskModelRoute,
  TaskType,
  UsageDashboard,
  VisibleTweet,
  XaiDeviceChallenge,
} from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { DataPanel } from "../learning/DataPanel";
import { PreferencesPanel } from "../onboarding/PreferencesStep";

interface Props {
  rules: RulesStatus | null;
  offline: boolean;
  onReplayTour: () => void;
  onReadRules: () => void;
  onDataChanged: () => void;
}

type SettingsTab = "general" | "providers" | "routing" | "usage" | "prompts";

const panel = "rounded-xl border border-zinc-200 p-4 dark:border-zinc-800";
const btn =
  "rounded-lg border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";
const primaryBtn =
  "rounded-lg bg-zinc-900 px-3 py-1.5 text-sm font-medium text-white hover:bg-zinc-800 disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200";
const inputCls =
  "min-w-0 rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900";

function StatusBadge({ status }: { status: ProviderHealth }) {
  const styles: Record<ProviderHealth, string> = {
    connected: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300",
    needs_login: "bg-amber-100 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300",
    expired: "bg-amber-100 text-amber-800 dark:bg-amber-950/60 dark:text-amber-300",
    offline: "bg-red-100 text-red-800 dark:bg-red-950/60 dark:text-red-300",
    error: "bg-red-100 text-red-800 dark:bg-red-950/60 dark:text-red-300",
    disconnected: "bg-zinc-200 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300",
  };
  return <span className={`rounded px-2 py-0.5 text-xs font-medium ${styles[status]}`}>{status.replace("_", " ")}</span>;
}

function SecurityPanel() {
  const [status, setStatus] = useState<SecretsStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const check = useCallback(async () => {
    setBusy(true);
    try {
      setStatus(await api.secretsStatus());
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    void check();
  }, [check]);

  return (
    <section className={panel} aria-labelledby="sec-h">
      <div className="flex items-center justify-between">
        <h2 id="sec-h" className="font-medium">
          Security
        </h2>
        <button type="button" className={btn} onClick={check} disabled={busy}>
          Re-check
        </button>
      </div>
      {status && (
        <div className="mt-3 text-sm">
          <p className={status.available ? "text-emerald-600 dark:text-emerald-400" : "text-red-600 dark:text-red-400"}>
            {status.available ? "Secret storage is ready." : "Secret storage is not available."}{" "}
            <span className="text-zinc-500">({status.backend})</span>
          </p>
          {status.issue && <p className="mt-1 text-zinc-600 dark:text-zinc-400">{status.issue}</p>}
          {status.guide && (
            <div className="mt-3 space-y-3">
              <p className="font-medium">{status.guide.title}</p>
              <p className="text-zinc-600 dark:text-zinc-400">{status.guide.summary}</p>
              {status.guide.options.map((o) => (
                <details key={o.name} className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
                  <summary className="cursor-pointer font-medium">{o.name}</summary>
                  <ol className="mt-2 list-decimal space-y-1 pl-5">
                    {o.steps.map((s) => (
                      <li key={s}>
                        <code className="text-xs">{s}</code>
                      </li>
                    ))}
                  </ol>
                </details>
              ))}
              <p className="text-zinc-600 dark:text-zinc-400">{status.guide.verify}</p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function BrowserPanel({ rulesAcknowledged }: { rulesAcknowledged: boolean }) {
  const [info, setInfo] = useState<BrowserInfo | null>(null);
  const [path, setPath] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [tweets, setTweets] = useState<VisibleTweet[] | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    const i = await api.browserInfo();
    setInfo(i);
    setPath(i.override_path ?? "");
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setMessage(null);
    try {
      await fn();
    } catch (e) {
      setMessage((e as Error).message);
    } finally {
      setBusy(false);
      await refresh();
    }
  };

  return (
    <section className={panel} aria-labelledby="br-h">
      <h2 id="br-h" className="font-medium">
        X browser window
      </h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
        The app opens your real Chrome in its own window with a separate profile. Log in to X there once; the login is kept
        for next time. X is never loaded inside this app.
      </p>
      {info && (
        <div className="mt-3 space-y-3 text-sm">
          <p>
            {info.install ? (
              <>
                Using <strong>{info.install.kind}</strong> at <code className="text-xs">{info.install.path}</code>
              </>
            ) : (
              <span className="text-red-600 dark:text-red-400">{info.problem}</span>
            )}
          </p>
          {browserNotices(info).map((n) => (
            <p
              key={n.id}
              role="status"
              className="rounded-lg border border-amber-300 bg-amber-50 p-2 text-amber-800 dark:border-amber-700 dark:bg-amber-950 dark:text-amber-200"
            >
              {n.text}
            </p>
          ))}
          <div className="flex gap-2">
            <input
              aria-label="Browser path override"
              className={`${inputCls} flex-1`}
              placeholder="Optional: path to a Chrome, Chromium or Brave binary"
              value={path}
              onChange={(e) => setPath(e.target.value)}
            />
            <button type="button" className={btn} disabled={busy} onClick={() => run(() => api.setBrowserPath(path.trim() || null))}>
              Save path
            </button>
          </div>
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={btn}
              disabled={busy || info.running || !rulesAcknowledged}
              onClick={() => run(() => api.launchX())}
            >
              Open X window
            </button>
            <button type="button" className={btn} disabled={busy || !info.running} onClick={() => run(() => api.closeX())}>
              Close X window
            </button>
            <button
              type="button"
              className={btn}
              disabled={busy || !info.running}
              onClick={() => run(async () => setTweets(await api.readVisibleTweets()))}
            >
              Read visible tweets (dev)
            </button>
          </div>
          {!rulesAcknowledged && (
            <p className="text-amber-600 dark:text-amber-400">Acknowledge the rules of engagement to open the X window.</p>
          )}
          {message && (
            <p role="alert" className="text-red-600 dark:text-red-400">
              {message}
            </p>
          )}
          {tweets && (
            <div>
              <p className="mb-1 text-zinc-500">{tweets.length} visible tweet(s). Read-only.</p>
              <ul className="max-h-48 space-y-1 overflow-y-auto">
                {tweets.map((t, i) => (
                  <li key={t.id ?? i} className="rounded border border-zinc-200 p-2 text-xs dark:border-zinc-800">
                    <strong>{t.author}</strong> {t.handle && <span className="text-zinc-500">@{t.handle}</span>}{" "}
                    {t.promoted && <em>(promoted)</em>}
                    <div>{t.text}</div>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function LayoutPanel() {
  const [status, setStatus] = useState<LayoutStatus | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void api.windowLayoutStatus().then(setStatus);
  }, []);

  const run = async (fn: () => Promise<LayoutStatus>, done: string) => {
    setBusy(true);
    setMessage(null);
    try {
      setStatus(await fn());
      setMessage(done);
    } catch (e) {
      setMessage((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  const off = !status?.placement_supported;
  return (
    <section className={panel} aria-labelledby="lay-h">
      <h2 id="lay-h" className="font-medium">
        Window layout
      </h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
        Opening the X window puts it beside this one. Move them where you like, then save; the layout is restored next time.
      </p>
      {status?.hint && (
        <p role="status" className="mt-2 text-sm text-amber-700 dark:text-amber-300">
          {status.hint}
        </p>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" className={btn} disabled={busy || off} onClick={() => run(api.saveWindowLayout, "Layout saved.")}>
          Save current layout
        </button>
        <button type="button" className={btn} disabled={busy || off} onClick={() => run(api.applyWindowLayout, "Layout applied.")}>
          Arrange side by side
        </button>
        <button
          type="button"
          className={btn}
          disabled={busy || off || !status?.saved}
          onClick={() => run(api.resetWindowLayout, "Saved layout cleared.")}
        >
          Reset
        </button>
      </div>
      {message && <p className="mt-2 text-sm text-zinc-500">{message}</p>}
    </section>
  );
}

function DiagnosticsPanel() {
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const copy = async () => {
    setBusy(true);
    setMessage(null);
    try {
      await navigator.clipboard.writeText(await api.diagnostics());
      setMessage("Diagnostics copied. Tokens and post text are redacted.");
    } catch (e) {
      setMessage((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className={panel} aria-labelledby="diag-h">
      <h2 id="diag-h" className="font-medium">
        Diagnostics
      </h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
        Logs stay on this computer and rotate automatically. Copy a redacted report to paste into a bug report.
      </p>
      <div className="mt-3">
        <button type="button" className={btn} disabled={busy} onClick={copy}>
          Copy diagnostics
        </button>
      </div>
      {message && (
        <p role="status" className="mt-2 text-sm text-zinc-500">
          {message}
        </p>
      )}
    </section>
  );
}

function ProvidersPanel({
  providers,
  offline,
  onChanged,
}: {
  providers: ProviderAccountView[];
  offline: boolean;
  onChanged: () => Promise<void>;
}) {
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Claude CLI state
  const [cliPath, setCliPath] = useState("");
  const [cliDetect, setCliDetect] = useState<ClaudeCliDetectionResponse | null>(null);

  // xAI OAuth device flow state
  const [challenge, setChallenge] = useState<XaiDeviceChallenge | null>(null);
  const [xaiEndpoint, setXaiEndpoint] = useState("https://api.x.ai/v1");
  const [oauthStatus, setOauthStatus] = useState<string | null>(null);

  // API key inputs
  const [xaiApiKey, setXaiApiKey] = useState("");
  const [anthropicApiKey, setAnthropicApiKey] = useState("");

  const findAccount = (pt: ProviderType) => providers.find((p) => p.provider_type === pt);

  const runAction = async (tag: string, fn: () => Promise<void>) => {
    setBusyId(tag);
    setError(null);
    try {
      await fn();
      await onChanged();
    } catch (e) {
      setError(parseLlmError(e).message);
    } finally {
      setBusyId(null);
    }
  };

  const renderAccountHeader = (title: string, lane: "subscription" | "api", acct?: ProviderAccountView) => (
    <div className="flex flex-wrap items-center justify-between gap-2">
      <div className="flex items-center gap-2">
        <h3 className="font-medium">{title}</h3>
        <span
          className={`rounded px-2 py-0.5 text-[11px] font-medium ${
            lane === "subscription"
              ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950/60 dark:text-emerald-300"
              : "bg-zinc-200 text-zinc-800 dark:bg-zinc-800 dark:text-zinc-200"
          }`}
        >
          {lane === "subscription" ? "Subscription" : "API Key"}
        </span>
      </div>
      {acct ? <StatusBadge status={acct.status} /> : <StatusBadge status="disconnected" />}
    </div>
  );

  const renderConnectedControls = (acct: ProviderAccountView) => (
    <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
      <button
        type="button"
        className={btn}
        disabled={offline || busyId !== null}
        onClick={() => runAction(acct.id, async () => { await api.llmHealthCheckProvider(acct.id); })}
      >
        Health check
      </button>
      {(acct.provider_type === "xai_api" || acct.provider_type === "anthropic_api") && (
        <button
          type="button"
          className={btn}
          disabled={offline || busyId !== null}
          onClick={() => runAction(acct.id, async () => { await api.llmRefreshProviderModels(acct.id); })}
        >
          Refresh models
        </button>
      )}
      <button
        type="button"
        className={btn}
        disabled={busyId !== null}
        onClick={() => runAction(acct.id, async () => { await api.llmDisconnectProvider(acct.id); })}
      >
        Disconnect
      </button>
      {acct.last_verified_at && <span className="text-zinc-500">Checked {acct.last_verified_at}</span>}
      {acct.last_error && <span className="text-red-600 dark:text-red-400">{acct.last_error}</span>}
    </div>
  );

  const claudeCli = findAccount("claude_cli");
  const xaiOAuth = findAccount("xai_oauth");
  const xaiApi = findAccount("xai_api");
  const anthropicApi = findAccount("anthropic_api");
  const grokWeb = findAccount("grok_web");

  return (
    <div className="space-y-4">
      {error && (
        <div role="alert" className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-800 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}

      {/* 1. Claude Code CLI */}
      <section className={panel}>
        {renderAccountHeader("Claude Code CLI", "subscription", claudeCli)}
        <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
          Uses your local unmodified <code className="text-xs">claude</code> CLI login with{" "}
          <code className="text-xs">--safe-mode</code> and tools disabled. Never extracts session tokens.
        </p>
        <div className="mt-2 rounded-lg bg-zinc-100 p-2.5 text-xs text-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
          <strong>Personal use notice (SEC-12):</strong>{" "}
          {cliDetect?.personal_use_notice ??
            claudeCli?.personal_use_notice ??
            "Personal single-user use only. Uses your local unmodified `claude` CLI login (`--safe-mode`). Any external distribution requires each user's own Anthropic API key or Linked Billing."}
        </div>
        <div className="mt-3 flex flex-wrap gap-2">
          <input
            aria-label="Claude binary override"
            className={`${inputCls} flex-1`}
            placeholder="Optional: path to claude binary"
            value={cliPath}
            onChange={(e) => setCliPath(e.target.value)}
          />
          <button
            type="button"
            className={btn}
            disabled={busyId !== null}
            onClick={() =>
              runAction("detect-cli", async () => {
                setCliDetect(await api.llmDetectClaudeCli(cliPath.trim() || null));
              })
            }
          >
            Detect CLI
          </button>
          <button
            type="button"
            className={primaryBtn}
            disabled={offline || busyId !== null}
            onClick={() =>
              runAction("connect-cli", async () => {
                await api.llmConnectClaudeCli(null, cliPath.trim() || null);
              })
            }
          >
            Connect &amp; Check
          </button>
        </div>
        {cliDetect && (
          <p className="mt-2 text-xs text-zinc-600 dark:text-zinc-400">
            {cliDetect.detection.found
              ? `Found ${cliDetect.detection.binary_path} (${cliDetect.detection.version ?? "unknown version"})`
              : cliDetect.detection.error ?? "claude binary not found. Install Claude Code CLI and run `claude login`."}
          </p>
        )}
        {claudeCli && renderConnectedControls(claudeCli)}
      </section>

      {/* 2. SuperGrok / xAI OAuth */}
      <section className={panel}>
        {renderAccountHeader("SuperGrok / xAI OAuth", "subscription", xaiOAuth)}
        <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
          Sign in with your SuperGrok / xAI account via device-code OAuth (RFC 8628). Tokens are stored in your OS Keychain.
        </p>
        <div className="mt-3 flex flex-wrap items-center gap-2">
          <select
            aria-label="xAI OAuth inference endpoint"
            className={inputCls}
            value={xaiEndpoint}
            onChange={(e) => setXaiEndpoint(e.target.value)}
          >
            <option value="https://api.x.ai/v1">api.x.ai/v1 (Standard)</option>
            <option value="https://cli-chat-proxy.grok.com/v1">cli-chat-proxy.grok.com/v1 (SuperGrok Proxy)</option>
          </select>
          <button
            type="button"
            className={primaryBtn}
            disabled={offline || busyId !== null}
            onClick={() =>
              runAction("xai-oauth-start", async () => {
                const ch = await api.llmXaiOAuthStart();
                setChallenge(ch);
                setOauthStatus(`Open ${ch.verification_uri} and enter code ${ch.user_code}, then click Complete Sign-In.`);
              })
            }
          >
            Start Sign-In
          </button>
        </div>
        {challenge && (
          <div className="mt-3 rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-xs dark:border-zinc-800 dark:bg-zinc-900">
            <div>
              Verification URL:{" "}
              <a
                href={challenge.verification_uri_complete ?? challenge.verification_uri}
                target="_blank"
                rel="noreferrer"
                className="underline"
              >
                {challenge.verification_uri}
              </a>
            </div>
            <div className="mt-1">
              User code: <code className="rounded bg-zinc-200 px-1.5 py-0.5 font-semibold dark:bg-zinc-800">{challenge.user_code}</code>
            </div>
            <div className="mt-2 flex items-center gap-2">
              <button
                type="button"
                className={primaryBtn}
                disabled={offline || busyId !== null}
                onClick={() =>
                  runAction("xai-oauth-poll", async () => {
                    const res = await api.llmXaiOAuthPoll(
                      challenge.token_endpoint,
                      challenge.device_code,
                      xaiEndpoint,
                      null
                    );
                    if (res.outcome.state === "authorized") {
                      setChallenge(null);
                      setOauthStatus("Authorized and saved to OS Keychain.");
                    } else if (res.outcome.state === "pending") {
                      setOauthStatus("Still waiting for approval in your browser—complete login and click Poll again.");
                    } else {
                      setChallenge(null);
                      setOauthStatus(`Sign-in ${res.outcome.state}. Start Sign-In again.`);
                    }
                  })
                }
              >
                Complete / Poll Sign-In
              </button>
              {oauthStatus && <span className="text-zinc-600 dark:text-zinc-400">{oauthStatus}</span>}
            </div>
          </div>
        )}
        {!challenge && oauthStatus && <p className="mt-2 text-xs text-emerald-600 dark:text-emerald-400">{oauthStatus}</p>}
        {xaiOAuth && renderConnectedControls(xaiOAuth)}
      </section>

      {/* 3. xAI API Key */}
      <section className={panel}>
        {renderAccountHeader("xAI API", "api", xaiApi)}
        <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
          Direct xAI API key stored in your OS Keychain. Never written to SQLite or logged.
        </p>
        <div className="mt-3 flex flex-wrap gap-2">
          <input
            type="password"
            aria-label="xAI API key"
            className={`${inputCls} flex-1`}
            placeholder={xaiApi?.has_secret ? "Key stored in Keychain (enter new key to replace)" : "xai-..."}
            value={xaiApiKey}
            onChange={(e) => setXaiApiKey(e.target.value)}
          />
          <button
            type="button"
            className={primaryBtn}
            disabled={offline || busyId !== null || xaiApiKey.trim().length < 8}
            onClick={() =>
              runAction("save-xai-api", async () => {
                await api.llmSaveApiKeyProvider("xai_api", null, xaiApiKey);
                setXaiApiKey("");
              })
            }
          >
            Save &amp; Verify
          </button>
        </div>
        {xaiApi && renderConnectedControls(xaiApi)}
      </section>

      {/* 4. Anthropic API Key */}
      <section className={panel}>
        {renderAccountHeader("Anthropic API", "api", anthropicApi)}
        <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
          Direct Anthropic Messages API key stored in your OS Keychain.
        </p>
        <div className="mt-3 flex flex-wrap gap-2">
          <input
            type="password"
            aria-label="Anthropic API key"
            className={`${inputCls} flex-1`}
            placeholder={anthropicApi?.has_secret ? "Key stored in Keychain (enter new key to replace)" : "sk-ant-..."}
            value={anthropicApiKey}
            onChange={(e) => setAnthropicApiKey(e.target.value)}
          />
          <button
            type="button"
            className={primaryBtn}
            disabled={offline || busyId !== null || anthropicApiKey.trim().length < 8}
            onClick={() =>
              runAction("save-ant-api", async () => {
                await api.llmSaveApiKeyProvider("anthropic_api", null, anthropicApiKey);
                setAnthropicApiKey("");
              })
            }
          >
            Save &amp; Verify
          </button>
        </div>
        {anthropicApi && renderConnectedControls(anthropicApi)}
      </section>

      {/* 5. Grok X Tab (Experimental) */}
      <section className={panel}>
        {renderAccountHeader("Grok X Tab (Experimental)", "subscription", grokWeb)}
        <p className="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
          Experimental fallback that drives <code className="text-xs">x.com/i/grok</code> inside your open X Chrome window.
          Text-only; slower and more fragile than API or OAuth.
        </p>
        <div className="mt-3">
          <button
            type="button"
            className={btn}
            disabled={offline || busyId !== null}
            onClick={() => runAction("connect-grok-web", async () => { await api.llmConnectGrokWeb(null); })}
          >
            {grokWeb ? "Re-check X Grok Tab" : "Enable Experimental Tab Provider"}
          </button>
        </div>
        {grokWeb && renderConnectedControls(grokWeb)}
      </section>
    </div>
  );
}

function RoutingPanel({
  providers,
  routes,
  onChanged,
}: {
  providers: ProviderAccountView[];
  routes: TaskModelRoute[];
  onChanged: () => Promise<void>;
}) {
  const [drafts, setDrafts] = useState<Record<string, TaskModelRoute>>({});
  const [savingTask, setSavingTask] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const next: Record<string, TaskModelRoute> = {};
    for (const r of routes) next[r.task_type] = r;
    setDrafts(next);
  }, [routes]);

  const updateDraft = (taskType: TaskType, patch: Partial<TaskModelRoute>) => {
    setDrafts((prev) => ({
      ...prev,
      [taskType]: { ...prev[taskType], ...patch },
    }));
  };

  const saveRow = async (taskType: TaskType) => {
    const r = drafts[taskType];
    if (!r) return;
    setSavingTask(taskType);
    setError(null);
    try {
      await api.llmSaveTaskRoute({
        task_type: taskType,
        provider_account_id: r.provider_account_id,
        model_id: r.model_id,
        reasoning_effort: r.reasoning_effort,
        temperature: r.temperature,
        fallback_account_id: r.fallback_account_id,
        fallback_model_id: r.fallback_model_id,
        max_calls_per_hour: r.max_calls_per_hour,
      });
      await onChanged();
    } catch (e) {
      setError(parseLlmError(e).message);
    } finally {
      setSavingTask(null);
    }
  };

  return (
    <div className="space-y-4">
      <p className="text-sm text-zinc-600 dark:text-zinc-400">
        Every task ships unset. Choose a primary provider and model per task. Fallbacks run only when explicitly
        configured—never silently.
      </p>
      {error && (
        <div role="alert" className="rounded-lg border border-red-300 bg-red-50 px-3 py-2 text-sm text-red-700 dark:border-red-800 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}
      <div className="space-y-3">
        {TASK_METAS.map((meta) => {
          const r = drafts[meta.id];
          if (!r) return null;
          const primarySel: ModelSelection = {
            providerAccountId: r.provider_account_id,
            modelId: r.model_id,
          };
          const fallbackSel: ModelSelection = {
            providerAccountId: r.fallback_account_id,
            modelId: r.fallback_model_id,
          };
          const isUnset = !r.provider_account_id || !r.model_id;
          return (
            <section key={meta.id} className={panel}>
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div>
                  <div className="flex items-center gap-2">
                    <h3 className="font-medium">{meta.label}</h3>
                    {meta.requiresVision && (
                      <span className="rounded bg-zinc-200 px-1.5 py-0.5 text-[11px] font-medium dark:bg-zinc-800">
                        Vision required
                      </span>
                    )}
                    {isUnset && (
                      <span className="rounded bg-amber-100 px-1.5 py-0.5 text-[11px] font-medium text-amber-800 dark:bg-amber-950/60 dark:text-amber-300">
                        Unset
                      </span>
                    )}
                  </div>
                  <p className="text-xs text-zinc-500">{meta.description}</p>
                </div>
                <button
                  type="button"
                  className={primaryBtn}
                  disabled={savingTask !== null}
                  onClick={() => void saveRow(meta.id)}
                >
                  {savingTask === meta.id ? "Saving..." : "Save route"}
                </button>
              </div>

              <div className="mt-3 grid gap-3 md:grid-cols-2">
                <div>
                  <div className="mb-1 text-xs font-medium text-zinc-600 dark:text-zinc-400">Primary model</div>
                  <ModelPicker
                    providers={providers}
                    value={primarySel}
                    requireVision={meta.requiresVision}
                    ariaLabel={`Primary model for ${meta.label}`}
                    onProvidersUpdated={onChanged}
                    onChange={(next) =>
                      updateDraft(meta.id, {
                        provider_account_id: next.providerAccountId,
                        model_id: next.modelId,
                      })
                    }
                  />
                </div>
                <div>
                  <div className="mb-1 text-xs font-medium text-zinc-600 dark:text-zinc-400">
                    Explicit fallback (optional)
                  </div>
                  <ModelPicker
                    providers={providers}
                    value={fallbackSel}
                    requireVision={meta.requiresVision}
                    unsetLabel="No fallback (stop on failure)"
                    ariaLabel={`Fallback model for ${meta.label}`}
                    onChange={(next) =>
                      updateDraft(meta.id, {
                        fallback_account_id: next.providerAccountId,
                        fallback_model_id: next.modelId,
                      })
                    }
                  />
                </div>
              </div>

              <div className="mt-3 flex flex-wrap gap-3 text-xs">
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Reasoning</span>
                  <select
                    className={inputCls}
                    value={r.reasoning_effort ?? ""}
                    onChange={(e) => updateDraft(meta.id, { reasoning_effort: e.target.value || null })}
                  >
                    <option value="">Default</option>
                    <option value="low">low</option>
                    <option value="medium">medium</option>
                    <option value="high">high</option>
                  </select>
                </label>
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Temp</span>
                  <input
                    type="number"
                    step="0.1"
                    min="0"
                    max="2"
                    className={`${inputCls} w-20`}
                    placeholder="0.7"
                    value={r.temperature ?? ""}
                    onChange={(e) =>
                      updateDraft(meta.id, {
                        temperature: e.target.value === "" ? null : Number(e.target.value),
                      })
                    }
                  />
                </label>
                <label className="flex items-center gap-1.5">
                  <span className="text-zinc-500">Max calls / hr</span>
                  <input
                    type="number"
                    min="1"
                    max="10000"
                    className={`${inputCls} w-24`}
                    placeholder="No cap"
                    value={r.max_calls_per_hour ?? ""}
                    onChange={(e) =>
                      updateDraft(meta.id, {
                        max_calls_per_hour: e.target.value === "" ? null : Number(e.target.value),
                      })
                    }
                  />
                </label>
              </div>
            </section>
          );
        })}
      </div>
    </div>
  );
}

function UsagePanel() {
  const [dash, setDash] = useState<UsageDashboard | null>(null);

  const load = useCallback(async () => {
    setDash(await api.llmUsageDashboard(7));
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (!dash) return <div className="text-sm text-zinc-500">Loading usage stats...</div>;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="font-medium">Last {dash.window_days} days</h2>
        <button type="button" className={btn} onClick={() => void load()}>
          Refresh
        </button>
      </div>
      <div className="grid grid-cols-3 gap-3">
        <div className={panel}>
          <div className="text-xs text-zinc-500">Total calls</div>
          <div className="mt-1 text-2xl font-semibold">{dash.total_calls}</div>
        </div>
        <div className={panel}>
          <div className="text-xs text-zinc-500">Input tokens</div>
          <div className="mt-1 text-2xl font-semibold">{dash.total_input_tokens.toLocaleString()}</div>
        </div>
        <div className={panel}>
          <div className="text-xs text-zinc-500">Output tokens</div>
          <div className="mt-1 text-2xl font-semibold">{dash.total_output_tokens.toLocaleString()}</div>
        </div>
      </div>

      <section className={panel}>
        <h3 className="font-medium">Hourly Rate Guard</h3>
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          {dash.hourly_counts.map((h) => {
            const label = TASK_METAS.find((t) => t.id === h.task_type)?.label ?? h.task_type;
            return (
              <div key={h.task_type} className="flex items-center justify-between rounded-lg border border-zinc-200 px-3 py-2 text-xs dark:border-zinc-800">
                <span>{label}</span>
                <span className="font-mono">
                  {h.calls_last_hour} / {h.max_calls_per_hour ?? "∞"}
                </span>
              </div>
            );
          })}
        </div>
      </section>

      <section className={panel}>
        <h3 className="font-medium">Breakdown by Task &amp; Model</h3>
        {dash.rows.length === 0 ? (
          <p className="mt-2 text-xs text-zinc-500">No LLM calls recorded yet.</p>
        ) : (
          <div className="mt-2 overflow-x-auto">
            <table className="w-full text-left text-xs">
              <thead className="border-b border-zinc-200 text-zinc-500 dark:border-zinc-800">
                <tr>
                  <th className="py-1.5 pr-3">Task</th>
                  <th className="py-1.5 pr-3">Model</th>
                  <th className="py-1.5 pr-3">Calls</th>
                  <th className="py-1.5 pr-3">Fallbacks</th>
                  <th className="py-1.5 pr-3">Tokens (in/out)</th>
                  <th className="py-1.5">Avg latency</th>
                </tr>
              </thead>
              <tbody>
                {dash.rows.map((row, idx) => (
                  <tr key={idx} className="border-b border-zinc-100 dark:border-zinc-900">
                    <td className="py-1.5 pr-3">{row.task_type}</td>
                    <td className="py-1.5 pr-3 font-mono">{row.model_id}</td>
                    <td className="py-1.5 pr-3">
                      {row.successful_calls}/{row.total_calls}
                    </td>
                    <td className="py-1.5 pr-3">{row.fallback_calls}</td>
                    <td className="py-1.5 pr-3">
                      {row.input_tokens}/{row.output_tokens}
                    </td>
                    <td className="py-1.5">{row.avg_latency_ms} ms</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </section>
    </div>
  );
}

function PromptsPanel() {
  const [prompts, setPrompts] = useState<PromptTemplate[]>([]);
  const [selectedTask, setSelectedTask] = useState<TaskType>("reply_drafting");
  const [sys, setSys] = useState("");
  const [usr, setUsr] = useState("");
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(async () => {
    const list = await api.llmListPrompts();
    setPrompts(list);
    const active = list.find((p) => p.task_type === selectedTask);
    if (active) {
      setSys(active.system_prompt);
      setUsr(active.user_template);
    }
  }, [selectedTask]);

  useEffect(() => {
    void load();
  }, [load]);

  const current = prompts.find((p) => p.task_type === selectedTask);

  const onSelectTask = (t: TaskType) => {
    setSelectedTask(t);
    setMsg(null);
    const p = prompts.find((x) => x.task_type === t);
    if (p) {
      setSys(p.system_prompt);
      setUsr(p.user_template);
    }
  };

  return (
    <section className={panel}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <select
            aria-label="Prompt task"
            className={inputCls}
            value={selectedTask}
            onChange={(e) => onSelectTask(e.target.value as TaskType)}
          >
            {TASK_METAS.map((t) => (
              <option key={t.id} value={t.id}>
                {t.label}
              </option>
            ))}
          </select>
          {current && (
            <span className="rounded bg-zinc-100 px-2 py-0.5 text-xs dark:bg-zinc-800">
              v{current.version} {current.is_override ? "(override)" : "(built-in)"}
            </span>
          )}
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            className={btn}
            disabled={busy || !current?.is_override}
            onClick={async () => {
              setBusy(true);
              setMsg(null);
              try {
                const reset = await api.llmResetPrompt(selectedTask);
                setSys(reset.system_prompt);
                setUsr(reset.user_template);
                await load();
                setMsg("Reset to built-in prompt.");
              } finally {
                setBusy(false);
              }
            }}
          >
            Reset to built-in
          </button>
          <button
            type="button"
            className={primaryBtn}
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              setMsg(null);
              try {
                await api.llmSavePromptOverride(selectedTask, sys, usr);
                await load();
                setMsg("Saved new prompt override version.");
              } catch (e) {
                setMsg((e as Error).message);
              } finally {
                setBusy(false);
              }
            }}
          >
            Save override
          </button>
        </div>
      </div>
      {msg && <p className="mt-2 text-xs text-zinc-600 dark:text-zinc-400">{msg}</p>}
      <div className="mt-3 space-y-3">
        <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-400">
          System prompt
          <textarea
            rows={4}
            className={`${inputCls} mt-1 w-full font-mono text-xs`}
            value={sys}
            onChange={(e) => setSys(e.target.value)}
          />
        </label>
        <label className="block text-xs font-medium text-zinc-600 dark:text-zinc-400">
          User template
          <textarea
            rows={6}
            className={`${inputCls} mt-1 w-full font-mono text-xs`}
            value={usr}
            onChange={(e) => setUsr(e.target.value)}
          />
        </label>
      </div>
    </section>
  );
}

export function SettingsPage({ rules, offline, onReplayTour, onReadRules, onDataChanged }: Props) {
  const [tab, setTab] = useState<SettingsTab>("general");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [routes, setRoutes] = useState<TaskModelRoute[]>([]);

  const loadLlm = useCallback(async () => {
    const [p, r] = await Promise.all([api.llmListProviders(), api.llmListTaskRoutes()]);
    setProviders(p);
    setRoutes(r);
  }, []);

  useEffect(() => {
    api.appInfo().then(setInfo);
    void loadLlm();
  }, [loadLlm]);

  const tabs: Array<{ id: SettingsTab; label: string }> = [
    { id: "general", label: "General & Browser" },
    { id: "providers", label: "AI Providers" },
    { id: "routing", label: "Task Routing" },
    { id: "usage", label: "Usage & Limits" },
    { id: "prompts", label: "Prompt Library" },
  ];

  return (
    <div className="mx-auto max-w-4xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-semibold">Settings</h1>
        <div role="tablist" aria-label="Settings sections" className="flex flex-wrap gap-1 rounded-xl border border-zinc-200 p-1 dark:border-zinc-800">
          {tabs.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              onClick={() => setTab(t.id)}
              className={`rounded-lg px-3 py-1.5 text-xs font-medium ${
                tab === t.id
                  ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                  : "text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
              }`}
            >
              {t.label}
            </button>
          ))}
        </div>
      </div>

      {tab === "general" && (
        <div className="space-y-4">
          <section className={panel} aria-labelledby="help-h">
            <h2 id="help-h" className="font-medium">
              Help
            </h2>
            <div className="mt-3 flex flex-wrap gap-2">
              <button type="button" className={btn} onClick={onReplayTour}>
                Replay intro tour
              </button>
              <button type="button" className={btn} onClick={onReadRules}>
                Read rules of engagement
              </button>
            </div>
            {rules && (
              <p className="mt-2 text-xs text-zinc-500">
                {rules.acknowledged
                  ? `Acknowledged notice v${rules.acknowledged_version} on ${rules.acknowledged_at}.`
                  : "Rules not acknowledged yet."}
              </p>
            )}
          </section>
          <PreferencesPanel />
          <SecurityPanel />
          <BrowserPanel rulesAcknowledged={rules?.acknowledged ?? false} />
          <LayoutPanel />
          <DiagnosticsPanel />
          <DataPanel onDataChanged={onDataChanged} />
          {info && (
            <section className={panel} aria-labelledby="about-h">
              <h2 id="about-h" className="font-medium">
                About
              </h2>
              <dl className="mt-2 grid grid-cols-[8rem_1fr] gap-y-1 text-sm">
                <dt className="text-zinc-500">Version</dt>
                <dd>{info.version}</dd>
                <dt className="text-zinc-500">Platform</dt>
                <dd>
                  {info.platform} {info.arch}
                </dd>
                <dt className="text-zinc-500">Database</dt>
                <dd className="break-all">
                  {info.db_path} (schema v{info.schema_version})
                </dd>
              </dl>
            </section>
          )}
        </div>
      )}

      {tab === "providers" && <ProvidersPanel providers={providers} offline={offline} onChanged={loadLlm} />}
      {tab === "routing" && <RoutingPanel providers={providers} routes={routes} onChanged={loadLlm} />}
      {tab === "usage" && <UsagePanel />}
      {tab === "prompts" && <PromptsPanel />}
    </div>
  );
}
