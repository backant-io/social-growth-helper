import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { REASONS } from "./logic";
import type { QueueItem, ScoutProgress, SessionParams } from "./types";
import { EmptyBlock, LoadingBlock, PageHeader, Segmented } from "../components/ui";
import { btn, btnGhost, btnPrimary, chip, muted, Notice } from "./ui";

interface Props {
  onOpenDraft: (draftId: string) => void;
}

export function ScoutPage({ onOpenDraft }: Props) {
  const [params, setParams] = useState<SessionParams>({ feed_source: "for_you", max_duration_sec: 600, max_posts: 100, stop_after_queued: null, threshold: 72 });
  const [running, setRunning] = useState(false);
  const [paused, setPaused] = useState(false);
  const [progress, setProgress] = useState<ScoutProgress | null>(null);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [dismissing, setDismissing] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const budgetLoaded = useRef(false);
  const [loaded, setLoaded] = useState(false);

  const reload = useCallback(async () => {
    const [status, q] = await Promise.all([p3.scoutStatus(), p3.scoutQueue()]);
    setRunning(status.running);
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
        setPaused(false);
        void p3.scoutQueue().then(setQueue);
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

  const source: "for_you" | "following" = params.feed_source === "following" ? "following" : "for_you";

  const actions = running ? (
    <button type="button" className={btn} onClick={stop}>
      Stop
    </button>
  ) : (
    <button type="button" data-testid="scout-start" className={btnPrimary} onClick={start} disabled={busy}>
      Start scouting
    </button>
  );

  return (
    <div className="mx-auto max-w-3xl space-y-8">
      <PageHeader title="Scout" description="Find the posts worth answering. Nothing is posted from here." actions={actions} />

      {error && <Notice tone="error">{error}</Notice>}
      {paused && <Notice tone="info">Paused while you use the X window. It resumes a few seconds after you stop.</Notice>}

      <div className="flex flex-wrap items-center justify-between gap-4">
        <Segmented
          label="Feed"
          value={source}
          disabled={running}
          options={[
            { value: "for_you", label: "For You" },
            { value: "following", label: "Following" },
          ]}
          onChange={(v) => setParams({ ...params, feed_source: v })}
        />
        {running && (
          <p aria-live="polite" className={`font-mono text-xs ${muted}`}>
            {progress ? `Reading your feed / ${progress.queued} found` : "Starting..."}
          </p>
        )}
      </div>

      {!loaded ? (
        <LoadingBlock label="Loading the queue" />
      ) : queue.length === 0 ? (
        <EmptyBlock title="No posts yet">{running ? "Scout is reading your feed. Good posts appear here as they are found." : "Press Start scouting to find posts worth answering."}</EmptyBlock>
      ) : (
        <ul className="divide-y divide-zinc-200 border-y border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
          {queue.map((item, i) => {
            const active = i === selected;
            return (
              <li
                key={item.evaluation_id}
                aria-current={active ? "true" : undefined}
                onClick={() => setSelected(i)}
                className={`relative grid cursor-default grid-cols-[2.5rem_1fr] gap-x-4 py-4 pl-4 transition-colors ${active ? "bg-zinc-100/70 dark:bg-zinc-900/60" : "hover:bg-zinc-50 dark:hover:bg-zinc-900/30"}`}
              >
                {active && <span aria-hidden="true" className="absolute inset-y-0 left-0 w-0.5 bg-zinc-950 dark:bg-zinc-50" />}
                <div className="font-mono text-xl leading-none font-medium tabular-nums" title={`Scored by ${item.model_used}`}>
                  {Math.round(item.score)}
                </div>
                <div className="min-w-0">
                  <span className="text-sm font-medium">@{item.post.handle}</span>
                  <p className={`mt-1 text-sm leading-relaxed whitespace-pre-wrap ${active ? "" : "line-clamp-2"}`}>{item.post.text}</p>
                  {active && (
                    <>
                      <p className={`mt-3 text-sm ${muted}`}>{item.angle}</p>
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
    </div>
  );
}
