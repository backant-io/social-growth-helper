import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { enqueueGeneration } from "./genQueue";
import { charInfo, REASONS } from "./logic";
import type { DraftView, QueueItem } from "./types";
import { btnGhost, btnPrimary, chip, muted, Notice } from "./ui";

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
  const [reasons, setReasons] = useState(false);
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

  return (
    <div ref={root} className="space-y-2.5">
      <div className="flex items-center justify-between gap-2 text-xs">
        <span className={`font-medium ${muted}`}>Your reply</span>
        {!sent && (
          <button type="button" onClick={() => void regenerate()} disabled={generating || busy || !canDraft} aria-label="Write a new reply" title={canDraft ? "Write a new reply" : "Assign a reply model in Settings"} className={`${btnGhost} !px-2 !py-1 text-xs`}>
            <svg viewBox="0 0 24 24" width="14" height="14" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" className={generating ? "animate-spin" : ""} aria-hidden="true">
              <path d="M20 12a8 8 0 1 1-2.5-5.8" />
              <path d="M20 4v5h-5" />
            </svg>
            {generating ? "Writing" : "New reply"}
          </button>
        )}
      </div>

      <label htmlFor={`reply-${evalId}`} className="sr-only">
        Reply text
      </label>
      <textarea
        id={`reply-${evalId}`}
        rows={3}
        value={text}
        disabled={busy || sent}
        placeholder={generating ? "Writing a reply in your voice..." : canDraft ? "Write your reply..." : "Write your reply, or assign a reply model in Settings to draft one for you."}
        onChange={(e) => {
          dirty.current = true;
          setText(e.target.value);
        }}
        onBlur={save}
        className="w-full resize-y rounded-xl border border-zinc-200 bg-white px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-zinc-400 focus:border-zinc-900 disabled:opacity-70 dark:border-zinc-800 dark:bg-zinc-950 dark:focus:border-zinc-100"
      />

      {warnings.length > 0 && <Notice tone="warn">{warnings.join(" ")}</Notice>}
      {error && <Notice tone="error">{error}</Notice>}
      {failure && <Notice tone="error">{failure}</Notice>}
      {sent && (
        <Notice tone="ok">
          Reply sent.{" "}
          {url && (
            <a href={url} target="_blank" rel="noreferrer" className="underline">
              View on X
            </a>
          )}
        </Notice>
      )}

      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className={`font-mono text-xs ${info.over ? "text-rose-600 dark:text-rose-400" : muted}`}>{info.count}/280</span>
        <div className="flex items-center gap-1.5">
          {!sent && (
            <button type="button" className={btnGhost} aria-expanded={reasons} onClick={() => setReasons((v) => !v)}>
              Skip
            </button>
          )}
          <button type="button" className={btnPrimary} disabled={busy || sent || generating || !text.trim() || info.over} onClick={() => void send()}>
            {sent ? "Sent" : busy ? "Sending..." : "Send reply"}
          </button>
        </div>
      </div>

      {reasons && !sent && (
        <div className="flex flex-wrap gap-2" role="group" aria-label="Why skip this post">
          {REASONS.map((r) => (
            <button key={r.id} type="button" className={chip(false)} onClick={() => onDismiss(r.id)}>
              {r.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
