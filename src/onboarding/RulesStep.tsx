import { useEffect, useState } from "react";
import { btnGhost, btnPrimary } from "../components/ui";
import { api } from "../lib/api";
import type { RulesNotice, RulesStatus } from "../lib/types";

interface Props {
  onAcknowledged: (status: RulesStatus) => void;
  onBack?: () => void;
  alreadyAcknowledged?: boolean;
  onClose?: () => void;
  closeLabel?: string;
}

function renderNotice(text: string) {
  return text.split("\n").map((line, i) => {
    if (line.startsWith("# ")) return <h2 key={i} className="mb-3 text-xl font-semibold">{line.slice(2)}</h2>;
    if (line.startsWith("- ")) return <li key={i} className="ml-5 list-disc py-1">{line.slice(2)}</li>;
    if (line.trim() === "") return null;
    return <p key={i} className="py-1">{line}</p>;
  });
}

export function RulesStep({ onAcknowledged, onBack, alreadyAcknowledged, onClose, closeLabel = "Close" }: Props) {
  const [notice, setNotice] = useState<RulesNotice | null>(null);
  const [checked, setChecked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api.rulesNotice().then(setNotice).catch((e: Error) => setError(e.message));
  }, []);

  const confirm = async () => {
    if (!notice) return;
    setBusy(true);
    setError(null);
    try {
      onAcknowledged(await api.acknowledgeRules(notice.version, checked));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex-1 overflow-y-auto text-sm leading-relaxed" data-testid="rules-text">
        {notice ? renderNotice(notice.text) : <p>Loading...</p>}
      </div>
      {alreadyAcknowledged ? (
        <div className="mt-4 flex items-center justify-between border-t border-zinc-200 pt-4 dark:border-zinc-800">
          <span className="text-sm text-emerald-600 dark:text-emerald-400">You acknowledged this notice.</span>
          <button type="button" onClick={onClose} className={btnPrimary}>
            {closeLabel}
          </button>
        </div>
      ) : (
        <div className="mt-4 space-y-3 border-t border-zinc-200 pt-4 dark:border-zinc-800">
          <label className="flex items-start gap-2 text-sm">
            <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} className="mt-1" />
            <span>I have read the rules above. I will not spam or post AI slop, and I own every action taken under my account.</span>
          </label>
          {error && <p role="alert" className="text-sm text-red-600 dark:text-red-400">{error}</p>}
          <div className="flex justify-between">
            {onBack ? (
              <button type="button" onClick={onBack} className={btnGhost}>
                Back
              </button>
            ) : <span />}
            <button
              type="button"
              onClick={confirm}
              disabled={!checked || busy}
              className={btnPrimary}
            >
              Confirm and continue
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
