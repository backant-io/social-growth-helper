import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../lib/api";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { XPostCard } from "../components/XPostCard";
import { EmptyBlock, LoadingBlock, Modal, Page, PageHeader, Segmented } from "../components/ui";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { scoreLine } from "./logic";
import { ReplyBox } from "./ReplyBox";
import type { CompletionStatus, QueueItem, ScoutProgress, SessionParams } from "./types";
import { ReplyVoicePanel } from "./ReplyVoicePanel";
import { btn, btnPrimary, input, muted, Notice } from "./ui";

interface Props {
  onGoToSettings: () => void;
}

type Feed = "for_you" | "following" | "both";

const SESSION_SECONDS = 3600;

/** A session reads until it has found the target, so the read budget is generous but still bounded. */
export function postsBudget(target: number): number {
  return Math.min(2000, Math.max(200, target * 20));
}

function toFeed(source: SessionParams["feed_source"]): Feed {
  return source === "following" ? "following" : source === "both" || source === "all" ? "both" : "for_you";
}

export function ScoutPage({ onGoToSettings }: Props) {
  const [feed, setFeed] = useState<Feed>("for_you");
  const [target, setTarget] = useState(20);
  const [threshold, setThreshold] = useState(72);
  const [running, setRunning] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [paused, setPaused] = useState(false);
  const [progress, setProgress] = useState<ScoutProgress | null>(null);
  const [queue, setQueue] = useState<QueueItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState(0);
  const [busy, setBusy] = useState(false);
  const [setup, setSetup] = useState(false);
  const [startError, setStartError] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [voiceOpen, setVoiceOpen] = useState(false);
  const [completion, setCompletion] = useState<CompletionStatus>({ scoring: true, drafting: true, browser_navigation: true });
  const remembered = useRef(false);
  const stoppingRef = useRef(false);
  const reloadSeq = useRef(0);

  const refreshQueue = useCallback(() => void p3.scoutQueue().then(setQueue).catch(() => undefined), []);

  const reload = useCallback(async () => {
    const seq = ++reloadSeq.current;
    const [status, q, comp] = await Promise.all([p3.scoutStatus(), p3.scoutQueue(), p3.completionStatus()]);
    if (seq !== reloadSeq.current) return;
    setRunning(status.running);
    if (!status.running) {
      setPaused(false);
      setStopping(false);
      stoppingRef.current = false;
    }
    setCompletion(comp);
    setThreshold(status.threshold);
    if (!remembered.current && status.last_params) {
      remembered.current = true;
      setFeed(toFeed(status.last_params.feed_source));
      if (status.last_params.stop_after_queued) setTarget(status.last_params.stop_after_queued);
    }
    setQueue(q);
    setLoaded(true);
    setSelected((i) => Math.min(i, Math.max(0, q.length - 1)));
  }, []);

  useEffect(() => {
    void reload().catch((e) => setError(String(e.message ?? e)));
    const offs = [
      listen<ScoutProgress>("scout:progress", (p) => {
        if (stoppingRef.current) return;
        setProgress(p);
        setPaused(false);
        refreshQueue();
      }),
      listen("scout:finished", () => {
        reloadSeq.current += 1;
        stoppingRef.current = false;
        setStopping(false);
        setRunning(false);
        setPaused(false);
        void reload();
      }),
      listen<{ message: string }>("scout:error", (e) => setError(e.message)),
      listen("scout:paused", () => {
        if (!stoppingRef.current) setPaused(true);
      }),
      listen("scout:resumed", () => setPaused(false)),
    ];
    return () => offs.forEach((o) => void o.then((f) => f()));
  }, [reload, refreshQueue]);

  const start = async () => {
    setStartError(null);
    setError(null);
    setBusy(true);
    try {
      const n = Math.max(1, Math.min(500, Math.round(target) || 1));
      await p3.scoutStart({ feed_source: feed, max_duration_sec: SESSION_SECONDS, max_posts: postsBudget(n), stop_after_queued: n, threshold });
      reloadSeq.current += 1;
      stoppingRef.current = false;
      setStopping(false);
      setProgress(null);
      setRunning(true);
      setSetup(false);
    } catch (e) {
      setStartError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    if (stoppingRef.current) return;
    stoppingRef.current = true;
    setStopping(true);
    setError(null);
    try {
      await p3.scoutStop();
      reloadSeq.current += 1;
      setRunning(false);
      setPaused(false);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      stoppingRef.current = false;
      setStopping(false);
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

  return (
    <Page>
      <PageHeader
        title="Scout"
        actions={
          <div className="flex items-center gap-2">
            <button type="button" className={btn} onClick={() => setVoiceOpen(true)}>
              Reply prompt
            </button>
            {running ? (
              <button type="button" data-testid="scout-stop" className={btn} onClick={() => void stop()} disabled={stopping}>
                {stopping ? "Stopping..." : "Stop"}
              </button>
            ) : (
              <button type="button" data-testid="scout-start" className={btnPrimary} onClick={() => { setStartError(null); setSetup(true); }}>
                Start scouting
              </button>
            )}
          </div>
        }
      />

      {error && <Notice tone="error">{error}</Notice>}
      {paused && <Notice tone="info">Paused while you use the X window. It resumes a few seconds after you stop.</Notice>}
      {!completion.drafting && <InlineModelGuard taskType="reply_drafting" onAssigned={reload} onGoToSettings={onGoToSettings} hint="Assign a reply model so every post gets a drafted reply." />}

      {running && (
        <p aria-live="polite" className="text-sm font-medium tabular-nums">
          {found} of {goal} posts found
          {progress && <span className={`ml-2 font-normal ${muted}`}>{progress.scanned} read</span>}
        </p>
      )}

      {setup && (
        <Modal
          title="Start scouting"
          onClose={() => !busy && setSetup(false)}
          actions={
            <>
              <button type="button" className={btn} onClick={() => setSetup(false)} disabled={busy}>
                Cancel
              </button>
              <button type="button" data-testid="scout-confirm-start" className={btnPrimary} onClick={start} disabled={busy}>
                {busy ? "Starting..." : "Start"}
              </button>
            </>
          }
        >
          <div className="space-y-5">
            {startError && <Notice tone="error">{startError}</Notice>}
            <div>
              <span className={`mb-1.5 block text-sm ${muted}`}>Feed</span>
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
              <span className={`mb-1.5 block text-sm ${muted}`}>Posts to find</span>
              <input type="number" min={1} max={500} value={target} onChange={(e) => setTarget(Number(e.target.value))} onKeyDown={(e) => e.key === "Enter" && !busy && void start()} className={`${input} w-28`} autoFocus />
            </label>
            <p className={`text-xs ${muted}`}>Scout keeps scrolling until it has found this many posts, or until you press Stop.</p>
          </div>
        </Modal>
      )}

      {!loaded ? (
        <LoadingBlock label="Loading the queue" />
      ) : queue.length === 0 ? (
        <EmptyBlock title="No posts yet">{running ? "Posts appear here as Scout finds them." : "Choose a feed and how many posts you want, then press Start scouting."}</EmptyBlock>
      ) : (
        <ul className="divide-y divide-zinc-200 border-y border-zinc-200 dark:divide-zinc-800 dark:border-zinc-800">
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
                signal={scoreLine(item.score, item.breakout_flags ?? [])}
                signalTitle={[item.angle && `Angle: ${item.angle}`, item.reasoning].filter(Boolean).join("\n") || undefined}
                selected={i === selected}
                onSelect={() => setSelected(i)}
              >
                <ReplyBox item={item} canDraft={completion.drafting} onDismiss={(reason) => void dismiss(item, reason)} onChanged={refreshQueue} />
              </XPostCard>
            </li>
          ))}
        </ul>
      )}
      {voiceOpen && (
        <Modal title="Reply prompt" wide onClose={() => setVoiceOpen(false)} actions={<button type="button" className={btn} onClick={() => setVoiceOpen(false)}>Done</button>}>
          <ReplyVoicePanel onGoToSettings={onGoToSettings} />
        </Modal>
      )}
    </Page>
  );
}
