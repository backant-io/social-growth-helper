import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { enqueueGeneration } from "./genQueue";
import { charInfo } from "./logic";
import type { DraftView, QueueItem } from "./types";
import { muted, Notice } from "./ui";

interface Props {
  item: QueueItem;
  /** False while no reply model is assigned: the box stays editable but nothing is generated. */
  canDraft: boolean;
  onDismiss: (reason: string) => void;
  onChanged: () => void;
}

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The reply that sits under a post card: pre-filled, editable in place, one control for a fresh version. */
export function ReplyBox({ item, canDraft, onDismiss, onChanged }: Props) {
  const evalId = item.evaluation_id;
  const [draft, setDraft] = useState<DraftView | null>(null);
  const [text, setText] = useState(item.draft_text ?? "");
  const [generating, setGenerating] = useState(false);
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [near, setNear] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const started = useRef(false);
  const dirty = useRef(false);

  useEffect(() => {
    const el = root.current;
    if (!el || near) return;
    if (typeof IntersectionObserver === "undefined") {
      setNear(true);
      return;
    }
    const io = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && setNear(true), { rootMargin: "500px" });
    io.observe(el);
    return () => io.disconnect();
  }, [near]);

  const fresh = useCallback(
    async (d: DraftView, instruction: string | null) => {
      setGenerating(true);
      setError(null);
      try {
        const known = new Set(d.variants.map((v) => v.id));
        let updated = await enqueueGeneration(() => p3.draftGenerate(d.id, instruction, null));
        const added = updated.variants.find((v) => !known.has(v.id));
        if (added && updated.edited_text !== added.text) updated = await p3.draftSelectVariant(updated.id, added.id);
        setDraft(updated);
        setText(updated.edited_text);
        dirty.current = false;
        setWarnings([]);
      } catch (e) {
        setError(message(e));
      } finally {
        setGenerating(false);
      }
    },
    [],
  );

  useEffect(() => {
    if (!near || started.current) return;
    started.current = true;
    void (async () => {
      try {
        const d = await p3.draftOpen(evalId);
        setDraft(d);
        const existing = (d.edited_text || item.draft_text || "").trim();
        if (existing) {
          setText(d.edited_text || item.draft_text || "");
        } else if (d.variants.length > 0) {
          const v = await p3.draftSelectVariant(d.id, d.variants[0].id);
          setDraft(v);
          setText(v.edited_text);
        } else if (canDraft) {
          await fresh(d, null);
        }
      } catch (e) {
        setError(message(e));
      }
    })();
  }, [near, evalId, item.draft_text, canDraft, fresh]);

  useEffect(() => {
    const off = listen<{ draft_id: string }>("publish:update", (payload) => {
      if (!draft || payload.draft_id !== draft.id) return;
      void p3
        .draftGet(draft.id)
        .then((d) => {
          setDraft(d);
          setSending(d.status === "queued_for_publish" || d.status === "publishing");
          onChanged();
        })
        .catch(() => undefined);
    });
    return () => void off.then((f) => f());
  }, [draft, onChanged]);

  const regenerate = async () => {
    const d = draft ?? (await p3.draftOpen(evalId).catch(() => null));
    if (d) await fresh(d, null);
  };

  const save = () => {
    if (!draft || !dirty.current) return;
    dirty.current = false;
    void p3.draftSaveEdit(draft.id, text).then(setDraft).catch(() => undefined);
  };

  const send = async () => {
    const body = text.trim();
    if (!body) return;
    setError(null);
    setSending(true);
    try {
      const d = draft ?? (await p3.draftOpen(evalId));
      const saved = await p3.draftSaveEdit(d.id, body);
      setDraft(saved);
      const found = await p3.draftCheck(d.id);
      setWarnings(found);
      if (charInfo(body).over) {
        setSending(false);
        return;
      }
      await p3.publishDraft(d.id, body);
      const after = await p3.draftGet(d.id);
      setDraft(after);
      if (after.status === "published" || after.status === "failed") setSending(false);
      onChanged();
    } catch (e) {
      setError(message(e));
      setSending(false);
    }
  };

  const status = draft?.status ?? item.draft_status;
  const sent = status === "published";
  const busy = sending || status === "queued_for_publish" || status === "publishing";
  const info = charInfo(text);
  const failure = status === "failed" ? (draft?.error_message ?? item.draft_error) : null;
  const url = draft?.published_url ?? item.draft_published_url;

  const near280 = info.count > 240;

  return (
    <div ref={root} className="space-y-2">
      <label htmlFor={`reply-${evalId}`} className="sr-only">
        Your reply
      </label>
      <div className={`rounded-2xl border bg-white px-3.5 pt-2.5 pb-2 transition-colors focus-within:border-zinc-900 dark:bg-zinc-950 dark:focus-within:border-zinc-100 ${info.over ? "border-rose-500" : "border-zinc-200 dark:border-zinc-800"}`}>
        <textarea
          id={`reply-${evalId}`}
          rows={2}
          value={text}
          disabled={busy || sent}
          placeholder={generating ? "Writing a reply in your voice..." : canDraft ? "Post your reply" : "Post your reply, or assign a reply model in Settings to draft one."}
          onChange={(e) => {
            dirty.current = true;
            setText(e.target.value);
          }}
          onBlur={save}
          className="block w-full resize-none bg-transparent text-[15px] leading-[1.35] outline-none placeholder:text-zinc-500 disabled:opacity-70"
          style={{ fieldSizing: "content", minHeight: "2.75rem" } as CSSProperties}
        />
        <div className="mt-1 flex items-center justify-between gap-2">
          <div className="flex items-center gap-1">
            {!sent && (
              <button
                type="button"
                onClick={() => void regenerate()}
                disabled={generating || busy || !canDraft}
                aria-label="Write a new reply"
                title={canDraft ? "Write a new reply" : "Assign a reply model in Settings"}
                className="rounded-full p-1.5 text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-zinc-950 disabled:opacity-40 dark:hover:bg-zinc-800 dark:hover:text-zinc-50"
              >
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={generating ? "animate-spin" : ""} aria-hidden="true">
                  <path d="M20 12a8 8 0 1 1-2.5-5.8" />
                  <path d="M20 4v5h-5" />
                </svg>
              </button>
            )}
            {!sent && (
              <button
                type="button"
                onClick={() => onDismiss("other")}
                aria-label="Skip this post"
                title="Skip this post"
                className="rounded-full p-1.5 text-zinc-500 transition-colors hover:bg-zinc-100 hover:text-zinc-950 dark:hover:bg-zinc-800 dark:hover:text-zinc-50"
              >
                <svg viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
                  <path d="M6 6l12 12M18 6 6 18" />
                </svg>
              </button>
            )}
          </div>
          <div className="flex items-center gap-3">
            {near280 && <span className={`text-xs tabular-nums ${info.over ? "text-rose-600 dark:text-rose-400" : muted}`}>{280 - info.count}</span>}
            <button type="button" className="rounded-full bg-zinc-950 px-4 py-1.5 text-sm font-bold text-white transition-opacity hover:opacity-90 disabled:opacity-40 dark:bg-zinc-50 dark:text-zinc-950" disabled={busy || sent || generating || !text.trim() || info.over} onClick={() => void send()}>
              {sent ? "Sent" : busy ? "Sending" : "Reply"}
            </button>
          </div>
        </div>
      </div>

      {warnings.length > 0 && <Notice tone="warn">{warnings.join(" ")}</Notice>}
      {error && <Notice tone="error">{error}</Notice>}
      {failure && <Notice tone="error">{failure}</Notice>}
      {sent && (
        <p className={`text-sm ${muted}`}>
          Reply posted.{" "}
          {url && (
            <a href={url} target="_blank" rel="noreferrer" className="underline">
              View on X
            </a>
          )}
        </p>
      )}
    </div>
  );
}
