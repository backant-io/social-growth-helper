import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../lib/api";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { EmptyBlock, LoadingBlock, Page, PageHeader, Segmented } from "../components/ui";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { charInfo, compactCount, REASONS } from "./logic";
import type { CompletionStatus, DraftView, MediaItem, QueueItem, ScoutProgress, SessionParams } from "./types";
import { btn, btnGhost, btnPrimary, chip, muted, Notice } from "./ui";

interface Props {
  onGoToSettings: () => void;
}

function isPlayableUrl(url: string | null): url is string {
  return Boolean(url && !url.startsWith("blob:"));
}

function PostMedia({ media, postUrl }: { media: MediaItem[]; postUrl: string }) {
  if (!media || media.length === 0) return null;

  const images = media.filter((m) => m.kind === "image" && m.url);
  const videos = media.filter((m) => m.kind === "video" || m.kind === "gif");

  return (
    <div className="mt-2.5 space-y-2" onClick={(e) => e.stopPropagation()}>
      {images.length > 0 && (
        <div className={`grid gap-2 ${images.length === 1 ? "max-w-md grid-cols-1" : "max-w-lg grid-cols-2"}`}>
          {images.map((img, idx) => (
            <a
              key={`${img.url}-${idx}`}
              href={img.url ?? postUrl}
              target="_blank"
              rel="noreferrer"
              className="block overflow-hidden rounded-lg border border-zinc-200 bg-zinc-100 dark:border-zinc-800 dark:bg-zinc-900"
            >
              <img
                src={img.url!}
                alt={img.alt ?? "Post image"}
                loading="lazy"
                className={`w-full object-cover ${images.length === 1 ? "max-h-64" : "h-36"}`}
              />
            </a>
          ))}
        </div>
      )}

      {videos.map((vid, idx) => {
        const playable = isPlayableUrl(vid.url);
        if (playable) {
          return (
            <div key={`${vid.url}-${idx}`} className="max-w-md overflow-hidden rounded-lg border border-zinc-200 bg-zinc-950 dark:border-zinc-800">
              <video
                src={vid.url!}
                poster={vid.poster_url ?? undefined}
                controls={vid.kind === "video"}
                autoPlay={vid.kind === "gif"}
                loop={vid.kind === "gif"}
                muted={vid.kind === "gif"}
                playsInline
                preload="metadata"
                className="max-h-64 w-full object-contain"
              />
              {vid.duration && (
                <div className="flex items-center justify-between px-2.5 py-1 font-mono text-[11px] text-zinc-400">
                  <span>{vid.kind === "gif" ? "GIF" : "Video"}</span>
                  <span>{vid.duration}</span>
                </div>
              )}
            </div>
          );
        }

        return (
          <a
            key={`${vid.poster_url ?? "video"}-${idx}`}
            href={postUrl}
            target="_blank"
            rel="noreferrer"
            className="group relative block max-w-md overflow-hidden rounded-lg border border-zinc-200 bg-zinc-950 dark:border-zinc-800"
          >
            {vid.poster_url ? (
              <img src={vid.poster_url} alt={vid.alt ?? "Video preview"} loading="lazy" className="max-h-60 w-full object-cover opacity-90 transition-opacity group-hover:opacity-100" />
            ) : (
              <div className="flex h-36 items-center justify-center text-xs text-zinc-400">Video preview unavailable</div>
            )}
            <div className="flex items-center justify-between bg-zinc-950/90 px-3 py-1.5 font-mono text-[11px] text-zinc-200">
              <span>{vid.kind === "gif" ? "GIF on X" : "Watch video on X"}</span>
              {vid.duration && <span className="text-zinc-400">{vid.duration}</span>}
            </div>
          </a>
        );
      })}
    </div>
  );
}

export function ScoutPage({ onGoToSettings }: Props) {
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

  const [completion, setCompletion] = useState<CompletionStatus>({ scoring: true, drafting: true, browser_navigation: true });
  const [drafts, setDrafts] = useState<Record<string, DraftView>>({});
  const [texts, setTexts] = useState<Record<string, string>>({});
  const [draftingEval, setDraftingEval] = useState<Record<string, boolean>>({});
  const [sendingEval, setSendingEval] = useState<Record<string, boolean>>({});
  const [draftWarnings, setDraftWarnings] = useState<Record<string, string[]>>({});
  const autoGenAttempted = useRef<Set<string>>(new Set());
  const openingEval = useRef<Set<string>>(new Set());

  const reload = useCallback(async () => {
    const [status, q, comp] = await Promise.all([p3.scoutStatus(), p3.scoutQueue(), p3.completionStatus()]);
    setRunning(status.running);
    setCompletion(comp);
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
      listen<{ draft_id: string; status: string }>("publish:update", (payload) => {
        void p3
          .draftGet(payload.draft_id)
          .then((d) => {
            const evalId = d.evaluation_id;
            if (evalId) {
              setDrafts((prev) => ({ ...prev, [evalId]: d }));
              setSendingEval((prev) => ({ ...prev, [evalId]: d.status === "queued_for_publish" || d.status === "publishing" }));
            }
            void p3.scoutQueue().then(setQueue);
          })
          .catch(() => undefined);
      }),
    ];
    return () => {
      offs.forEach((o) => void o.then((f) => f()));
    };
  }, [reload]);

  const activeItem = queue[selected];

  const ensureDraftLoaded = useCallback(
    async (item: QueueItem) => {
      const evalId = item.evaluation_id;
      if (openingEval.current.has(evalId)) return;
      openingEval.current.add(evalId);
      try {
        const d = await p3.draftOpen(evalId);
        setDrafts((prev) => ({ ...prev, [evalId]: d }));
        setTexts((prev) => (prev[evalId] !== undefined ? prev : { ...prev, [evalId]: d.edited_text || item.draft_text || "" }));

        const hasReplyText = Boolean((d.edited_text && d.edited_text.trim()) || d.variants.length > 0);
        if (!hasReplyText && completion.drafting && !autoGenAttempted.current.has(d.id)) {
          autoGenAttempted.current.add(d.id);
          setDraftingEval((prev) => ({ ...prev, [evalId]: true }));
          try {
            const generated = await p3.draftGenerate(d.id, null, null);
            setDrafts((prev) => ({ ...prev, [evalId]: generated }));
            setTexts((prev) => ({ ...prev, [evalId]: generated.edited_text }));
          } catch (e) {
            setError(e instanceof Error ? e.message : String(e));
          } finally {
            setDraftingEval((prev) => ({ ...prev, [evalId]: false }));
          }
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        openingEval.current.delete(evalId);
      }
    },
    [completion.drafting],
  );

  useEffect(() => {
    if (!activeItem) return;
    if (!drafts[activeItem.evaluation_id]) {
      void ensureDraftLoaded(activeItem);
    }
  }, [activeItem, drafts, ensureDraftLoaded]);

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

  const dismiss = useCallback(async (item: QueueItem, reason: string) => {
    await p3.scoutDismiss(item.evaluation_id, reason);
    setDismissing(null);
    setQueue((q) => q.filter((x) => x.evaluation_id !== item.evaluation_id));
  }, []);

  const regenerate = useCallback(
    async (item: QueueItem, instruction: string | null) => {
      const evalId = item.evaluation_id;
      setError(null);
      setDraftingEval((prev) => ({ ...prev, [evalId]: true }));
      try {
        const d = drafts[evalId] ?? (await p3.draftOpen(evalId));
        const updated = await p3.draftGenerate(d.id, instruction, null);
        setDrafts((prev) => ({ ...prev, [evalId]: updated }));
        setTexts((prev) => ({ ...prev, [evalId]: updated.edited_text }));
        setDraftWarnings((prev) => ({ ...prev, [evalId]: [] }));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setDraftingEval((prev) => ({ ...prev, [evalId]: false }));
      }
    },
    [drafts],
  );

  const selectVariant = useCallback(async (item: QueueItem, variantId: string) => {
    const evalId = item.evaluation_id;
    const d = drafts[evalId];
    if (!d) return;
    try {
      const updated = await p3.draftSelectVariant(d.id, variantId);
      setDrafts((prev) => ({ ...prev, [evalId]: updated }));
      setTexts((prev) => ({ ...prev, [evalId]: updated.edited_text }));
      setDraftWarnings((prev) => ({ ...prev, [evalId]: [] }));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [drafts]);

  const sendReply = useCallback(
    async (item: QueueItem) => {
      const evalId = item.evaluation_id;
      setError(null);
      setSendingEval((prev) => ({ ...prev, [evalId]: true }));
      try {
        const d = drafts[evalId] ?? (await p3.draftOpen(evalId));
        const textToSend = (texts[evalId] ?? d.edited_text ?? item.draft_text ?? "").trim();
        if (!textToSend) {
          setError("Write or generate a reply before sending.");
          setSendingEval((prev) => ({ ...prev, [evalId]: false }));
          return;
        }
        const saved = await p3.draftSaveEdit(d.id, textToSend);
        setDrafts((prev) => ({ ...prev, [evalId]: saved }));
        const warnings = await p3.draftCheck(d.id);
        setDraftWarnings((prev) => ({ ...prev, [evalId]: warnings }));
        if (warnings.length > 0 && charInfo(textToSend).over) {
          setSendingEval((prev) => ({ ...prev, [evalId]: false }));
          return;
        }
        await p3.publishDraft(d.id, textToSend);
        const refreshed = await p3.draftGet(d.id);
        setDrafts((prev) => ({ ...prev, [evalId]: refreshed }));
        if (refreshed.status === "published" || refreshed.status === "failed") {
          setSendingEval((prev) => ({ ...prev, [evalId]: false }));
        }
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
        setSendingEval((prev) => ({ ...prev, [evalId]: false }));
      }
    },
    [drafts, texts],
  );

  useShortcuts((a) => {
    if (a === "next") setSelected((i) => Math.min(queue.length - 1, i + 1));
    else if (a === "prev") setSelected((i) => Math.max(0, i - 1));
    else if (a === "dismiss" && queue[selected]) void dismiss(queue[selected], "other");
    else if (a === "open" && queue[selected]) void ensureDraftLoaded(queue[selected]);
    else if (a === "publish" && queue[selected]) void sendReply(queue[selected]);
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
    <Page>
      <PageHeader title="Scout" description="Review recommended posts with their media and send edited replies in the background." actions={actions} />

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
            const draft = drafts[item.evaluation_id];
            const replyText = texts[item.evaluation_id] ?? draft?.edited_text ?? item.draft_text ?? "";
            const info = charInfo(replyText);
            const draftStatus = draft?.status ?? item.draft_status;
            const publishedUrl = draft?.published_url ?? item.draft_published_url;
            const draftError = draft?.error_message ?? item.draft_error;
            const isDrafting = Boolean(draftingEval[item.evaluation_id]);
            const isSending = Boolean(sendingEval[item.evaluation_id]) || draftStatus === "queued_for_publish" || draftStatus === "publishing";
            const isSent = draftStatus === "published";
            const warnings = draftWarnings[item.evaluation_id] ?? [];

            return (
              <li
                key={item.evaluation_id}
                aria-current={active ? "true" : undefined}
                onClick={() => setSelected(i)}
                className={`relative grid cursor-default grid-cols-[3rem_1fr] gap-x-4 py-4 pl-4 transition-colors ${active ? "bg-zinc-100/70 dark:bg-zinc-900/60" : "hover:bg-zinc-50 dark:hover:bg-zinc-900/30"}`}
              >
                {active && <span aria-hidden="true" className="absolute inset-y-0 left-0 w-0.5 bg-zinc-950 dark:bg-zinc-50" />}
                <div className="pt-0.5">
                  <div className="font-mono text-xl leading-none font-medium tabular-nums" title={`Scored by ${item.model_used}`}>
                    {Math.round(item.score)}
                  </div>
                  {item.outlier_score != null && item.outlier_score >= 60 && (
                    <div className="mt-1 font-mono text-[10px] tracking-wide text-emerald-700 uppercase dark:text-emerald-400" title={`Outlier score ${Math.round(item.outlier_score)}`}>
                      Outlier
                    </div>
                  )}
                </div>

                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-x-2 gap-y-0.5 text-sm">
                    <span className="font-medium">@{item.post.handle}</span>
                    {item.author_verified && <span className={`font-mono text-[11px] ${muted}`}>verified</span>}
                    {item.author_followers != null && <span className={`font-mono text-xs ${muted}`}>{compactCount(item.author_followers)} followers</span>}
                    <span className={`font-mono text-xs ${muted}`}>
                      {compactCount(item.metrics.likes)} likes · {compactCount(item.metrics.bookmarks)} saves · {compactCount(item.metrics.views)} views
                    </span>
                  </div>

                  <p className={`mt-1 text-sm leading-relaxed whitespace-pre-wrap ${active ? "" : "line-clamp-2"}`}>{item.post.text}</p>

                  <PostMedia media={item.post.media} postUrl={item.post.url} />

                  {!active && replyText && (
                    <p className={`mt-2 line-clamp-1 text-xs ${muted}`}>
                      Draft: {replyText}
                    </p>
                  )}

                  {active && (
                    <div className="mt-3 space-y-3" onClick={(e) => e.stopPropagation()}>
                      <div className="flex flex-wrap items-center gap-2">
                        <p className={`text-sm ${muted}`}>{item.angle}</p>
                        {item.breakout_flags.map((flag) => (
                          <span key={flag} className="rounded-full border border-zinc-200 px-2 py-0.5 font-mono text-[11px] text-zinc-600 dark:border-zinc-700 dark:text-zinc-300">
                            {flag}
                          </span>
                        ))}
                      </div>

                      {!completion.drafting && (
                        <InlineModelGuard
                          taskType="reply_drafting"
                          onAssigned={async () => {
                            await reload();
                            await regenerate(item, null);
                          }}
                          onGoToSettings={onGoToSettings}
                          hint="Assign a reply model to auto-draft and refine replies right inside Scout."
                        />
                      )}

                      {draft && draft.variants.length > 1 && (
                        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Reply variants">
                          {draft.variants.map((v, idx) => (
                            <button
                              key={v.id}
                              type="button"
                              className={chip(draft.edited_text === v.text)}
                              onClick={() => void selectVariant(item, v.id)}
                            >
                              {v.tone || `Variant ${idx + 1}`}
                            </button>
                          ))}
                        </div>
                      )}

                      <div className="rounded-lg border border-zinc-200 bg-white p-2.5 dark:border-zinc-800 dark:bg-zinc-950">
                        <label htmlFor={`reply-${item.evaluation_id}`} className="sr-only">
                          Reply text
                        </label>
                        <textarea
                          id={`reply-${item.evaluation_id}`}
                          rows={3}
                          value={replyText}
                          disabled={isSending || isSent}
                          placeholder={isDrafting ? "Drafting a reply in your voice..." : "Write or edit your reply..."}
                          onChange={(e) => {
                            const val = e.target.value;
                            setTexts((prev) => ({ ...prev, [item.evaluation_id]: val }));
                          }}
                          onBlur={() => {
                            if (draft && replyText !== draft.edited_text && !isSent) {
                              void p3.draftSaveEdit(draft.id, replyText).then((saved) => {
                                setDrafts((prev) => ({ ...prev, [item.evaluation_id]: saved }));
                              }).catch(() => undefined);
                            }
                          }}
                          className="w-full resize-y bg-transparent text-sm leading-relaxed text-zinc-950 outline-none placeholder:text-zinc-400 disabled:opacity-70 dark:text-zinc-50"
                        />
                        <div className="mt-1.5 flex items-center justify-between text-xs">
                          <span className={info.over ? "font-mono text-rose-600 dark:text-rose-400" : `font-mono ${muted}`}>
                            {info.count}/280
                          </span>
                          {isDrafting && <span className={`font-mono ${muted}`}>Generating reply...</span>}
                        </div>
                      </div>

                      {warnings.length > 0 && (
                        <Notice tone="warn">{warnings.join(" ")}</Notice>
                      )}
                      {draftStatus === "failed" && draftError && (
                        <Notice tone="error">{draftError}</Notice>
                      )}
                      {isSent && (
                        <Notice tone="info">
                          Reply sent in the background browser.{" "}
                          {publishedUrl && (
                            <a href={publishedUrl} target="_blank" rel="noreferrer" className="underline">
                              View reply on X
                            </a>
                          )}
                        </Notice>
                      )}

                      <div className="flex flex-wrap items-center gap-2">
                        <button
                          type="button"
                          className={btnPrimary}
                          disabled={isSending || isSent || isDrafting || !replyText.trim()}
                          onClick={() => void sendReply(item)}
                        >
                          {isSent ? "Sent" : isSending ? "Sending..." : "Send reply"}
                        </button>
                        {completion.drafting && !isSent && (
                          <>
                            <button type="button" className={btn} disabled={isDrafting || isSending} onClick={() => void regenerate(item, null)}>
                              Regenerate
                            </button>
                            <button type="button" className={btn} disabled={isDrafting || isSending} onClick={() => void regenerate(item, "shorter")}>
                              Shorter
                            </button>
                            <button type="button" className={btn} disabled={isDrafting || isSending} onClick={() => void regenerate(item, "direct")}>
                              Direct
                            </button>
                          </>
                        )}
                        <a href={item.post.url} target="_blank" rel="noreferrer" className={btn}>
                          View on X
                        </a>
                        <button
                          type="button"
                          className={btnGhost}
                          onClick={() => setDismissing(dismissing === item.evaluation_id ? null : item.evaluation_id)}
                          aria-expanded={dismissing === item.evaluation_id}
                        >
                          Dismiss
                        </button>
                      </div>

                      {dismissing === item.evaluation_id && (
                        <div className="flex flex-wrap gap-2" role="group" aria-label="Why dismiss">
                          {REASONS.map((r) => (
                            <button key={r.id} type="button" className={chip(false)} onClick={() => void dismiss(item, r.id)}>
                              {r.label}
                            </button>
                          ))}
                        </div>
                      )}
                    </div>
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </Page>
  );
}
