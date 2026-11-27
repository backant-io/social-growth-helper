import { useCallback, useEffect, useState } from "react";
import {
  btn,
  btnGhost,
  btnPrimary as primaryBtn,
  Disclosure,
  input as inputCls,
  LoadingBlock,
  Page,
  panel,
  PageHeader,
  Tabs,
} from "../components/ui";
import { ModelsPanel } from "./ModelsPanel";
import { api } from "../lib/api";
import { browserNotices } from "../lib/browserNotices";
import type {
  AppInfo,
  BrowserInfo,
  LayoutStatus,
  PromptTemplate,
  ProviderAccountView,
  RulesStatus,
  SecretsStatus,
  TaskModelRoute,
  TaskType,
  UsageDashboard,
} from "../lib/types";
import { TASK_METAS } from "../lib/types";
import { DataPanel } from "../learning/DataPanel";
import { PreferencesPanel } from "../onboarding/PreferencesStep";
import { Checklist } from "../phase3/Checklist";
import { PublishSettings } from "../phase3/PublishSettings";

interface Props {
  rules: RulesStatus | null;
  offline: boolean;
  onReplayTour: () => void;
  onReadRules: () => void;
  onResumeSetup?: () => void;
  onDataChanged: () => void;
}

type SettingsTab = "general" | "models" | "advanced";

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
        <h2
          id="sec-h"
          className="font-display text-lg font-semibold tracking-tight"
        >
          Security
        </h2>
        <button type="button" className={btn} onClick={check} disabled={busy}>
          Re-check
        </button>
      </div>
      {status && (
        <div className="mt-3 text-sm">
          <p
            className={
              status.available
                ? "text-emerald-600 dark:text-emerald-400"
                : "text-red-600 dark:text-red-400"
            }
          >
            {status.available
              ? "Secret storage is ready."
              : "Secret storage is not available."}{" "}
            <span className="text-zinc-500">({status.backend})</span>
          </p>
          {status.issue && (
            <p className="mt-1 text-zinc-600 dark:text-zinc-400">
              {status.issue}
            </p>
          )}
          {status.guide && (
            <div className="mt-3 space-y-3">
              <p className="font-medium">{status.guide.title}</p>
              <p className="text-zinc-600 dark:text-zinc-400">
                {status.guide.summary}
              </p>
              {status.guide.options.map((o) => (
                <details
                  key={o.name}
                  className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800"
                >
                  <summary className="cursor-pointer font-medium">
                    {o.name}
                  </summary>
                  <ol className="mt-2 list-decimal space-y-1 pl-5">
                    {o.steps.map((s) => (
                      <li key={s}>
                        <code className="text-xs">{s}</code>
                      </li>
                    ))}
                  </ol>
                </details>
              ))}
              <p className="text-zinc-600 dark:text-zinc-400">
                {status.guide.verify}
              </p>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

export function BrowserPanel({
  rulesAcknowledged,
}: {
  rulesAcknowledged: boolean;
}) {
  const [info, setInfo] = useState<BrowserInfo | null>(null);
  const [path, setPath] = useState("");
  const [message, setMessage] = useState<string | null>(null);
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
      <h2
        id="br-h"
        className="font-display text-lg font-semibold tracking-tight"
      >
        X browser window
      </h2>
      {info && (
        <div className="mt-3 space-y-3 text-sm">
          <p>
            {info.install ? (
              <>
                Using <strong>{info.install.kind}</strong> at{" "}
                <code className="text-xs">{info.install.path}</code>
              </>
            ) : (
              <span className="text-red-600 dark:text-red-400">
                {info.problem}
              </span>
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
          <div className="flex flex-wrap gap-2">
            <button
              type="button"
              className={btn}
              disabled={busy || info.running || !rulesAcknowledged}
              onClick={() => run(() => api.launchX())}
            >
              Open X window
            </button>
            <button
              type="button"
              className={btn}
              disabled={busy || !info.running}
              onClick={() => run(() => api.closeX())}
            >
              Close X window
            </button>
          </div>
          {!rulesAcknowledged && (
            <p className="text-zinc-500">
              Acknowledge the rules to open the X window.
            </p>
          )}
          {message && (
            <p role="alert" className="text-red-600 dark:text-red-400">
              {message}
            </p>
          )}
          <Disclosure summary="Use a different browser">
            <div className="flex gap-2">
              <input
                aria-label="Browser path override"
                className={`${inputCls} flex-1`}
                placeholder="Optional: path to a Chrome, Chromium or Brave binary"
                value={path}
                onChange={(e) => setPath(e.target.value)}
              />
              <button
                type="button"
                className={btn}
                disabled={busy}
                onClick={() =>
                  run(() => api.setBrowserPath(path.trim() || null))
                }
              >
                Save path
              </button>
            </div>
          </Disclosure>
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
      <h2
        id="lay-h"
        className="font-display text-lg font-semibold tracking-tight"
      >
        Window layout
      </h2>

      {status?.hint && (
        <p
          role="status"
          className="mt-2 text-sm text-amber-700 dark:text-amber-300"
        >
          {status.hint}
        </p>
      )}
      <div className="mt-3 flex flex-wrap gap-2">
        <button
          type="button"
          className={btn}
          disabled={busy || off}
          onClick={() => run(api.saveWindowLayout, "Layout saved.")}
        >
          Save current layout
        </button>
        <button
          type="button"
          className={btn}
          disabled={busy || off}
          onClick={() => run(api.applyWindowLayout, "Layout applied.")}
        >
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
      <h2
        id="diag-h"
        className="font-display text-lg font-semibold tracking-tight"
      >
        Diagnostics
      </h2>
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

function UsagePanel() {
  const [dash, setDash] = useState<UsageDashboard | null>(null);

  const load = useCallback(async () => {
    setDash(await api.llmUsageDashboard(7));
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  if (!dash) return <LoadingBlock label="Loading usage stats" />;

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h2 className="font-display text-lg font-semibold tracking-tight">
          Last {dash.window_days} days
        </h2>
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
          <div className="mt-1 text-2xl font-semibold">
            {dash.total_input_tokens.toLocaleString()}
          </div>
        </div>
        <div className={panel}>
          <div className="text-xs text-zinc-500">Output tokens</div>
          <div className="mt-1 text-2xl font-semibold">
            {dash.total_output_tokens.toLocaleString()}
          </div>
        </div>
      </div>

      <section className={panel}>
        <h3 className="font-medium">Hourly Rate Guard</h3>
        <div className="mt-2 grid gap-2 sm:grid-cols-2">
          {dash.hourly_counts.map((h) => {
            const label =
              TASK_METAS.find((t) => t.id === h.task_type)?.label ??
              h.task_type;
            return (
              <div
                key={h.task_type}
                className="flex items-center justify-between rounded-lg border border-zinc-200 px-3 py-2 text-xs dark:border-zinc-800"
              >
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
          <p className="mt-2 text-xs text-zinc-500">
            No LLM calls recorded yet.
          </p>
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
                  <tr
                    key={idx}
                    className="border-b border-zinc-100 dark:border-zinc-900"
                  >
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
              v{current.version}{" "}
              {current.is_override ? "(override)" : "(built-in)"}
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
      {msg && (
        <p className="mt-2 text-xs text-zinc-600 dark:text-zinc-400">{msg}</p>
      )}
      {["post_scoring", "reply_drafting", "ogc_generation"].includes(
        selectedTask,
      ) && (
        <p className="mt-2 text-xs text-zinc-500">
          An override applies to Scout scoring, Reply Studio drafts, Creator and
          the Style bench. In Scout, Reply Studio and Creator the safety rules
          and output format are always added, and the post input is appended if
          the template leaves out its placeholder.
        </p>
      )}
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

export function SettingsPage({
  rules,
  offline,
  onReplayTour,
  onReadRules,
  onResumeSetup,
  onDataChanged,
}: Props) {
  const [tab, setTab] = useState<SettingsTab>("general");
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [providers, setProviders] = useState<ProviderAccountView[]>([]);
  const [routes, setRoutes] = useState<TaskModelRoute[]>([]);

  const loadLlm = useCallback(async () => {
    const [p, r] = await Promise.all([
      api.llmListProviders(),
      api.llmListTaskRoutes(),
    ]);
    setProviders(p);
    setRoutes(r);
  }, []);

  useEffect(() => {
    api.appInfo().then(setInfo);
    void loadLlm();
  }, [loadLlm]);

  const tabs: Array<{ id: SettingsTab; label: string }> = [
    { id: "general", label: "General" },
    { id: "models", label: "AI models" },
    { id: "advanced", label: "Advanced" },
  ];

  return (
    <Page>
      <PageHeader
        title="Settings"
        actions={
          <>
            {onResumeSetup && (
              <button
                type="button"
                className={primaryBtn}
                onClick={onResumeSetup}
              >
                Finish setup
              </button>
            )}
            <button type="button" className={btnGhost} onClick={onReplayTour}>
              Replay tour
            </button>
            <button type="button" className={btnGhost} onClick={onReadRules}>
              Rules
            </button>
          </>
        }
      />
      <Tabs
        label="Settings sections"
        tabs={tabs}
        active={tab}
        onChange={setTab}
      />

      {tab === "general" && (
        <div className="space-y-8">
          <PreferencesPanel />
          <BrowserPanel rulesAcknowledged={rules?.acknowledged ?? false} />
        </div>
      )}

      {tab === "models" && (
        <ModelsPanel
          providers={providers}
          routes={routes}
          offline={offline}
          onChanged={loadLlm}
        />
      )}

      {tab === "advanced" && (
        <div className="space-y-8">
          <UsagePanel />
          <PublishSettings />
          <PromptsPanel />
          <LayoutPanel />
          <SecurityPanel />
          <DiagnosticsPanel />
          <DataPanel onDataChanged={onDataChanged} />
          <Disclosure summary="Live action check">
            <Checklist />
          </Disclosure>
          {info && (
            <p className="font-mono text-xs text-zinc-500">
              v{info.version} / {info.platform} {info.arch} / schema{" "}
              {info.schema_version}
            </p>
          )}
        </div>
      )}
    </Page>
  );
}
