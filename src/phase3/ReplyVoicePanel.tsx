import { useCallback, useEffect, useState } from "react";
import { btn, btnGhost, btnPrimary, muted, Notice } from "../components/ui";
import { p3 } from "./api";
import type { ReplyVoice } from "./types";

const PLACEHOLDER = "Write like me: blunt, plain words, one specific idea. Never flatter. No hashtags.";
const MAX = 4000;

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The reply system prompt and the successful comment formats that Scout reply drafts are built from. */
export function ReplyVoicePanel({ onGoToSettings }: { onGoToSettings?: () => void }) {
  const [voice, setVoice] = useState<ReplyVoice | null>(null);
  const [draft, setDraft] = useState("");
  const [saving, setSaving] = useState(false);
  const [fetching, setFetching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    const v = await p3.replyVoiceGet();
    setVoice(v);
    setDraft(v.prompt);
  }, []);

  useEffect(() => {
    void load().catch((e) => setError(message(e)));
  }, [load]);

  const run = async (fn: () => Promise<void>) => {
    setError(null);
    setNotice(null);
    try {
      await fn();
    } catch (e) {
      setError(message(e));
    }
  };

  const save = async () => {
    setSaving(true);
    await run(async () => {
      const v = await p3.replyVoiceSetPrompt(draft);
      setVoice(v);
      setDraft(v.prompt);
      setNotice(v.prompt ? "Saved. New replies use this prompt." : "Back to the built-in prompt.");
    });
    setSaving(false);
  };

  const fetchFormats = async () => {
    setFetching(true);
    await run(async () => {
      const s = await p3.replyFormatsFetch();
      setVoice(await p3.replyVoiceGet());
      setNotice(s.added.length ? `Added ${s.added.length} format${s.added.length === 1 ? "" : "s"} from ${s.replies_used} replies on ${s.posts_read} posts.` : "Nothing new: these formats are already saved.");
    });
    setFetching(false);
  };

  const update = (fn: () => Promise<ReplyVoice>) => void run(async () => setVoice(await fn()));
  const changed = draft.trim() !== (voice?.prompt ?? "");
  const formats = voice?.formats ?? [];
  const noReplyModel = error?.toLowerCase().includes("reply model");

  return (
    <div className="space-y-8">
      {error && (
        <Notice tone="error">
          {error}{" "}
          {noReplyModel && onGoToSettings && (
            <button type="button" className="underline" onClick={onGoToSettings}>
              Open Settings
            </button>
          )}
        </Notice>
      )}
      {notice && <Notice tone="ok">{notice}</Notice>}

      <section className="space-y-3" aria-labelledby="reply-prompt-title">
        <div>
          <h3 id="reply-prompt-title" className="text-sm font-medium">
            Reply prompt
          </h3>
          <p className={`mt-0.5 text-xs ${muted}`}>How the AI should sound when it drafts your Scout replies. Leave empty to use the built-in prompt.</p>
        </div>
        <textarea aria-label="Reply system prompt" rows={6} maxLength={MAX} value={draft} onChange={(e) => setDraft(e.target.value)} placeholder={PLACEHOLDER} className="w-full resize-y rounded-xl border border-zinc-200 bg-white px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-zinc-400 focus:border-zinc-900 dark:border-zinc-800 dark:bg-zinc-950 dark:focus:border-zinc-100" />
        <div className="flex items-center justify-between gap-2">
          <span className={`font-mono text-xs ${muted}`}>{voice?.prompt ? "Custom prompt in use" : "Built-in prompt in use"}</span>
          <div className="flex items-center gap-1.5">
            {voice?.prompt && (
              <button type="button" className={btnGhost} onClick={() => setDraft("")} disabled={saving || draft === ""}>
                Clear
              </button>
            )}
            <button type="button" className={btnPrimary} onClick={() => void save()} disabled={saving || !changed}>
              {saving ? "Saving..." : "Save"}
            </button>
          </div>
        </div>
      </section>

      <section className="space-y-3 border-t border-zinc-200 pt-5 dark:border-zinc-800" aria-labelledby="reply-formats-title">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div className="min-w-48 flex-1">
            <h3 id="reply-formats-title" className="text-sm font-medium">
              Successful comment formats
            </h3>
            <p className={`mt-0.5 text-xs ${muted}`}>Structures learned from replies that earned likes under your best scouted posts. Structure only, never the words.</p>
          </div>
          <button type="button" className={btn} onClick={() => void fetchFormats()} disabled={fetching}>
            {fetching ? "Reading replies..." : "Fetch successful comment formats"}
          </button>
        </div>

        {formats.length === 0 ? (
          <p className={`text-sm ${muted}`}>No formats yet. Scout some posts, then fetch.</p>
        ) : (
          <ul className="divide-y divide-zinc-200 dark:divide-zinc-800">
            {formats.map((f) => (
              <li key={f.id} className="flex items-start gap-3 py-3">
                <input type="checkbox" className="mt-1" checked={f.enabled} aria-label={`Use ${f.name} in replies`} onChange={(e) => update(() => p3.replyFormatSetEnabled(f.id, e.target.checked))} />
                <div className={`min-w-0 flex-1 ${f.enabled ? "" : "opacity-50"}`}>
                  <p className="text-sm font-medium">
                    {f.name}
                    <span className={`ml-2 font-mono text-xs font-normal ${muted}`}>{f.avg_likes ? `~${Math.round(f.avg_likes)} likes` : ""}</span>
                  </p>
                  <p className="mt-0.5 text-sm leading-relaxed">{f.pattern}</p>
                  {f.when_to_use && <p className={`mt-0.5 text-xs ${muted}`}>Best for: {f.when_to_use}</p>}
                </div>
                <button type="button" className={`${btnGhost} !px-2 !py-1 text-xs`} aria-label={`Delete ${f.name}`} onClick={() => update(() => p3.replyFormatDelete(f.id))}>
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
