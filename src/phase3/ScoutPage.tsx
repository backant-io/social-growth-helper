import { useCallback, useEffect, useRef, useState } from "react";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { DURATION_PRESETS, POST_PRESETS, REASONS, compactCount, formatDuration, scoreTone } from "./logic";
import type { QueueItem, ScoutProgress, SessionParams, SessionView } from "./types";
import { EmptyBlock, LoadingBlock, PageHeader, Segmented, useKillSwitch } from "../components/ui";
import { btn, btnGhost, btnPrimary, chip, input, muted, Notice } from "./ui";

interface Props {
  onOpenDraft: (draftId: string) => void;
  onGoToSettings?: () => void;
}

const TONE = { high: "bg-zinc-950 dark:bg-zinc-50", mid: "bg-zinc-500", low: "bg-zinc-300 dark:bg-zinc-700" } as const;

function Progress({ p, params }: { p: ScoutProgress | null; params: SessionParams }) {
  const pct = p ? Math.min(100, Math.round((p.scanned / params.max_posts) * 100)) : 0;
  return (
    <div aria-live="polite">
      <div className="h-0.5 overflow-hidden bg-zinc-200 dark:bg-zinc-800">
        <div className="h-full bg-zinc-950 transition-all dark:bg-zinc-50" style={{ width: `${pct}%` }} />
      </div>
      <p className={`mt-2 font-mono text-xs ${muted}`}>
        {p ? `${p.scanned}/${params.max_posts} read / ${p.queued} queued / ${formatDuration(p.elapsed_sec)} of ${formatDuration(params.max_duration_sec)}` : "Starting..."}
      </p>
    </div>
  );
}

function SessionRow({ s }: { s: SessionView }) {
  const sum = s.summary;
  return (
    <li className="flex flex-wrap items-baseline justify-between gap-2 py-2 text-sm">
      <span>{new Date(s.started_at).toLocaleString()}</span>
      <span className={`font-mono text-xs ${muted}`}>
        {s.posts_scanned} read / {s.posts_queued} queued
        {sum ? ` / ${sum.heuristic_scored > 0 ? "local score" : sum.model}` : ""}
        {s.ended_reason ? ` / ${s.ended_reason.replace(/_/g, " ")}` : ""}
      </span>
    </li>
  );
}

export function ScoutPage({ onOpenDraft, onGoToSettings }: Props) {
  const [params, setParams] = useState<SessionParams>({ feed_source: "for_you", max_duration_sec: 600, max_posts: 100, stop_after_queued: null, threshold: 72 });
  const killed = useKillSwitch();
  const [running, setRunning] = useState(false);
  const [paused, setPaused] = useState<null | "manual" | "user_activity">(null);
  const [progress, setProgress] = useState<ScoutProgress | null>(null);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [recent, setRecent] = useState<SessionView[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [dismissing, setDismissing] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const budgetLoaded = useRef(false);
  const [loaded, setLoaded] = useState(false);
  const [filter, setFilter] = useState("");
  const [more, setMore] = useState(false);

  const reload = useCallback(async () => {
    const [status, q] = await Promise.all([p3.scoutStatus(), p3.scoutQueue()]);
    setRunning(status.running);
    setRecent(status.recent);
    const remembered = budgetLoaded.current ? null : status.last_params;
    budgetLoaded.current = true;
    setParams((p) => ({ ...p, ...(remembered ?? {}), threshold: status.threshold }));
    setQueue(q);
    setLoaded(true);
    setSelected((i) => Math.min(i, Math.max(0, q.length - 1)));
  }, []);

  useEffect(() => {
    void reload().catch((e) => setError(String(e.message ?? e)));
    const offs = [
      listen<ScoutProgress>("scout:progress", (p) => {
        setProgress(p);
        setPaused(null);
        void p3.scoutQueue().then(setQueue);
      }),
      listen("scout:finished", () => {
        setRunning(false);
        setPaused(null);
        void reload();
      }),
      listen<{ message: string }>("scout:error", (e) => setError(e.message)),
      listen<{ reason?: string }>("scout:paused", (e) => setPaused(e.reason === "manual" ? "manual" : "user_activity")),
      listen("scout:resumed", () => setPaused(null)),
    ];
    return () => {
      offs.forEach((o) => void o.then((f) => f()));
    };
  }, [reload]);

  const start = async () => {
    setError(null);
    setBusy(true);
    try {
      await p3.scoutStart(params);
      setProgress(null);
      setRunning(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    await p3.scoutStop();
  };

  const togglePause = async () => {
    const wantPause = paused !== "manual";
    if (await p3.scoutPause(wantPause)) setPaused(wantPause ? "manual" : null);
  };

  const draft = useCallback(
    async (item: QueueItem) => {
      setError(null);
      try {
        const d = await p3.draftOpen(item.evaluation_id);
        onOpenDraft(d.id);
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      }
    },
    [onOpenDraft],
  );

  const dismiss = useCallback(
    async (item: QueueItem, reason: string) => {
      await p3.scoutDismiss(item.evaluation_id, reason);
      setDismissing(null);
      setQueue((q) => q.filter((x) => x.evaluation_id !== item.evaluation_id));
    },
    [],
  );

  useShortcuts((a) => {
    if (a === "next") setSelected((i) => Math.min(queue.length - 1, i + 1));
    else if (a === "prev") setSelected((i) => Math.max(0, i - 1));
    else if (a === "dismiss" && queue[selected]) void dismiss(queue[selected], "other");
    else if (a === "open" && queue[selected]) void draft(queue[selected]);
    else return false;
  });

  const visible = queue.filter((i) => {
    const q = filter.trim().toLowerCase();
    return !q || i.post.handle.toLowerCase().includes(q) || i.post.text.toLowerCase().includes(q);
  });
  const commitThreshold = () => void p3.setThreshold(params.threshold).then(() => p3.scoutQueue().then(setQueue));

  const numField = (label: string, value: number | string, set: (n: number) => void, min: number, max: number, placeholder?: string) => (
    <label className="flex items-center gap-2 text-sm">
      <span className={muted}>{label}</span>
      <input type="number" min={min} max={max} placeholder={placeholder} value={value} onChange={(e) => set(Number(e.target.value))} className={`${input} w-24`} />
    </label>
  );

  const actions = running ? (
    <>
      <button type="button" className={btn} onClick={togglePause}>
        {paused === "manual" ? "Resume" : "Pause"}
      </button>
      <button type="button" className={btn} onClick={stop}>
        Stop
      </button>
    </>
  ) : (
    <button type="button" data-testid="scout-start" className={btnPrimary} onClick={start} disabled={busy || killed} title={killed ? "Paused. Resume in the bar above." : undefined}>
      Start scouting
    </button>
  );

  return (
    <div className="mx-auto max-w-4xl space-y-8">
      <PageHeader title="Scout" description="Read your feed for a set time and keep the posts worth answering. Nothing is posted from here." actions={actions} />

      <InlineModelGuard
        taskType="post_scoring"
        onAssigned={() => undefined}
        onGoToSettings={onGoToSettings}
        hint="Until a model is chosen, posts get a quick local score from engagement and your topics."
      />
      {killed && <Notice tone="warn">Everything is paused, so Scout cannot start. Resume in the bar above.</Notice>}
      {error && <Notice tone="error">{error}</Notice>}
      {paused === "user_activity" && <Notice tone="warn">Paused while you use the X window. Scouting resumes a few seconds after you stop.</Notice>}
      {paused === "manual" && <Notice tone="warn">Scout is paused. The session is kept; resume it when you are ready.</Notice>}

      <section aria-labelledby="scout-start" className="space-y-4">
        <div className="flex items-baseline justify-between">
          <h2 id="scout-start" className="eyebrow">
            Session
          </h2>
          <span className="font-mono text-xs text-zinc-500">
            {formatDuration(params.max_duration_sec)} / up to {params.max_posts} posts
          </span>
        </div>
        <fieldset disabled={running} className="flex flex-wrap items-center gap-x-6 gap-y-3 disabled:opacity-60">
          <Segmented
            label="Duration"
            value={params.max_duration_sec / 60}
            options={DURATION_PRESETS.map((m) => ({ value: m, label: `${m} min` }))}
            onChange={(m) => setParams({ ...params, max_duration_sec: m * 60 })}
          />
          <Segmented label="Posts to read" value={params.max_posts} options={POST_PRESETS.map((n) => ({ value: n, label: `${n} posts` }))} onChange={(n) => setParams({ ...params, max_posts: n })} />
          <select aria-label="Source" value={params.feed_source} onChange={(e) => setParams({ ...params, feed_source: e.target.value as SessionParams["feed_source"] })} className={input}>
            <option value="for_you">For You feed</option>
            <option value="following">Following feed</option>
            <option value="watchlist">Watchlist accounts</option>
            <option value="both">For You + Watchlist</option>
            <option value="all">All sources</option>
          </select>
          <button type="button" className={btnGhost} aria-expanded={more} onClick={() => setMore(!more)}>
            {more ? "Fewer options" : "More options"}
          </button>
        </fieldset>
        {more && (
          <fieldset disabled={running} className="flex flex-wrap items-center gap-x-6 gap-y-3">
            {numField("Custom minutes", Math.round(params.max_duration_sec / 60), (n) => setParams({ ...params, max_duration_sec: Math.max(1, n) * 60 }), 1, 240)}
            {numField("Custom posts", params.max_posts, (n) => setParams({ ...params, max_posts: n }), 5, 2000)}
            <label className="flex items-center gap-2 text-sm">
              <span className={muted}>Stop after</span>
              <input
                type="number"
                min={1}
                max={500}
                placeholder="no limit"
                value={params.stop_after_queued ?? ""}
                onChange={(e) => setParams({ ...params, stop_after_queued: e.target.value ? Number(e.target.value) : null })}
                className={`${input} w-24`}
              />
              <span className={muted}>queued</span>
            </label>
          </fieldset>
        )}
        {running && <Progress p={progress} params={params} />}
      </section>

      <section aria-labelledby="queue-h" className="space-y-3">
        <div className="flex flex-wrap items-end justify-between gap-3 border-t border-zinc-200 pt-5 dark:border-zinc-800">
          <div>
            <h2 id="queue-h" className="font-display text-lg font-semibold tracking-tight">
              Worth answering <span className="font-mono text-sm font-normal text-zinc-500">{queue.length}</span>
            </h2>
            <p className="mt-0.5 font-mono text-xs text-zinc-500">J/K move / Enter draft / D dismiss</p>
          </div>
          <div className="flex flex-wrap items-center gap-4">
            <input aria-label="Filter queue" type="search" placeholder="Filter by handle or text" value={filter} onChange={(e) => setFilter(e.target.value)} className={`${input} w-52`} />
            <label className="flex items-center gap-2 text-sm">
              <span className={muted}>
                Min score <span className="font-mono text-zinc-950 dark:text-zinc-50">{params.threshold}</span>
              </span>
              <input
                type="range"
                min={0}
                max={100}
                value={params.threshold}
                onChange={(e) => setParams({ ...params, threshold: Number(e.target.value) })}
                onMouseUp={commitThreshold}
                onKeyUp={commitThreshold}
                aria-label="Minimum score"
                className="w-28 accent-zinc-950 dark:accent-zinc-50"
              />
            </label>
          </div>
        </div>

        {!loaded ? (
          <LoadingBlock label="Loading the queue" />
        ) : queue.length === 0 ? (
          <EmptyBlock
            title="Nothing queued yet"
            action={
              running ? undefined : (
                <button type="button" className={btnPrimary} onClick={start} disabled={busy || killed}>
                  Start scouting
                </button>
              )
            }
          >
            {running ? `Scout is reading your feed. Posts scoring above ${params.threshold} appear here as they are found.` : `Nothing scores above ${params.threshold} yet. Start a session, or lower the minimum score.`}
          </EmptyBlock>
        ) : visible.length === 0 ? (
          <EmptyBlock>No queued post matches "{filter}".</EmptyBlock>
        ) : (
          <ul className="divide-y divide-zinc-200 border-y border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
            {visible.map((item) => {
              const i = queue.indexOf(item);
              const tone = scoreTone(item.score);
              const active = i === selected;
              return (
                <li
                  key={item.evaluation_id}
                  aria-current={active ? "true" : undefined}
                  onClick={() => setSelected(i)}
                  className={`relative grid cursor-default grid-cols-[3rem_1fr] gap-x-4 py-4 pl-4 transition-colors ${active ? "bg-zinc-100/70 dark:bg-zinc-900/60" : "hover:bg-zinc-50 dark:hover:bg-zinc-900/30"}`}
                >
                  {active && <span aria-hidden="true" className="absolute inset-y-0 left-0 w-0.5 bg-zinc-950 dark:bg-zinc-50" />}
                  <div title={`Scored by ${item.model_used}`}>
                    <div className="font-mono text-2xl leading-none font-medium tabular-nums">{Math.round(item.score)}</div>
                    <div aria-hidden="true" className={`mt-2 h-0.5 w-6 ${TONE[tone]}`} />
                  </div>
                  <div className="min-w-0">
                    <div className="flex flex-wrap items-baseline justify-between gap-x-3">
                      <span className="text-sm font-medium">@{item.post.handle}</span>
                      <span className="font-mono text-xs text-zinc-500">
                        {compactCount(item.metrics.likes)} likes / {compactCount(item.metrics.reposts)} reposts / {compactCount(item.metrics.replies)} replies
                      </span>
                    </div>
                    <p className={`mt-1 text-sm leading-relaxed whitespace-pre-wrap ${active ? "" : "line-clamp-2"}`}>{item.post.text}</p>
                    {active && (
                      <>
                        <p className="mt-3 text-sm">
                          <span className="eyebrow mr-2">Angle</span>
                          {item.angle}
                        </p>
                        <p className={`mt-1 text-sm ${muted}`}>{item.reasoning}</p>
                        <div className="mt-4 flex flex-wrap items-center gap-2">
                          <button type="button" className={btnPrimary} onClick={() => void draft(item)}>
                            {item.draft_id ? "Continue draft" : "Draft reply"}
                          </button>
                          <a href={item.post.url} target="_blank" rel="noreferrer" className={btn}>
                            View on X
                          </a>
                          <button type="button" className={btnGhost} onClick={() => setDismissing(dismissing === item.evaluation_id ? null : item.evaluation_id)} aria-expanded={dismissing === item.evaluation_id}>
                            Dismiss
                          </button>
                        </div>
                        {dismissing === item.evaluation_id && (
                          <div className="mt-3 flex flex-wrap gap-2" role="group" aria-label="Why dismiss">
                            {REASONS.map((r) => (
                              <button key={r.id} type="button" className={chip(false)} onClick={() => void dismiss(item, r.id)}>
                                {r.label}
                              </button>
                            ))}
                          </div>
                        )}
                      </>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {recent.length > 0 && (
        <details className="group">
          <summary className="eyebrow cursor-pointer list-none hover:text-zinc-950 dark:hover:text-zinc-50">
            Recent sessions <span className="font-mono">{recent.length}</span>
          </summary>
          <ul className="mt-2 divide-y divide-zinc-200 dark:divide-zinc-800">
            {recent.map((s) => (
              <SessionRow key={s.id} s={s} />
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
