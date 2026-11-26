import { useState } from "react";
import { blankBlueprint, syncSlots } from "../../lib/creator";
import type { FormatBlueprint } from "../../lib/types";
import { btn, danger, field, Modal, muted, primary } from "./ui";

interface Props {
  initial: FormatBlueprint | null;
  onSave: (bp: FormatBlueprint) => Promise<void>;
  onCancel: () => void;
}

export function FormatEditor({ initial, onSave, onCancel }: Props) {
  const [bp, setBp] = useState<FormatBlueprint>(initial ?? blankBlueprint());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<FormatBlueprint>) => setBp((cur) => ({ ...cur, ...patch }));

  async function save() {
    setBusy(true);
    setError(null);
    try {
      await onSave({ ...bp, slots: syncSlots(bp.skeleton, bp.slots) });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  }

  const slots = syncSlots(bp.skeleton, bp.slots);

  return (
    <Modal
      title={initial ? "Edit format" : "New format"}
      onClose={onCancel}
      actions={
        <>
          <button type="button" className={btn} onClick={onCancel}>Cancel</button>
          <button type="button" className={primary} disabled={busy} onClick={save}>Save format</button>
        </>
      }
    >
      <div className="space-y-3 text-sm">
        <label className="block">
          <span className="mb-1 block font-medium">Name</span>
          <input className={field} value={bp.name} onChange={(e) => set({ name: e.target.value })} />
        </label>
        <label className="block">
          <span className="mb-1 block font-medium">Hook formula</span>
          <input className={field} value={bp.hook_formula} onChange={(e) => set({ hook_formula: e.target.value })} placeholder="How the first line grabs attention" />
        </label>
        <label className="block">
          <span className="mb-1 block font-medium">Structure</span>
          <input className={field} value={bp.structural_pattern} onChange={(e) => set({ structural_pattern: e.target.value })} placeholder="Hook, then reason, then reframe" />
        </label>
        <label className="block">
          <span className="mb-1 block font-medium">Skeleton</span>
          <textarea className={`${field} min-h-24 font-mono text-xs`} value={bp.skeleton} onChange={(e) => set({ skeleton: e.target.value })} placeholder={"Unpopular opinion: {belief} is wrong.\n\n{reason}"} />
          <span className={`mt-1 block text-xs ${muted}`}>Put each part that changes between posts in {"{curly_braces}"}. Keep the topic out of the skeleton.</span>
        </label>
        {slots.length > 0 && (
          <fieldset className="space-y-2">
            <legend className="mb-1 font-medium">Slots</legend>
            {slots.map((s) => (
              <div key={s.key} className="grid grid-cols-[7rem_1fr] items-center gap-2">
                <code className="text-xs">{`{${s.key}}`}</code>
                <input
                  aria-label={`Label for ${s.key}`}
                  className={field}
                  value={s.label}
                  onChange={(e) => set({ slots: slots.map((x) => (x.key === s.key ? { ...x, label: e.target.value } : x)) })}
                />
              </div>
            ))}
          </fieldset>
        )}
        <label className="block">
          <span className="mb-1 block font-medium">Works well with (optional)</span>
          <input className={field} value={bp.media_hint} onChange={(e) => set({ media_hint: e.target.value })} placeholder="e.g. a screenshot, a chart" />
        </label>
        {error && <p role="alert" className={danger}>{error}</p>}
      </div>
    </Modal>
  );
}
