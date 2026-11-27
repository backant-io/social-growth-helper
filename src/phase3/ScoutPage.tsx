import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../lib/api";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { XPostCard } from "../components/XPostCard";
import { EmptyBlock, LoadingBlock, Page, PageHeader, Segmented } from "../components/ui";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { ReplyBox } from "./ReplyBox";
import type { CompletionStatus, QueueItem, ScoutProgress, SessionParams } from "./types";
import { btn, btnPrimary, input, muted, Notice } from "./ui";

interface Props {
  onGoToSettings: () => void;
}

type Feed = "for_you" | "following" | "both";

const TIME_LIMITS = [
  { value: 600, label: "10 min" },
  { value: 1800, label: "30 min" },
  { value: 3600, label: "1 hour" },
  { value: 7200, label: "2 hours" },
];

/** A session reads until it has found the target, so the read budget is generous but still bounded. */
export function postsBudget(target: number): number {
  return Math.min(2000, Math.max(200, target * 20));
}

function toFeed(source: SessionParams["feed_source"]): Feed {
  return source === "following" ? "following" : source === "both" || source === "all" ? "both" : "for_you";
}

function Badge({ item }: { item: QueueItem }) {
  const outlier = item.outlier_score != null && item.outlier_score >= 60;
  return (
    <div className="flex items-center gap-1.5 font-mono text-[11px]">
      {outlier && <span className="rounded-full bg-emerald-100 px-2 py-0.5 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300">Outlier</span>}
      <span className="rounded-full border border-zinc-300 px-2 py-0.5 tabular-nums dark:border-zinc-700" title={`Relevance score from ${item.model_used}`}>
        {Math.round(item.score)}
      </span>
    </div>
  );
}

export function ScoutPage({ onGoToSettings }: Props) {
  const [feed, setFeed] = useState<Feed>("for_you");
  const [target, setTarget] = useState(20);
  const [duration, setDuration] = useState(1800);
  const [threshold, setThreshold] = useState(72);
  const [running, setRunning] = useState(false);
  const [paused, setPaused] = useState(false);
  const [progress, setProgress] = useState<ScoutProgress | null>(null);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [completion, setCompletion] = useState<CompletionStatus>({ scoring: true, drafting: true, browser_navigation: true });
  const remembered = useRef(false);

  const refreshQueue = useCallback(() => void p3.scoutQueue().then(setQueue).catch(() => undefined), []);

  const reload = useCallback(async () => {
    const [status, q, comp] = await Promise.all([p3.scoutStatus(), p3.scoutQueue(), p3.completionStatus()]);
    setRunning(status.running);
    setCompletion(comp);
    setThreshold(status.threshold);
    if (!remembered.current && status.last_params) {
      remembered.current = true;
      setFeed(toFeed(status.last_params.feed_source));
      if (status.last_params.stop_after_queued) setTarget(status.last_params.stop_after_queued);
      if (TIME_LIMITS.some((t) => t.value === status.last_params!.max_duration_sec)) setDuration(status.last_params.max_duration_sec);
    }
    setQueue(q);
    setLoaded(true);
    setSelected((i) => Math.min(i, Math.max(0, q.length - 1)));
  }, []);

  useEffect(() => {
    void reload().catch((e) => setError(String(e.message ?? e)));
    const offs = [
      listen<ScoutProgress>("scout:progress", (p) => {
        setProgress(p);
        setPaused(false);
        refreshQueue();
      }),
      listen("scout:finished", () => {
        setRunning(false);
        setPaused(false);
        void reload();
      }),
      listen<{ message: string }>("scout:error", (e) => setError(e.message)),
      listen("scout:paused", () => setPaused(true)),
      listen("scout:resumed", () => setPaused(false)),
    ];
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, [reload, refreshQueue]);

  const start = async () => {
    setError(null);
    setBusy(true);
    try {
      const n = Math.max(1, Math.min(500, Math.round(target) || 1));
      await p3.scoutStart({ feed_source: feed, max_duration_sec: duration, max_posts: postsBudget(n), stop_after_queued: n, threshold });
      setProgress(null);
      setRunning(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const dismiss = useCallback(async (item: QueueItem, reason: string) => {
    await p3.scoutDismiss(item.evaluation_id, reason);
    setQueue((q) => q.filter((x) => x.evaluation_id !== item.evaluation_id));
  }, []);

  const focusCard = (i: number) => {
    setSelected(i);
    document.getElementById(`scout-card-${i}`)?.scrollIntoView({ block: "center", behavior: "smooth" });
  };

  useShortcuts((a) => {
    if (a === "next") focusCard(Math.min(queue.length - 1, selected + 1));
    else if (a === "prev") focusCard(Math.max(0, selected - 1));
    else if (a === "dismiss" && queue[selected]) void dismiss(queue[selected], "other");
    else return false;
  });

  const found = progress?.queued ?? queue.length;
  const goal = progress?.target ?? target;
  const pct = Math.min(100, Math.round((found / Math.max(1, goal)) * 100));

  return (
    <Page>
      <PageHeader title="Scout" description="Posts worth answering, each with a reply ready to edit and send." />

      {error && <Notice tone="error">{error}</Notice>}
      {paused && <Notice tone="info">Paused while you use the X window. It resumes a few seconds after you stop.</Notice>}
      {!completion.drafting && <InlineModelGuard taskType="reply_drafting" onAssigned={reload} onGoToSettings={onGoToSettings} hint="Assign a reply model so every post gets a drafted reply." />}

      {running ? (
        <section aria-live="polite" className="flex flex-wrap items-center gap-4 rounded-xl border border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <div className="min-w-48 flex-1">
            <p className="text-sm font-medium">
              {found} of {goal} posts found
              {progress && <span className={`ml-2 font-mono text-xs font-normal ${muted}`}>{progress.scanned} read</span>}
            </p>
            <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-800" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label="Posts found">
              <div className="h-full rounded-full bg-zinc-950 transition-all dark:bg-zinc-100" style={{ width: `${pct}%` }} />
            </div>
          </div>
          <button type="button" className={btn} onClick={() => void p3.scoutStop()}>
            Stop
          </button>
        </section>
      ) : (
        <section aria-label="Scouting session" className="flex flex-wrap items-end gap-x-5 gap-y-3 rounded-xl border border-zinc-200 px-4 py-3 dark:border-zinc-800">
          <div>
            <span className={`mb-1 block text-xs ${muted}`}>Feed</span>
            <Segmented
              label="Feed"
              value={feed}
              options={[
                { value: "for_you", label: "For You" },
                { value: "following", label: "Following" },
                { value: "both", label: "Both" },
              ]}
              onChange={setFeed}
            />
          </div>
          <label className="block">
            <span className={`mb-1 block text-xs ${muted}`}>Posts to find</span>
            <input type="number" min={1} max={500} value={target} onChange={(e) => setTarget(Number(e.target.value))} className={`${input} w-24`} />
          </label>
          <label className="block">
            <span className={`mb-1 block text-xs ${muted}`}>Time limit</span>
            <select value={duration} onChange={(e) => setDuration(Number(e.target.value))} className={input}>
              {TIME_LIMITS.map((t) => (
                <option key={t.value} value={t.value}>
                  {t.label}
                </option>
              ))}
            </select>
          </label>
          <button type="button" data-testid="scout-start" className={`${btnPrimary} ml-auto`} onClick={start} disabled={busy}>
            Start scouting
          </button>
        </section>
      )}

      {!loaded ? (
        <LoadingBlock label="Loading the queue" />
      ) : queue.length === 0 ? (
        <EmptyBlock title="No posts yet">{running ? "Scout keeps scrolling your feed until it has found enough. Posts appear here as they are found." : "Choose a feed and how many posts you want, then press Start scouting."}</EmptyBlock>
      ) : (
        <ul className="space-y-4">
          {queue.map((item, i) => (
            <li key={item.evaluation_id} id={`scout-card-${i}`}>
              <XPostCard
                handle={item.post.handle}
                name={item.post.author_name || undefined}
                avatarUrl={item.post.avatar_url}
                verified={item.author_verified}
                postedAt={item.posted_at}
                text={item.post.text}
                media={item.post.media}
                url={item.post.url}
                metrics={item.metrics}
                badge={<Badge item={item} />}
                selected={i === selected}
                onSelect={() => setSelected(i)}
              >
                {item.angle && <p className={`mb-2.5 text-xs ${muted}`}>Angle: {item.angle}</p>}
                <ReplyBox item={item} canDraft={completion.drafting} onDismiss={(reason) => void dismiss(item, reason)} onChanged={refreshQueue} />
              </XPostCard>
            </li>
          ))}
        </ul>
      )}
    </Page>
  );
}
