import { useCallback, useEffect, useRef, useState } from "react";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { DURATION_PRESETS, POST_PRESETS, REASONS, compactCount, formatDuration, scoreTone } from "./logic";
import type { QueueItem, ScoutProgress, SessionParams, SessionView } from "./types";
import { EmptyBlock, PageHeader, useKillSwitch } from "../components/ui";
import { btn, btnPrimary, chip, input, muted, Notice, panel } from "./ui";

interface Props {
  onOpenDraft: (draftId: string) => void;
  onGoToSettings?: () => void;
}

const TONE = { high: "bg-emerald-600", mid: "bg-amber-500", low: "bg-zinc-400" } as const;

function Progress({ p, params }: { p: ScoutProgress | null; params: SessionParams }) {
  const pct = p ? Math.min(100, Math.round((p.scanned / params.max_posts) * 100)) : 0;
  return (
    <div className="mt-3" aria-live="polite">
      <div className="h-2 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-800">
        <div className="h-full bg-zinc-900 transition-all dark:bg-zinc-100" style={{ width: `${pct}%` }} />
      </div>
      <p className={`mt-1 text-xs ${muted}`}>
        {p ? `${p.scanned} of ${params.max_posts} posts read, ${p.queued} queued, ${formatDuration(p.elapsed_sec)} of ${formatDuration(params.max_duration_sec)}` : "Starting..."}
      </p>
    </div>
  );
}

function SessionRow({ s }: { s: SessionView }) {
  const sum = s.summary;
  return (
    <li className="flex flex-wrap items-baseline justify-between gap-2 py-2 text-sm">
      <span>{new Date(s.started_at).toLocaleString()}</span>
      <span className={muted}>
        {s.posts_scanned} read, {s.posts_queued} queued
        {sum ? `, ${sum.heuristic_scored > 0 ? "local score" : sum.model}` : ""}
        {s.ended_reason ? `, ${s.ended_reason.replace(/_/g, " ")}` : ""}
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

  const reload = useCallback(async () => {
    const [status, q] = await Promise.all([p3.scoutStatus(), p3.scoutQueue()]);
    setRunning(status.running);
    setRecent(status.recent);
    const remembered = budgetLoaded.current ? null : status.last_params;
    budgetLoaded.current = true;
    setParams((p) => ({ ...p, ...(remembered ?? {}), threshold: status.threshold }));
    setQueue(q);
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

  const numField = (label: string, value: number, set: (n: number) => void, min: number, max: number) => (
    <label className="flex items-center gap-2 text-sm">
      <span className={muted}>{label}</span>
      <input type="number" min={min} max={max} value={value} onChange={(e) => set(Number(e.target.value))} className={`${input} w-24`} />
    </label>
  );

  return (
    <div className="mx-auto max-w-3xl space-y-6">
      <PageHeader title="Scout Queue" description="You choose how long to scroll and how many posts to read. Nothing is posted from here." />

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

      <section className={panel} aria-labelledby="scout-start">
        <h2 id="scout-start" className="mb-3 text-base font-medium">New session</h2>
        <fieldset disabled={running} className="space-y-4">
          <div>
            <div className={`mb-1 text-xs uppercase tracking-wide ${muted}`}>Duration</div>
            <div className="flex flex-wrap items-center gap-2">
              {DURATION_PRESETS.map((m) => (
                <button key={m} type="button" className={chip(params.max_duration_sec === m * 60)} onClick={() => setParams({ ...params, max_duration_sec: m * 60 })}>
                  {m} min
                </button>
              ))}
              {numField("Custom (min)", Math.round(params.max_duration_sec / 60), (n) => setParams({ ...params, max_duration_sec: Math.max(1, n) * 60 }), 1, 240)}
            </div>
          </div>
          <div>
            <div className={`mb-1 text-xs uppercase tracking-wide ${muted}`}>Posts to read</div>
            <div className="flex flex-wrap items-center gap-2">
              {POST_PRESETS.map((n) => (
                <button key={n} type="button" className={chip(params.max_posts === n)} onClick={() => setParams({ ...params, max_posts: n })}>
                  {n}
                </button>
              ))}
              {numField("Custom", params.max_posts, (n) => setParams({ ...params, max_posts: n }), 5, 2000)}
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-4">
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
            <label className="flex items-center gap-2 text-sm">
              <span className={muted}>Source</span>
              <select value={params.feed_source} onChange={(e) => setParams({ ...params, feed_source: e.target.value as SessionParams["feed_source"] })} className={input}>
                <option value="for_you">For You feed</option>
                <option value="following">Following feed</option>
                <option value="watchlist">Watchlist accounts</option>
                <option value="both">For You + Watchlist</option>
                <option value="all">All sources</option>
              </select>
            </label>
            <label className="flex items-center gap-2 text-sm">
              <span className={muted}>Minimum score {params.threshold}</span>
              <input
                type="range"
                min={0}
                max={100}
                value={params.threshold}
                onChange={(e) => setParams({ ...params, threshold: Number(e.target.value) })}
                onMouseUp={() => void p3.setThreshold(params.threshold).then(() => p3.scoutQueue().then(setQueue))}
                onKeyUp={() => void p3.setThreshold(params.threshold).then(() => p3.scoutQueue().then(setQueue))}
                aria-label="Minimum score"
              />
            </label>
          </div>
        </fieldset>
        <div className="mt-4 flex items-center gap-3">
          {running ? (
            <>
              <button type="button" className={btn} onClick={togglePause}>
                {paused === "manual" ? "Resume session" : "Pause session"}
              </button>
              <button type="button" className={btn} onClick={stop}>
                Stop session
              </button>
            </>
          ) : (
            <button type="button" className={btnPrimary} onClick={start} disabled={busy || killed} title={killed ? "Paused. Resume in the bar above." : undefined}>
              Start scouting
            </button>
          )}
          <span className={`text-xs ${muted}`}>
            {formatDuration(params.max_duration_sec)}, up to {params.max_posts} posts
          </span>
        </div>
        {running && <Progress p={progress} params={params} />}
      </section>

      <section aria-labelledby="queue-h">
        <div className="mb-2 flex items-baseline justify-between">
          <h2 id="queue-h" className="text-base font-medium">
            Worth answering ({queue.length})
          </h2>
          <span className={`text-xs ${muted}`}>J/K to move, Enter to draft, D to dismiss</span>
        </div>
        {queue.length === 0 ? (
          <EmptyBlock title="Nothing queued yet">
            Nothing is scoring above {params.threshold}. Start a session, or lower the minimum score.
          </EmptyBlock>
        ) : (
          <ul className="space-y-3">
            {queue.map((item, i) => {
              const tone = scoreTone(item.score);
              return (
                <li
                  key={item.evaluation_id}
                  aria-current={i === selected ? "true" : undefined}
                  onClick={() => setSelected(i)}
                  className={`rounded-xl border p-4 ${i === selected ? "border-zinc-900 dark:border-zinc-100" : "border-zinc-200 dark:border-zinc-800"}`}
                >
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-sm font-medium">@{item.post.handle}</div>
                      <p className="mt-1 whitespace-pre-wrap text-sm">{item.post.text}</p>
                    </div>
                    <span className={`shrink-0 rounded-full px-2.5 py-1 text-xs font-semibold text-white ${TONE[tone]}`} title={`Scored by ${item.model_used}`}>
                      {Math.round(item.score)}
                    </span>
                  </div>
                  <p className={`mt-2 text-xs ${muted}`}>
                    {compactCount(item.metrics.likes)} likes, {compactCount(item.metrics.reposts)} reposts, {compactCount(item.metrics.replies)} replies
                  </p>
                  <p className="mt-2 text-sm"><span className="font-medium">Angle:</span> {item.angle}</p>
                  <p className={`text-sm ${muted}`}>{item.reasoning}</p>
                  <div className="mt-3 flex flex-wrap items-center gap-2">
                    <button type="button" className={btnPrimary} onClick={() => void draft(item)}>
                      {item.draft_id ? "Continue draft" : "Draft reply"}
                    </button>
                    <a href={item.post.url} target="_blank" rel="noreferrer" className={btn}>
                      View on X
                    </a>
                    <button type="button" className={btn} onClick={() => setDismissing(dismissing === item.evaluation_id ? null : item.evaluation_id)} aria-expanded={dismissing === item.evaluation_id}>
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
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {recent.length > 0 && (
        <section className={panel} aria-labelledby="recent-h">
          <h2 id="recent-h" className="mb-1 text-base font-medium">Recent sessions</h2>
          <ul className="divide-y divide-zinc-200 dark:divide-zinc-800">
            {recent.map((s) => (
              <SessionRow key={s.id} s={s} />
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
