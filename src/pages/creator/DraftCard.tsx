import { useEffect, useState } from "react";
import { canEdit, fileName, hasPlaceholders, parsePaths, postLength, similarityNote, statusLabel } from "../../lib/creator";
import { offlineActionHint, useOnlineStatus } from "../../lib/errors";
import { creatorApi } from "../../lib/creatorApi";
import type { FormatRecord, OgcDraft } from "../../lib/types";
import { btnGhost, Notice } from "../../components/ui";
import { btn, danger, field, Modal, muted, primary } from "./ui";

interface Props {
  draft: OgcDraft;
  format: FormatRecord | undefined;
  threshold: number;
  maxChars: number;
  canPublish: boolean;
  publishBlock?: string;
  onChanged: (d: OgcDraft) => void;
  onRemoved: (id: string) => void;
}

const TONE = {
  ok: "text-emerald-600 dark:text-emerald-400",
  warn: "text-amber-600 dark:text-amber-400",
  none: "text-zinc-500",
};

export function DraftCard({ draft, format, threshold, maxChars, canPublish, publishBlock, onChanged, onRemoved }: Props) {
  const api = creatorApi();
  const online = useOnlineStatus();
  const [text, setText] = useState(draft.edited_text);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirming, setConfirming] = useState<OgcDraft | null>(null);
  const [pathInput, setPathInput] = useState("");

  useEffect(() => setText(draft.edited_text), [draft.edited_text]);

  const editable = canEdit(draft.status);
  const dirty = text !== draft.edited_text;
  const length = postLength(text);
  const over = length > maxChars;
  const note = similarityNote(draft.similarity, threshold);

  async function run<T>(fn: () => Promise<T>): Promise<T | undefined> {
    setBusy(true);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return undefined;
    } finally {
      setBusy(false);
    }
  }

  async function save(): Promise<OgcDraft | undefined> {
    const d = await run(() => api.edit(draft.id, text));
    if (d) onChanged(d);
    return d;
  }

  async function startPublish() {
    const saved = dirty ? await save() : draft;
    if (saved) setConfirming(saved);
  }

  async function attach() {
    const paths = parsePaths(pathInput);
    if (!paths.length) return;
    const d = await run(() => api.setMedia(draft.id, [...draft.media_paths, ...paths]));
    if (d) {
      onChanged(d);
      setPathInput("");
    }
  }

  async function removeMedia(path: string) {
    const d = await run(() => api.setMedia(draft.id, draft.media_paths.filter((p) => p !== path)));
    if (d) onChanged(d);
  }

  async function confirmPublish() {
    if (!online) return;
    if (!confirming) return;
    const target = confirming;
    setConfirming(null);
    const d = await run(() => api.publish(target.id, target.edited_text));
    if (d) onChanged(d);
  }

  const warnings: string[] = [];
  if (confirming) {
    if (confirming.similarity !== null && confirming.similarity >= threshold) warnings.push("This is close to the viral post it follows.");
    if (hasPlaceholders(confirming.edited_text)) warnings.push("It still contains {placeholders} that were never filled in.");
  }

  return (
    <article className="rounded-2xl border border-zinc-200 p-4 dark:border-zinc-800" data-draft-id={draft.id} aria-label="Draft">
      <header className="mb-2 flex items-center justify-between text-xs">
        <span className={`flex min-w-0 flex-wrap items-center gap-2 ${muted}`}>
          <span className={`rounded-full px-2 py-0.5 font-medium ${draft.status === "published" ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300" : draft.status === "failed" || draft.status === "unconfirmed" ? "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300" : "bg-zinc-100 text-zinc-700 dark:bg-zinc-800 dark:text-zinc-300"}`}>
            {statusLabel(draft.status)}
          </span>
          <span className="truncate">
            {format ? format.name : ""}
            {format && draft.topic_seed ? " · " : ""}
            {draft.topic_seed ?? ""}
          </span>
        </span>
        {draft.status === "published" && draft.published_url && (
          <a className="underline" href={draft.published_url} target="_blank" rel="noreferrer">View on X</a>
        )}
      </header>

      <textarea
        aria-label="Post text"
        className={`${field} min-h-36 leading-relaxed disabled:opacity-60`}
        value={text}
        disabled={!editable || busy}
        onChange={(e) => setText(e.target.value)}
        onBlur={() => {
          if (dirty && !busy && text.trim()) void save();
        }}
      />
      <div className="mt-1 flex items-center justify-between text-xs">
        <span className={over ? danger : muted} aria-live="polite">{length} / {maxChars}</span>
        <span className={TONE[note.tone]}>{note.text}</span>
      </div>

      {draft.media_paths.length > 0 && (
        <ul className="mt-2 flex flex-wrap gap-1.5 text-xs" aria-label="Attached media">
          {draft.media_paths.map((p) => (
            <li key={p} className="flex items-center gap-1 rounded-full border border-zinc-300 px-2 py-0.5 dark:border-zinc-700" title={p}>
              {fileName(p)}
              {editable && (
                <button type="button" aria-label={`Remove ${fileName(p)}`} className="opacity-60 hover:opacity-100" disabled={busy} onClick={() => removeMedia(p)}>
                  x
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {editable && (
        <details className="mt-2 text-xs">
          <summary className="cursor-pointer text-zinc-500">Attach images, a GIF or a video</summary>
          <div className="mt-2 space-y-2">
            <p className={muted}>Drop files on this card, or paste full file paths, one per line. Up to 4 images, or one GIF or video.</p>
            <textarea aria-label="Media file paths" className={`${field} min-h-14 font-mono text-xs`} value={pathInput} onChange={(e) => setPathInput(e.target.value)} placeholder="/Users/you/Pictures/chart.png" />
            <button type="button" className={btn} disabled={busy || !pathInput.trim()} onClick={attach}>Attach</button>
          </div>
        </details>
      )}

      {draft.status === "unconfirmed" && (
        <div role="alert" className="mt-3 rounded-lg border border-amber-300 bg-amber-50 p-3 text-sm text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200">
          <p>{draft.error_message}</p>
          <p className="mt-1">It will not be posted again by itself. Look at your X profile first.</p>
          <button type="button" className={`${btn} mt-2`} disabled={busy} onClick={async () => { const d = await run(() => api.reopen(draft.id)); if (d) onChanged(d); }}>
            It is not on X, let me post again
          </button>
        </div>
      )}
      {draft.status === "failed" && draft.error_message && <div className="mt-3"><Notice tone="error">{draft.error_message}</Notice></div>}
      {error && <div className="mt-3"><Notice tone="error">{error}</Notice></div>}

      {editable && (
        <footer className="mt-3 flex flex-wrap items-center justify-between gap-2 border-t border-zinc-200 pt-3 dark:border-zinc-800">
          <button type="button" className={btnGhost} disabled={busy} onClick={async () => { if (await run(async () => { await api.discard(draft.id); return true; })) onRemoved(draft.id); }}>
            Discard
          </button>
          <div className="flex flex-wrap items-center gap-2">
            {publishBlock && <span className={`text-xs ${muted}`}>{publishBlock}</span>}
            <button type="button" className={primary} disabled={busy || !online || !canPublish || !text.trim() || over} onClick={startPublish} title={offlineActionHint(online) ?? publishBlock}>
              Post
            </button>
          </div>
        </footer>
      )}

      {confirming && (
        <Modal
          title="Post this to X?"
          onClose={() => setConfirming(null)}
          actions={
            <>
              <button type="button" className={btn} onClick={() => setConfirming(null)}>Back to editing</button>
              <button type="button" className={primary} disabled={!online} title={offlineActionHint(online)} onClick={confirmPublish}>Approve and post to X</button>
            </>
          }
        >
          <p className={`mb-2 text-sm ${muted}`}>
            The app will type this into the compose box in your X window and press Post. This is the only step that writes to X.
          </p>
          <blockquote className="whitespace-pre-wrap rounded-lg border border-zinc-200 bg-zinc-50 p-3 text-sm dark:border-zinc-800 dark:bg-zinc-900">{confirming.edited_text}</blockquote>
          {confirming.media_paths.length > 0 && <p className="mt-2 text-xs">With {confirming.media_paths.length} attachment(s): {confirming.media_paths.map(fileName).join(", ")}</p>}
          {warnings.map((w) => (
            <p key={w} role="alert" className="mt-2 text-sm text-amber-600 dark:text-amber-400">{w}</p>
          ))}
        </Modal>
      )}
    </article>
  );
}
