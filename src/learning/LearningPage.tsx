import { useCallback, useEffect, useState } from "react";
import { DiffView } from "./DiffView";
import { InsightCard } from "./InsightCard";
import { learningApi } from "./api";
import { diffSummary, targetLabel } from "./model";
import type { ChangelogEntry, ExampleInfo, Insight, LearningOverview, Patch, ReflectionMode, VersionInfo } from "./types";
import { EmptyBlock, LoadingBlock, Notice, Page, PageHeader, Tabs } from "../components/ui";
import { btn, btnPrimary, panel } from "./ui";

type Tab = "suggestions" | "history";

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
      <Page>
        <PageHeader title="Learning" />
        {message ? <Notice tone="error">{message.text}</Notice> : <LoadingBlock label="Loading suggestions" />}
      </Page>
    );
  }

  const { signals } = data;
  const tabs: Array<[Tab, string]> = [
    ["suggestions", `Suggestions${data.proposed.length ? ` (${data.proposed.length})` : ""}`],
    ["history", "History"],
  ];

  return (
    <Page>
      <PageHeader
        title="Learning"
        description="Small changes suggested from your edits and results."
        actions={
          <button type="button" className={btnPrimary} disabled={busy} onClick={() => reflect("deep")}>
            Reflect
          </button>
        }
      />

      {message && <Notice tone={message.kind === "error" ? "error" : "ok"}>{message.text}</Notice>}

      <Tabs label="Learning sections" tabs={tabs.map(([id, label]) => ({ id, label }))} active={tab} onChange={setTab} />

      {tab === "suggestions" && (
        <div className="space-y-3" role="tabpanel" aria-label="Suggestions">
          {data.proposed.length === 0 ? (
            <EmptyBlock title="Nothing to review">
              {signals.last_run ? `Last checked ${fmtDate(signals.last_run.started_at)}.` : "Press Reflect to look for suggestions."}
            </EmptyBlock>
          ) : (
            data.proposed.map((i) => <InsightCard key={i.id} insight={i} busy={busy} onAccept={accept} onReject={reject} />)
          )}
        </div>
      )}

      {tab === "history" && <HistoryTab key={historyKey} data={data} busy={busy} run={run} />}
    </Page>
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
