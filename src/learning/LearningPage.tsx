import { useCallback, useEffect, useState } from "react";
import { DiffView } from "./DiffView";
import { InsightCard } from "./InsightCard";
import { learningApi } from "./api";
import { categoryLabel, diffSummary, multiplier, sampleNote, targetLabel } from "./model";
import type { ChangelogEntry, ExampleInfo, Insight, LearningOverview, Patch, ReflectionMode, VersionInfo } from "./types";
import { EmptyBlock, LoadingBlock, Notice, PageHeader, Tabs } from "../components/ui";
import { btn, btnPrimary, input, panel } from "./ui";

type Tab = "suggestions" | "signals" | "history";

interface Props {
  onProposedChange?: (count: number) => void;
}

function fmtDate(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export function LearningPage({ onProposedChange }: Props) {
  const [tab, setTab] = useState<Tab>("suggestions");
  const [data, setData] = useState<LearningOverview | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ kind: "ok" | "error"; text: string } | null>(null);
  const [historyKey, setHistoryKey] = useState(0);

  const load = useCallback(async () => {
    const d = await learningApi.overview();
    setData(d);
    onProposedChange?.(d.proposed.length);
  }, [onProposedChange]);

  useEffect(() => {
    load().catch((e: Error) => setMessage({ kind: "error", text: e.message }));
  }, [load]);

  const run = async (fn: () => Promise<string | void>) => {
    setBusy(true);
    setMessage(null);
    try {
      const ok = await fn();
      if (ok) setMessage({ kind: "ok", text: ok });
    } catch (e) {
      setMessage({ kind: "error", text: (e as Error).message });
    } finally {
      await load().catch(() => undefined);
      setHistoryKey((k) => k + 1);
      setBusy(false);
    }
  };

  const reflect = (mode: ReflectionMode) =>
    run(async () => {
      const r = await learningApi.reflect(mode);
      return r.insights_created > 0 ? `${r.insights_created} new suggestion(s). ${r.note}` : r.note;
    });

  const accept = (i: Insight, edited: Patch | null) =>
    run(async () => {
      await learningApi.accept(i.id, edited);
      return "Applied. You can undo it from History.";
    });

  const reject = (i: Insight) =>
    run(async () => {
      await learningApi.reject(i.id);
      return "Rejected. This suggestion will not come back.";
    });

  if (!data) {
    return (
      <div className="mx-auto max-w-4xl space-y-8">
        <PageHeader title="Learning" description="What the app noticed about your edits and results, and small changes it suggests." />
        {message ? <Notice tone="error">{message.text}</Notice> : <LoadingBlock label="Loading suggestions" />}
      </div>
    );
  }

  const { signals } = data;
  const note = sampleNote(signals.published_items, signals.min_sample);
  const tabs: Array<[Tab, string]> = [
    ["suggestions", `Suggestions${data.proposed.length ? ` (${data.proposed.length})` : ""}`],
    ["signals", "What I noticed"],
    ["history", "History"],
  ];

  return (
    <div className="mx-auto max-w-4xl space-y-8">
      <PageHeader
        title="Learning"
        description="The app compares your edits and results with what it drafted, then suggests small changes. Nothing changes until you apply it, and every change can be undone."
        actions={
          <>
            <button type="button" className={btn} disabled={busy} onClick={() => reflect("light")} title="Looks at your edits and dismissals">
              Quick reflect
            </button>
            <button type="button" className={btnPrimary} disabled={busy} onClick={() => reflect("deep")} title="Adds results, timing and winners">
              Deep reflect
            </button>
          </>
        }
      />

      {note && <Notice tone="warn">{note}</Notice>}
      {message && <Notice tone={message.kind === "error" ? "error" : "ok"}>{message.text}</Notice>}

      <Tabs label="Learning sections" tabs={tabs.map(([id, label]) => ({ id, label }))} active={tab} onChange={setTab} />

      {tab === "suggestions" && (
        <div className="space-y-3" role="tabpanel" aria-label="Suggestions">
          {data.proposed.length === 0 ? (
            <EmptyBlock title="No suggestions waiting">
              {signals.last_run ? `Last checked ${fmtDate(signals.last_run.started_at)}. ` : ""}
              Keep editing and posting. The app looks again when you open it, or press Reflect.
            </EmptyBlock>
          ) : (
            data.proposed.map((i) => <InsightCard key={i.id} insight={i} busy={busy} onAccept={accept} onReject={reject} />)
          )}
          {data.decided.length > 0 && (
            <details className={panel}>
              <summary className="cursor-pointer text-sm font-medium">Earlier decisions ({data.decided.length})</summary>
              <ul className="mt-2 space-y-2 text-sm">
                {data.decided.map((i) => (
                  <li key={i.id} className="flex flex-wrap items-baseline justify-between gap-2">
                    <span>{i.hypothesis}</span>
                    <span className="text-xs text-zinc-500">
                      {categoryLabel(i.category)}, {i.status.replace("_", " ")}
                      {i.edited ? " (edited)" : ""} {i.decided_at ? fmtDate(i.decided_at) : ""}
                    </span>
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}

      {tab === "signals" && <SignalsTab data={data} onChanged={() => void run(async () => undefined)} />}
      {tab === "history" && <HistoryTab key={historyKey} data={data} busy={busy} run={run} />}
    </div>
  );
}


function Stat({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
      <div className="text-lg font-semibold">{value}</div>
      <div className="text-xs text-zinc-500">{label}</div>
      {hint && <div className="mt-1 text-xs text-zinc-400">{hint}</div>}
    </div>
  );
}

function SignalsTab({ data, onChanged }: { data: LearningOverview; onChanged: () => void }) {
  const s = data.signals;
  const editRate = s.edit.drafts ? Math.round((s.edit.edited_drafts / s.edit.drafts) * 100) : 0;
  return (
    <div className="space-y-4" role="tabpanel" aria-label="What I noticed">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="Published replies" value={String(s.published_items)} hint={`${s.mature_items} old enough to score`} />
        <Stat label="Edited before posting" value={`${editRate}%`} hint={`${s.edit.edited_drafts} of ${s.edit.drafts}`} />
        <Stat label="Typical length change" value={`${s.edit.avg_length_change_pct > 0 ? "+" : ""}${Math.round(s.edit.avg_length_change_pct)}%`} />
        <Stat label="Your typical reach" value={s.baseline_impressions ? `${Math.round(s.baseline_impressions)}` : "n/a"} hint="impressions per reply" />
      </div>

      {s.edit.removed.length > 0 && (
        <section className={panel}>
          <h2 className="font-display text-lg font-semibold tracking-tight">Phrases you keep deleting</h2>
          <ul className="mt-2 flex flex-wrap gap-2 text-xs">
            {s.edit.removed.map((p) => (
              <li key={p.phrase} className="rounded-full bg-red-50 px-2 py-0.5 text-red-800 dark:bg-red-950 dark:text-red-300">
                {p.phrase} <span className="opacity-70">x{p.count}</span>
              </li>
            ))}
          </ul>
          {s.edit.added.length > 0 && (
            <>
              <h3 className="mt-3 text-sm font-medium">Phrases you add</h3>
              <ul className="mt-2 flex flex-wrap gap-2 text-xs">
                {s.edit.added.map((p) => (
                  <li key={p.phrase} className="rounded-full bg-emerald-50 px-2 py-0.5 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300">
                    {p.phrase} <span className="opacity-70">x{p.count}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </section>
      )}

      {s.timing.age.length > 0 && (
        <section className={panel}>
          <h2 className="font-display text-lg font-semibold tracking-tight">Results by how fast you replied</h2>
          <p className="text-xs text-zinc-500">1.0x is your typical result, adjusted for the author's reach.</p>
          <Bars rows={s.timing.age} />
          {s.timing.tier.length > 0 && (
            <>
              <h3 className="mt-3 text-sm font-medium">Results by author size</h3>
              <Bars rows={s.timing.tier} />
            </>
          )}
        </section>
      )}

      {s.models.length > 0 && (
        <section className={panel}>
          <h2 className="font-display text-lg font-semibold tracking-tight">Which model drafts fit you best</h2>
          <table className="mt-2 w-full text-sm">
            <thead className="text-left text-xs text-zinc-500">
              <tr><th className="font-normal">Model</th><th className="font-normal">Drafts</th><th className="font-normal">Used with light edits</th><th className="font-normal">Results</th></tr>
            </thead>
            <tbody>
              {s.models.map((m) => (
                <tr key={m.model}>
                  <td>{m.model}</td><td>{m.drafts}</td><td>{Math.round(m.keep_rate * 100)}%</td><td>{multiplier(m.avg_normalized)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {s.dismissal.dismissed > 0 && (
        <section className={panel}>
          <h2 className="font-display text-lg font-semibold tracking-tight">What you skip</h2>
          <p className="mt-1 text-sm text-zinc-600 dark:text-zinc-400">
            You dismissed {s.dismissal.dismissed} posts and replied to {s.dismissal.drafted}.
            {s.dismissal.reasons.length > 0 && ` Most common reason: ${s.dismissal.reasons[0][0]} (${s.dismissal.reasons[0][1]}).`}
          </p>
        </section>
      )}

      {(s.best.length > 0 || s.weakest.length > 0) && (
        <section className={panel}>
          <h2 className="font-display text-lg font-semibold tracking-tight">Best and weakest replies</h2>
          <ul className="mt-2 space-y-2 text-sm">
            {[...s.best.map((b) => ({ ...b, tone: "best" })), ...s.weakest.map((b) => ({ ...b, tone: "weak" }))].map((b) => (
              <li key={`${b.tone}-${b.draft_id}`} className="flex justify-between gap-3">
                <span className="min-w-0 break-words">{b.text}</span>
                <span className="shrink-0 text-xs text-zinc-500">{multiplier(b.normalized)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <MinSample value={s.min_sample} onSaved={onChanged} />
    </div>
  );
}

function Bars({ rows }: { rows: Array<{ label: string; n: number; mean_normalized: number }> }) {
  const max = Math.max(...rows.map((r) => r.mean_normalized), 1);
  return (
    <ul className="mt-2 space-y-1">
      {rows.map((r) => (
        <li key={r.label} className="grid grid-cols-[9rem_1fr_6rem] items-center gap-2 text-xs">
          <span>{r.label}</span>
          <span className="h-2 rounded-full bg-zinc-100 dark:bg-zinc-800">
            <span className="block h-2 rounded-full bg-zinc-700 dark:bg-zinc-300" style={{ width: `${Math.max(4, (r.mean_normalized / max) * 100)}%` }} />
          </span>
          <span className="text-zinc-500">{multiplier(r.mean_normalized)} ({r.n})</span>
        </li>
      ))}
    </ul>
  );
}

function MinSample({ value, onSaved }: { value: number; onSaved: () => void }) {
  const [text, setText] = useState(String(value));
  const [msg, setMsg] = useState<string | null>(null);
  const save = async () => {
    try {
      const n = await learningApi.setMinSample(Number(text));
      setText(String(n));
      setMsg("Saved.");
      onSaved();
    } catch (e) {
      setMsg((e as Error).message);
    }
  };
  return (
    <section className={panel}>
      <h2 className="font-display text-lg font-semibold tracking-tight">Minimum sample</h2>
      <p className="mt-1 text-xs text-zinc-500">Below this many published replies, suggestions are marked low confidence.</p>
      <div className="mt-2 flex items-center gap-2">
        <input aria-label="Minimum published replies" inputMode="numeric" className={`${input} w-24`} value={text} onChange={(e) => setText(e.target.value)} />
        <button type="button" className={btn} onClick={save}>Save</button>
        {msg && <span className="text-xs text-zinc-500">{msg}</span>}
      </div>
    </section>
  );
}

function HistoryTab({ data, busy, run }: { data: LearningOverview; busy: boolean; run: (fn: () => Promise<string | void>) => Promise<void> }) {
  const [rubric, setRubric] = useState<VersionInfo[]>([]);
  const [changelog, setChangelog] = useState<ChangelogEntry[]>([]);
  const [examples, setExamples] = useState<ExampleInfo[]>([]);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    Promise.all([learningApi.rubricVersions(), learningApi.changelog(), learningApi.examples()])
      .then(([versions, log, ex]) => {
        setRubric(versions);
        setChangelog(log);
        setExamples(ex);
      })
      .catch((e: Error) => setLoadError(e.message));
  }, [data]);

  const activeV = rubric.find((v) => v.is_active)?.version ?? 0;
  return (
    <div className="space-y-4" role="tabpanel" aria-label="History">
      {loadError && <p role="alert" className="text-sm text-red-600">{loadError}</p>}

      <section className={panel} aria-label="Scout rubric versions">
        <h2 className="font-display text-lg font-semibold tracking-tight">Scout rubric</h2>
        {rubric.length === 0 ? (
          <p className="mt-1 text-sm text-zinc-500">No versions yet. The first change you apply creates one.</p>
        ) : (
          <ul className="mt-2 space-y-2">
            {rubric.map((v) => {
              const sum = diffSummary(v.diff);
              return (
                <li key={v.version} className="rounded-lg border border-zinc-200 p-2 dark:border-zinc-800">
                  <div className="flex items-center justify-between gap-2 text-sm">
                    <span>
                      Version {v.version} <span className="text-xs text-zinc-500">{fmtDate(v.created_at)}</span>
                      {v.is_active && <span className="ml-2 rounded-full bg-emerald-100 px-2 py-0.5 text-xs text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300">Active</span>}
                    </span>
                    {!v.is_active && v.version < activeV && (
                      <button
                        type="button"
                        className={btn}
                        disabled={busy}
                        onClick={() => {
                          if (window.confirm(`Go back to scout rubric version ${v.version}? Newer versions stay in history.`)) {
                            void run(async () => {
                              await learningApi.rollbackRubric(v.version);
                              return `Rolled back to version ${v.version}.`;
                            });
                          }
                        }}
                      >
                        Roll back to this
                      </button>
                    )}
                  </div>
                  {v.diff.length > 0 && (
                    <details className="mt-1">
                      <summary className="cursor-pointer text-xs text-zinc-500">What changed (+{sum.added} / -{sum.removed})</summary>
                      <div className="mt-1"><DiffView diff={v.diff} label={`Scout rubric version ${v.version} changes`} /></div>
                    </details>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        <p className="mt-3 text-xs text-zinc-500">Style guide versions, including rollback, are on the Style page. Learning changes to style are listed in the change log below.</p>
      </section>

      <section className={panel} aria-label="Examples">
        <h2 className="font-display text-lg font-semibold tracking-tight">Kept examples</h2>
        <p className="mt-1 text-xs text-zinc-500">Replies you chose to keep. They guide the voice of future drafts.</p>
        {examples.length === 0 ? (
          <p className="mt-2 text-sm text-zinc-500">Nothing here yet.</p>
        ) : (
          <ul className="mt-2 space-y-2">
            {examples.map((e) => (
              <li key={e.id} className="flex items-start justify-between gap-3 rounded-lg border border-zinc-200 p-2 text-sm dark:border-zinc-800">
                <div className="min-w-0">
                  <p className="break-words">{e.example_text}</p>
                  {e.why_it_works && <p className="mt-0.5 text-xs text-zinc-500">{e.why_it_works}</p>}
                </div>
                <button type="button" className={btn} disabled={busy} onClick={() => void run(async () => { await learningApi.removeExample(e.id); return "Example removed."; })}>
                  Remove
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className={panel} aria-label="Change log">
        <h2 className="font-display text-lg font-semibold tracking-tight">Change log</h2>
        {changelog.length === 0 ? (
          <p className="mt-2 text-sm text-zinc-500">No changes yet.</p>
        ) : (
          <ul className="mt-2 space-y-1 text-sm">
            {changelog.map((c) => (
              <li key={c.id} className="flex justify-between gap-2">
                <span>
                  {c.action === "accepted" ? "Applied" : "Rolled back"} {targetLabel(c.kind).toLowerCase()}
                  {c.from_version !== null && c.to_version !== null ? ` (v${c.from_version} to v${c.to_version})` : ""}
                </span>
                <span className="text-xs text-zinc-500">{fmtDate(c.created_at)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
