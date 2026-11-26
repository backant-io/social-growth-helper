import { useCallback, useEffect, useMemo, useState } from "react";
import { InlineModelGuard } from "../components/InlineModelGuard";
import { listen } from "../lib/api";
import { offlineActionHint, useOnlineStatus } from "../lib/errors";
import { p3 } from "./api";
import { useShortcuts } from "./hooks";
import { ACTION_META, ACTION_ORDER, MAX_CHARS, bundleText, charInfo, compactCount, editDistance, stepSummary, toggleInBundle } from "./logic";
import type { ActionName, CompletionStatus, DraftView, ModelOption, PostActionState, QueueItem } from "./types";
import { LoadingBlock, useKillSwitch } from "../components/ui";
import { btn, btnPrimary, chip, input, Modal, muted, Notice, panel } from "./ui";

interface Props {
  draftId: string | null;
  onOpenDraft: (draftId: string | null) => void;
  onGoScout: () => void;
  onGoToSettings?: () => void;
}

const TRANSFORMS: Array<{ id: string; label: string }> = [
  { id: "shorter", label: "Shorter" },
  { id: "funnier", label: "Funnier" },
  { id: "more_direct", label: "More direct" },
  { id: "fresh", label: "Regenerate" },
];

const STEP_STYLE: Record<string, string> = {
  done: "text-emerald-600 dark:text-emerald-400",
  skipped: "text-zinc-500",
  pending: "text-zinc-500",
  failed: "text-red-600 dark:text-red-400",
  uncertain: "text-amber-600 dark:text-amber-400",
};

function liveOn(action: ActionName, s: PostActionState | null): boolean {
  if (!s) return false;
  const l = s.live;
  if (l && action in l) return (l as unknown as Record<string, string>)[action] === "done";
  const k = s.known;
  return action === "reply" ? k.replied : action === "quote" ? k.quoted : k[action as "like" | "bookmark" | "repost" | "follow"];
}

export function ReplyStudio({ draftId, onOpenDraft, onGoScout, onGoToSettings }: Props) {
  const online = useOnlineStatus();
  const [draft, setDraft] = useState<DraftView | null>(null);
  const [text, setText] = useState("");
  const [problems, setProblems] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [state, setState] = useState<PostActionState | null>(null);
  const [otherModel, setOtherModel] = useState("");
  const [modelOptions, setModelOptions] = useState<ModelOption[]>([]);
  const [completion, setCompletion] = useState<CompletionStatus | null>(null);
  const [siblings, setSiblings] = useState<QueueItem[]>([]);

  const apply = useCallback((d: DraftView) => {
    setDraft(d);
    setText(d.edited_text);
  }, []);

  const refreshCompletion = useCallback(async () => {
    setCompletion(await p3.completionStatus());
    setModelOptions(await p3.draftModelOptions());
  }, []);

  useEffect(() => {
    void refreshCompletion();
  }, [refreshCompletion]);

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
    if (!draft) return;
    void p3.actionState(draft.post.observed_id, false).then(setState);
  }, [draft?.post.observed_id]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!draftId) return;
    const off = listen<{ draft_id: string }>("publish:update", (e) => {
      if (e.draft_id === draftId) void p3.draftGet(draftId).then(setDraft);
    });
    return () => {
      void off.then((f) => f());
    };
  }, [draftId]);

  const paused = useKillSwitch();
  const editable = draft ? draft.status === "draft" || draft.status === "failed" : false;
  const dirty = draft ? text !== draft.edited_text : false;
  const chars = charInfo(text, MAX_CHARS);
  const distance = useMemo(() => (draft ? editDistance(draft.initial_text, text) : 0), [draft, text]);
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
      <div className="mx-auto flex h-full max-w-md flex-col items-center justify-center text-center">
        <h1 className="mb-2 text-xl font-semibold tracking-tight">Reply Studio</h1>
        <p className={`text-sm ${muted}`}>Pick a post from the Scout Queue to draft a reply in your voice.</p>
        <button type="button" className={`${btnPrimary} mt-4`} onClick={onGoScout}>
          Go to Scout Queue
        </button>
      </div>
    );
  }
  if (!draft) {
    return (
      <div className="mx-auto max-w-3xl space-y-4">
        <h1 className="text-xl font-semibold tracking-tight">Reply Studio</h1>
        {error ? <Notice tone="error">{error}</Notice> : <LoadingBlock label="Loading draft" />}
      </div>
    );
  }

  const toggleBundle = async (a: ActionName) => {
    const next = toggleInBundle(draft.actions, a);
    const d = await run("actions", () => p3.draftSetActions(draft.id, next));
    if (d) setDraft({ ...d, edited_text: draft.edited_text });
  };

  const engage = async (a: ActionName) => {
    const undo = liveOn(a, state);
    const res = await run(`engage-${a}`, () => p3.engage(draft.post.observed_id, a, undo));
    if (res && !res.ok) setError(res.message ?? "That did not work.");
    setState(await p3.actionState(draft.post.observed_id, true));
  };

  const transform = async (instruction: string, model: string | null = null) => {
    const d = await run(`gen-${instruction}`, () => p3.draftGenerate(draft.id, instruction, model));
    if (d) apply(d);
  };

  const pick = async (variantId: string) => {
    const d = await run("variant", () => p3.draftSelectVariant(draft.id, variantId));
    if (d) apply(d);
  };

  const sum = stepSummary(draft.steps);
  const textAction = bundleText(draft.actions);
  const models = [...new Set(draft.variants.map((v) => v.model_used))];
  const offlineHint = offlineActionHint(online);
  const canActNow = draft.status !== "publishing";

  return (
    <div className="mx-auto grid max-w-6xl gap-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <div className="space-y-4">
        <header className="flex items-center justify-between">
          <h1 className="text-xl font-semibold tracking-tight">Reply Studio</h1>
          <div className="flex gap-2">
            <button type="button" className={btn} onClick={() => void move(-1)} aria-label="Previous queue item">Prev</button>
            <button type="button" className={btn} onClick={() => void move(1)} aria-label="Next queue item">Next</button>
          </div>
        </header>
        <section className={panel} aria-label="Original post">
          <div className="flex items-baseline justify-between">
            <span className="text-sm font-medium">@{draft.post.handle}</span>
            <a href={draft.post.url} target="_blank" rel="noreferrer" className="text-xs underline">View on X</a>
          </div>
          <p className="mt-2 whitespace-pre-wrap text-sm">{draft.post.text}</p>
          {draft.post.has_media && <p className={`mt-2 text-xs ${muted}`}>This post has media.</p>}
          {siblings.find((s) => s.evaluation_id === draft.evaluation_id) && (
            <p className={`mt-2 text-xs ${muted}`}>
              {(() => {
                const m = siblings.find((s) => s.evaluation_id === draft.evaluation_id)!.metrics;
                return `${compactCount(m.likes)} likes, ${compactCount(m.reposts)} reposts, ${compactCount(m.replies)} replies`;
              })()}
            </p>
          )}
        </section>
        <section className={panel} aria-label="Why this post was picked">
          <h2 className="mb-1 text-base font-medium">Why this post{draft.score != null ? ` (score ${Math.round(draft.score)})` : ""}</h2>
          {draft.angle && <p className="text-sm"><span className="font-medium">Angle:</span> {draft.angle}</p>}
          {draft.reasoning && <p className={`mt-1 text-sm ${muted}`}>{draft.reasoning}</p>}
        </section>
        <section className={panel} aria-label="Actions on this post">
          <h2 className="mb-1 text-base font-medium">Do it now</h2>
          <p className={`mb-3 text-xs ${muted}`}>One click, one action, in the X window. Reply and Quote go through Publish below.</p>
          <div className="grid grid-cols-2 gap-2">
            {ACTION_ORDER.filter((a) => !ACTION_META[a].text).map((a) => {
              const on = liveOn(a, state);
              return (
                <button key={a} type="button" disabled={!canActNow || busy !== null} className={btn} aria-pressed={on} onClick={() => void engage(a)}>
                  {busy === `engage-${a}` ? "Working..." : on ? ACTION_META[a].undo : ACTION_META[a].label}
                  {on && <span className="ml-1 text-xs text-emerald-600 dark:text-emerald-400">({ACTION_META[a].done})</span>}
                </button>
              );
            })}
          </div>
          {(state?.known.replied || state?.known.quoted) && (
            <p className={`mt-2 text-xs ${muted}`}>You already {state.known.replied ? "replied to" : "quoted"} this post from the app.</p>
          )}
        </section>
      </div>

      <div className="space-y-4">
        {error && <Notice tone="error">{error}</Notice>}
        {completion && !completion.drafting && (
          <InlineModelGuard
            taskType="reply_drafting"
            onAssigned={refreshCompletion}
            onGoToSettings={onGoToSettings}
            hint="You can also write your own text below without a model."
          />
        )}
        {completion && !completion.browser_navigation && draft.status !== "published" && (
          <InlineModelGuard
            taskType="browser_navigation"
            onAssigned={refreshCompletion}
            onGoToSettings={onGoToSettings}
            hint="Publish clicks through X with selectors first. This model is only used as a fallback when a selector breaks; without it, a broken selector fails the step."
          />
        )}
        {draft.status === "published" && (
          <Notice tone="ok">
            Done. {draft.published_url ? <a href={draft.published_url} target="_blank" rel="noreferrer" className="underline">Open on X</a> : "Your actions finished."}
          </Notice>
        )}
        {draft.status === "failed" && draft.error_message && <Notice tone="error">{draft.error_message} Finished steps are kept; Publish again to continue.</Notice>}

        <section className={panel} aria-label="Variants">
          <div className="mb-2 flex flex-wrap items-center justify-between gap-2">
            <h2 className="text-base font-medium">Variants</h2>
            <div className="flex flex-wrap gap-2">
              {TRANSFORMS.map((t) => (
                <button key={t.id} type="button" className={chip(false)} disabled={!editable || busy !== null} onClick={() => void transform(t.id)}>
                  {busy === `gen-${t.id}` ? "..." : t.label}
                </button>
              ))}
            </div>
          </div>
          {draft.variants.length === 0 ? (
            <p className={`text-sm ${muted}`}>No variants yet. Press Regenerate, or write your own.</p>
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
                      className={`w-full rounded-lg border p-3 text-left text-sm ${current ? "border-zinc-900 dark:border-zinc-100" : "border-zinc-200 hover:bg-zinc-50 dark:border-zinc-800 dark:hover:bg-zinc-900"}`}
                    >
                      <p className="whitespace-pre-wrap">{v.text}</p>
                      <p className={`mt-1 text-xs ${muted}`}>{[v.tone, v.model_used].filter(Boolean).join(" . ")}</p>
                    </button>
                  </li>
                );
              })}
            </ul>
          )}
          <div className="mt-3 flex flex-wrap items-center gap-2 border-t border-zinc-200 pt-3 dark:border-zinc-800">
            <label className="flex items-center gap-2 text-xs">
              <span className={muted}>Redraft with another model</span>
              <select value={otherModel} onChange={(e) => setOtherModel(e.target.value)} className={`${input} w-56`} disabled={modelOptions.length === 0}>
                <option value="">{modelOptions.length === 0 ? "Connect a model in Settings" : "Choose a model"}</option>
                {modelOptions.map((m) => (
                  <option key={m.value} value={m.value}>
                    {m.label}
                  </option>
                ))}
              </select>
            </label>
            <button type="button" className={btn} disabled={!editable || !otherModel.trim() || busy !== null} onClick={() => void transform("fresh", otherModel.trim())}>
              Redraft
            </button>
            {models.length > 1 && <span className={`text-xs ${muted}`}>Compare {models.join(" vs ")} above and pick one.</span>}
          </div>
        </section>

        <section className={panel} aria-label="Your reply">
          <label htmlFor="draft-text" className="mb-1 block text-sm font-medium">
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
          <div className="mt-1 flex items-center justify-between text-xs">
            <span className={chars.over ? "font-medium text-red-600" : muted} aria-live="polite">
              {chars.count} / {chars.max}
            </span>
            <span className={muted}>{distance === 0 ? "unedited" : `${distance} edit${distance === 1 ? "" : "s"} from the model draft`}</span>
          </div>
        </section>

        <section className={panel} aria-label="Publish bundle">
          <h2 className="mb-2 text-base font-medium">What Publish will do</h2>
          <div className="flex flex-wrap gap-2">
            {ACTION_ORDER.map((a) => (
              <button key={a} type="button" disabled={!editable || busy !== null} className={chip(draft.actions.includes(a))} aria-pressed={draft.actions.includes(a)} onClick={() => void toggleBundle(a)}>
                {ACTION_META[a].label}
              </button>
            ))}
          </div>
          <p className={`mt-2 text-xs ${muted}`}>Reply and Quote are exclusive. Steps always run in this order: like, bookmark, follow, repost, then your text.</p>
          {draft.steps.length > 0 && (
            <div className="mt-3" aria-label="Publish progress">
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
            className={btn}
            disabled={busy !== null || draft.status === "publishing"}
            onClick={async () => {
              await run("discard", () => p3.draftDiscard(draft.id, null));
              onOpenDraft(null);
              onGoScout();
            }}
          >
            Discard
          </button>
          <div className="flex items-center gap-3">
            <span className={`text-xs ${muted}`}>Cmd/Ctrl+Enter</span>
            <button type="button" className={btnPrimary} disabled={!online || !editable || busy !== null || draft.actions.length === 0 || (hasText && (chars.over || !text.trim()))} title={offlineHint} onClick={() => void openConfirm()}>
              {draft.status === "failed" ? "Retry publish" : "Publish..."}
            </button>
          </div>
        </div>
      </div>

      {confirming && (
        <Modal title="Confirm what will be posted" onClose={() => setConfirming(false)} wide>
          <p className={`mb-3 text-sm ${muted}`}>The app acts in your X window only after you confirm. This is exactly what will happen, in order.</p>
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
    </div>
  );
}
