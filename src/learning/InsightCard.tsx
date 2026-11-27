import { useState } from "react";
import { DiffView } from "./DiffView";
import { describeOp, editedPatch, opLabel, opText, patchesEqual } from "./model";
import type { Insight } from "./types";
import { btnGhost, btnPrimary, btn, input, panel } from "./ui";

interface Props {
  insight: Insight;
  busy: boolean;
  onAccept: (insight: Insight, edited: Insight["patch"] | null) => void;
  onReject: (insight: Insight) => void;
}

export function InsightCard({ insight, busy, onAccept, onReject }: Props) {
  const [editing, setEditing] = useState(false);
  const [texts, setTexts] = useState<Record<number, string>>({});

  const draftPatch = editing ? editedPatch(insight.patch, texts) : insight.patch;
  const changed = draftPatch ? !patchesEqual(draftPatch, insight.patch) : false;
  const canApply = !busy && !insight.stale && draftPatch !== null;

  return (
    <article className={panel} aria-label={insight.hypothesis}>
      <p className="font-display text-base leading-snug font-semibold tracking-tight">{insight.hypothesis}</p>

      <div className="mt-3">
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
        <button type="button" className={btnGhost} disabled={busy} onClick={() => onReject(insight)}>
          Reject
        </button>
      </div>
    </article>
  );
}
