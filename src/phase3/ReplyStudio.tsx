import { useCallback, useEffect, useState } from "react";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { listen } from "../lib/api";
import { offlineActionHint, useOnlineStatus } from "../lib/errors";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { ACTION_META, ACTION_ORDER, MAX_CHARS, bundleText, charInfo, stepSummary, toggleInBundle } from "./logic";
import type { ActionName, DraftView, QueueItem } from "./types";
import { EmptyBlock, LoadingBlock, Page, PageHeader } from "../components/ui";
import { btn, btnGhost, btnPrimary, chip, input, Modal, muted, Notice } from "./ui";

interface Props {
  draftId: string | null;
  onOpenDraft: (draftId: string | null) => void;
  onGoScout: () => void;
  onGoToSettings?: () => void;
}

const STEP_STYLE: Record<string, string> = {
  done: "text-emerald-600 dark:text-emerald-400",
  skipped: "text-zinc-500",
  pending: "text-zinc-500",
  failed: "text-red-600 dark:text-red-400",
  uncertain: "text-amber-600 dark:text-amber-400",
};

export function ReplyStudio({ draftId, onOpenDraft, onGoScout, onGoToSettings }: Props) {
  const online = useOnlineStatus();
  const [draft, setDraft] = useState<DraftView | null>(null);
  const [text, setText] = useState("");
  const [problems, setProblems] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [siblings, setSiblings] = useState<QueueItem[]>([]);

  const apply = useCallback((d: DraftView) => {
    setDraft(d);
    setText(d.edited_text);
  }, []);

  useEffect(() => {
    setError(null);
    setProblems([]);
    if (!draftId) {
      setDraft(null);
      return;
    }
    p3.draftGet(draftId).then(apply, (e) => setError(String(e.message ?? e)));
  }, [draftId, apply]);

  useEffect(() => {
    void p3.scoutQueue().then(setSiblings);
  }, [draftId]);

  useEffect(() => {
    if (!draftId) return;
    const off = listen<{ draft_id: string }>("publish:update", (e) => {
      if (e.draft_id === draftId) void p3.draftGet(draftId).then(setDraft);
    });
    return () => {
      void off.then((f) => f());
    };
  }, [draftId]);

  const editable = draft ? draft.status === "draft" || draft.status === "failed" : false;
  const dirty = draft ? text !== draft.edited_text : false;
  const chars = charInfo(text, MAX_CHARS);
  const hasText = draft ? bundleText(draft.actions) !== null : false;

  const run = async <T,>(label: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(label);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  };

  const save = async () => {
    if (!draft) return;
    const d = await run("save", () => p3.draftSaveEdit(draft.id, text));
    if (d) apply(d);
  };

  const openConfirm = async () => {
    if (!draft || !editable) return;
    const saved = dirty ? await run("save", () => p3.draftSaveEdit(draft.id, text)) : draft;
    if (!saved) return;
    apply(saved);
    const p = await run("check", () => p3.draftCheck(saved.id));
    setProblems(p ?? []);
    setConfirming(true);
  };

  const publish = async () => {
    if (!draft || !online) return;
    setConfirming(false);
    await run("publish", async () => {
      await p3.draftSetActions(draft.id, draft.actions);
      await p3.publishDraft(draft.id, text);
      apply(await p3.draftGet(draft.id));
    });
  };

  const move = async (dir: 1 | -1) => {
    if (!draft) return;
    const i = siblings.findIndex((s) => s.evaluation_id === draft.evaluation_id);
    const next = siblings[i + dir];
    if (!next) return;
    const d = await run("open", () => p3.draftOpen(next.evaluation_id));
    if (d) onOpenDraft(d.id);
  };

  useShortcuts((a) => {
    if (a === "publish" && draft && editable) void openConfirm();
    else if (a === "next") void move(1);
    else if (a === "prev") void move(-1);
    else return false;
  });

  if (!draftId) {
    return (
      <Page>
        <PageHeader title="Reply Studio" />
        <EmptyBlock
          title="No post selected"
          action={
            <button type="button" className={btnPrimary} onClick={onGoScout}>
              Open Scout
            </button>
          }
        >
          Choose Draft reply on a post in Scout.
        </EmptyBlock>
      </Page>
    );
  }
  if (!draft) {
    return (
      <Page>
        <PageHeader title="Reply Studio" />
        {error ? <Notice tone="error">{error}</Notice> : <LoadingBlock label="Loading draft" />}
      </Page>
    );
  }

  const toggleBundle = async (a: ActionName) => {
    const next = toggleInBundle(draft.actions, a);
    const d = await run("actions", () => p3.draftSetActions(draft.id, next));
    if (d) setDraft({ ...d, edited_text: draft.edited_text });
  };

  const regenerate = async () => {
    const d = await run("regenerate", () => p3.draftGenerate(draft.id, "fresh", null));
    if (d) apply(d);
  };

  const pick = async (variantId: string) => {
    const d = await run("variant", () => p3.draftSelectVariant(draft.id, variantId));
    if (d) apply(d);
  };

  const sum = stepSummary(draft.steps);
  const textAction = bundleText(draft.actions);
  const offlineHint = offlineActionHint(online);

  return (
    <Page>
      <PageHeader
        title="Reply Studio"
        actions={
          <>
            <button type="button" className={btnGhost} onClick={() => void move(-1)} aria-label="Previous queue item">Prev</button>
            <button type="button" className={btnGhost} onClick={() => void move(1)} aria-label="Next queue item">Next</button>
          </>
        }
      />

      <section aria-label="Original post" className="border-l-2 border-zinc-300 pl-4 dark:border-zinc-700">
        <div className="flex items-baseline justify-between gap-3">
          <span className="text-sm font-medium">@{draft.post.handle}</span>
          <a href={draft.post.url} target="_blank" rel="noreferrer" className={`text-sm underline ${muted}`}>View on X</a>
        </div>
        <p className="mt-1 text-sm leading-relaxed whitespace-pre-wrap">{draft.post.text}</p>
        {draft.angle && <p className={`mt-2 text-sm ${muted}`}>{draft.angle}</p>}
      </section>

      {error && <Notice tone="error">{error}</Notice>}
      <InlineModelGuard taskType="reply_drafting" showChip={false} onAssigned={() => undefined} onGoToSettings={onGoToSettings} />
      {draft.status === "published" && (
        <Notice tone="ok">
          Done. {draft.published_url ? <a href={draft.published_url} target="_blank" rel="noreferrer" className="underline">Open on X</a> : "Your actions finished."}
        </Notice>
      )}
      {draft.status === "failed" && draft.error_message && <Notice tone="error">{draft.error_message} Finished steps are kept; publish again to continue.</Notice>}

      <section aria-label="Variants" className="space-y-3">
        <div className="flex items-center justify-between gap-3">
          <h2 className="eyebrow">Drafts</h2>
          <button type="button" className={btnGhost} disabled={!editable || busy !== null} onClick={() => void regenerate()}>
            {busy === "regenerate" ? "Writing..." : "Regenerate"}
          </button>
        </div>
        {draft.variants.length === 0 ? (
          <p className={`text-sm ${muted}`}>No drafts yet. Press Regenerate, or write your own below.</p>
        ) : (
          <ul className="space-y-2">
            {draft.variants.map((v) => {
              const current = v.text === draft.edited_text && !dirty;
              return (
                <li key={v.id}>
                  <button
                    type="button"
                    disabled={!editable}
                    onClick={() => void pick(v.id)}
                    aria-pressed={current}
                    className={`w-full rounded-lg border p-3.5 text-left text-sm transition-colors ${current ? "border-zinc-950 bg-zinc-100/70 dark:border-zinc-50 dark:bg-zinc-900/60" : "border-zinc-200 hover:border-zinc-400 dark:border-zinc-800 dark:hover:border-zinc-600"}`}
                  >
                    <p className="whitespace-pre-wrap">{v.text}</p>
                  </button>
                </li>
              );
            })}
          </ul>
        )}
      </section>

      <section aria-label="Your reply" className="space-y-2">
        <label htmlFor="draft-text" className="eyebrow block">
          {textAction === "quote" ? "Your quote" : "Your reply"}
        </label>
        <textarea
          id="draft-text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={() => dirty && editable && void save()}
          disabled={!editable}
          rows={5}
          className={`${input} w-full resize-y`}
        />
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex flex-wrap gap-2" role="group" aria-label="Actions to include">
            {ACTION_ORDER.map((a) => (
              <button key={a} type="button" disabled={!editable || busy !== null} className={chip(draft.actions.includes(a))} aria-pressed={draft.actions.includes(a)} onClick={() => void toggleBundle(a)}>
                {ACTION_META[a].label}
              </button>
            ))}
          </div>
          <span className={`font-mono text-xs ${chars.over ? "font-medium text-red-600" : muted}`} aria-live="polite">
            {chars.count} / {chars.max}
          </span>
        </div>
        {draft.steps.length > 0 && (
          <div aria-label="Publish progress" className="pt-1">
            <p className="text-xs">{sum.done} of {sum.total} steps finished</p>
            <ol className="mt-1 space-y-1 text-sm">
              {draft.steps.map((s) => (
                <li key={s.action} className={STEP_STYLE[s.status]}>
                  {ACTION_META[s.action as ActionName]?.label ?? s.action}: {s.status}
                  {s.detail ? <span className={`ml-1 text-xs ${muted}`}>{s.detail}</span> : null}
                </li>
              ))}
            </ol>
          </div>
        )}
      </section>

      <div className="flex flex-wrap items-center justify-between gap-3">
        <button
          type="button"
          className={btnGhost}
          disabled={busy !== null || draft.status === "publishing"}
          onClick={async () => {
            await run("discard", () => p3.draftDiscard(draft.id, null));
            onOpenDraft(null);
            onGoScout();
          }}
        >
          Discard
        </button>
        <button type="button" className={btnPrimary} disabled={!online || !editable || busy !== null || draft.actions.length === 0 || (hasText && (chars.over || !text.trim()))} title={offlineHint ?? undefined} onClick={() => void openConfirm()}>
          {draft.status === "failed" ? "Retry publish" : "Review and publish"}
        </button>
      </div>

      {confirming && (
        <Modal title="Confirm what will be posted" onClose={() => setConfirming(false)} wide>
          <p className={`mb-3 text-sm ${muted}`}>This is exactly what will happen, in order.</p>
          <ol className="mb-3 list-decimal space-y-1 pl-5 text-sm">
            {draft.actions.map((a) => (
              <li key={a}>
                {ACTION_META[a].label}
                {ACTION_META[a].text ? ` (your text, below) @${draft.post.handle}` : ` @${draft.post.handle}'s post`}
              </li>
            ))}
          </ol>
          {textAction && (
            <blockquote className="mb-3 whitespace-pre-wrap rounded-lg border border-zinc-300 bg-zinc-50 p-3 text-sm dark:border-zinc-700 dark:bg-zinc-900" aria-label="Exact text">
              {text}
            </blockquote>
          )}
          {problems.length > 0 && <div className="mb-3"><Notice tone="warn">{problems.join(" ")}</Notice></div>}
          <div className="flex justify-end gap-2">
            <button type="button" className={btn} onClick={() => setConfirming(false)}>
              Back to editing
            </button>
            <button type="button" className={btnPrimary} disabled={!online || problems.length > 0} title={offlineHint} onClick={() => void publish()} autoFocus>
              Confirm and publish
            </button>
          </div>
        </Modal>
      )}
    </Page>
  );
}
