import { useState } from "react";
import { DiffView } from "./DiffView";
import { categoryLabel, confidenceLabel, describeOp, editedPatch, evidenceLine, opLabel, opText, patchesEqual, targetLabel } from "./model";
import type { Insight } from "./types";
import { btn, btnPrimary, input, panel } from "./ui";

interface Props {
  insight: Insight;
  busy: boolean;
  onAccept: (insight: Insight, edited: Insight["patch"] | null) => void;
  onReject: (insight: Insight) => void;
}

export function InsightCard({ insight, busy, onAccept, onReject }: Props) {
  const [editing, setEditing] = useState(false);
  const [texts, setTexts] = useState<Record<number, string>>({});
  const [showEvidence, setShowEvidence] = useState(false);

  const draftPatch = editing ? editedPatch(insight.patch, texts) : insight.patch;
  const changed = draftPatch ? !patchesEqual(draftPatch, insight.patch) : false;
  const canApply = !busy && !insight.stale && draftPatch !== null;

  return (
    <article className={panel} aria-label={insight.hypothesis}>
      <div className="flex flex-wrap items-center gap-2 text-xs">
        <span className="rounded-full bg-zinc-100 px-2 py-0.5 font-medium dark:bg-zinc-800">{categoryLabel(insight.category)}</span>
        <span className="text-zinc-500">Changes: {targetLabel(insight.patch.target)}</span>
        <span
          className={`rounded-full px-2 py-0.5 ${insight.low_confidence ? "border border-zinc-400 text-zinc-700 dark:border-zinc-600 dark:text-zinc-300" : "bg-zinc-100 dark:bg-zinc-800"}`}
          title={`Confidence ${(insight.confidence * 100).toFixed(0)}% from ${insight.sample_size} published replies`}
        >
          {confidenceLabel(insight.confidence)} confidence{insight.low_confidence ? " (small sample)" : ""}
        </span>
        {insight.source === "llm" && <span className="rounded-full bg-violet-100 px-2 py-0.5 text-violet-800 dark:bg-violet-950 dark:text-violet-300">AI-assisted</span>}
      </div>

      <p className="mt-2 text-sm">{insight.hypothesis}</p>
      {evidenceLine(insight.evidence) && (
        <p className="mt-1 text-xs text-zinc-500">
          Based on {evidenceLine(insight.evidence)} replies.{" "}
          {insight.evidence.items.length > 0 && (
            <button type="button" className="underline" onClick={() => setShowEvidence((v) => !v)} aria-expanded={showEvidence}>
              {showEvidence ? "Hide examples" : "Show examples"}
            </button>
          )}
        </p>
      )}
      {showEvidence && (
        <ul className="mt-2 space-y-1 text-xs text-zinc-600 dark:text-zinc-400">
          {insight.evidence.items.map((e, i) => (
            <li key={i}>
              {e.label}
              {e.detail ? `: ${e.detail}` : ""}
              {e.draft_id ? <span className="text-zinc-400"> ({e.draft_id})</span> : null}
            </li>
          ))}
        </ul>
      )}

      <div className="mt-3">
        <p className="mb-1 text-xs font-medium text-zinc-500">Proposed change</p>
        {editing ? (
          <div className="space-y-2">
            {insight.patch.ops.map((op, i) => {
              const text = opText(op);
              return text === null ? (
                <p key={i} className="rounded-lg border border-zinc-200 p-2 text-xs dark:border-zinc-800">{describeOp(op)}</p>
              ) : (
                <label key={i} className="block text-xs">
                  <span className="text-zinc-500">{opLabel(op)}</span>
                  <input
                    className={`${input} mt-0.5 w-full`}
                    value={texts[i] ?? text}
                    onChange={(e) => setTexts({ ...texts, [i]: e.target.value })}
                  />
                </label>
              );
            })}
            {draftPatch === null && <p className="text-xs text-red-600 dark:text-red-400">Nothing left to apply. Reject the suggestion instead.</p>}
          </div>
        ) : (
          <>
            <ul className="space-y-1 text-sm">
              {insight.patch.ops.map((op, i) => (
                <li key={i}>{describeOp(op)}</li>
              ))}
            </ul>
            {insight.diff.length > 0 && (
              <div className="mt-2">
                <DiffView diff={insight.diff} label="Preview of the change" />
              </div>
            )}
          </>
        )}
      </div>

      {insight.stale && (
        <p role="status" className="mt-2 text-xs text-amber-700 dark:text-amber-400">
          This change no longer applies cleanly because the rubric or style changed since it was suggested. Reject it and run Reflect again.
        </p>
      )}

      <div className="mt-3 flex flex-wrap gap-2">
        <button type="button" className={btnPrimary} disabled={!canApply} onClick={() => onAccept(insight, changed ? draftPatch : null)}>
          {changed ? "Apply my edit" : "Apply"}
        </button>
        <button
          type="button"
          className={btn}
          disabled={busy}
          onClick={() => {
            setEditing((v) => !v);
            setTexts({});
          }}
        >
          {editing ? "Cancel edit" : "Edit"}
        </button>
        <button type="button" className={btn} disabled={busy} onClick={() => onReject(insight)}>
          Reject
        </button>
      </div>
    </article>
  );
}
