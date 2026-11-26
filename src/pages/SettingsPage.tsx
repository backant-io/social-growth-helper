import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import type { AppInfo, BrowserInfo, RulesStatus, SecretsStatus, VisibleTweet } from "../lib/types";

interface Props {
  rules: RulesStatus | null;
  onReplayTour: () => void;
  onReadRules: () => void;
}

const panel = "rounded-xl border border-zinc-200 p-4 dark:border-zinc-800";
const btn = "rounded-lg border border-zinc-300 px-3 py-1.5 text-sm hover:bg-zinc-100 disabled:opacity-40 dark:border-zinc-700 dark:hover:bg-zinc-800";

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
        <h2 id="sec-h" className="font-medium">Security</h2>
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
                      <li key={s}><code className="text-xs">{s}</code></li>
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
      <h2 id="br-h" className="font-medium">X browser window</h2>
      <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
        The app opens your real Chrome in its own window with a separate profile. Log in to X there once; the login is kept for next time. X is never loaded inside this app.
      </p>
      {info && (
        <div className="mt-3 space-y-3 text-sm">
          <p>
            {info.install ? (
              <>Using <strong>{info.install.kind}</strong> at <code className="text-xs">{info.install.path}</code></>
            ) : (
              <span className="text-red-600 dark:text-red-400">{info.problem}</span>
            )}
          </p>
          <div className="flex gap-2">
            <input
              aria-label="Browser path override"
              className="min-w-0 flex-1 rounded-lg border border-zinc-300 bg-white px-3 py-1.5 text-sm dark:border-zinc-700 dark:bg-zinc-900"
              placeholder="Optional: path to a Chrome, Chromium or Brave binary"
              value={path}
              onChange={(e) => setPath(e.target.value)}
            />
            <button type="button" className={btn} disabled={busy} onClick={() => run(() => api.setBrowserPath(path.trim() || null))}>
              Save path
            </button>
          </div>
          <div className="flex flex-wrap gap-2">
            <button type="button" className={btn} disabled={busy || info.running || !rulesAcknowledged} onClick={() => run(() => api.launchX())}>
              Open X window
            </button>
            <button type="button" className={btn} disabled={busy || !info.running} onClick={() => run(() => api.closeX())}>
              Close X window
            </button>
            <button type="button" className={btn} disabled={busy || !info.running} onClick={() => run(async () => setTweets(await api.readVisibleTweets()))}>
              Read visible tweets (dev)
            </button>
          </div>
          {!rulesAcknowledged && <p className="text-amber-600 dark:text-amber-400">Acknowledge the rules of engagement to open the X window.</p>}
          {message && <p role="alert" className="text-red-600 dark:text-red-400">{message}</p>}
          {tweets && (
            <div>
              <p className="mb-1 text-zinc-500">{tweets.length} visible tweet(s). Read-only.</p>
              <ul className="max-h-48 space-y-1 overflow-y-auto">
                {tweets.map((t, i) => (
                  <li key={t.id ?? i} className="rounded border border-zinc-200 p-2 text-xs dark:border-zinc-800">
                    <strong>{t.author}</strong> {t.handle && <span className="text-zinc-500">@{t.handle}</span>} {t.promoted && <em>(promoted)</em>}
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

export function SettingsPage({ rules, onReplayTour, onReadRules }: Props) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    api.appInfo().then(setInfo);
  }, []);

  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <h1 className="text-xl font-semibold">Settings</h1>
      <section className={panel} aria-labelledby="help-h">
        <h2 id="help-h" className="font-medium">Help</h2>
        <div className="mt-3 flex flex-wrap gap-2">
          <button type="button" className={btn} onClick={onReplayTour}>Replay intro tour</button>
          <button type="button" className={btn} onClick={onReadRules}>Read rules of engagement</button>
        </div>
        {rules && (
          <p className="mt-2 text-xs text-zinc-500">
            {rules.acknowledged ? `Acknowledged notice v${rules.acknowledged_version} on ${rules.acknowledged_at}.` : "Rules not acknowledged yet."}
          </p>
        )}
      </section>
      <SecurityPanel />
      <BrowserPanel rulesAcknowledged={rules?.acknowledged ?? false} />
      {info && (
        <section className={panel} aria-labelledby="about-h">
          <h2 id="about-h" className="font-medium">About</h2>
          <dl className="mt-2 grid grid-cols-[8rem_1fr] gap-y-1 text-sm">
            <dt className="text-zinc-500">Version</dt><dd>{info.version}</dd>
            <dt className="text-zinc-500">Platform</dt><dd>{info.platform} {info.arch}</dd>
            <dt className="text-zinc-500">Database</dt><dd className="break-all">{info.db_path} (schema v{info.schema_version})</dd>
          </dl>
        </section>
      )}
    </div>
  );
}
