import { useState } from "react";
import { useKillSwitch } from "../components/ui";
import { p3 } from "./api";
import { btn, btnDanger } from "./ui";

/** Always-visible kill switch (SHL-20). Engaging it stops scouting, queued publishes and every browser action. */
export function KillBar() {
  const engaged = useKillSwitch();
  const [busy, setBusy] = useState(false);

  const toggle = async () => {
    setBusy(true);
    try {
      await p3.setKillSwitch(!engaged);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className={`flex items-center justify-between gap-3 border-b px-6 py-2 text-sm ${
        engaged ? "border-red-300 bg-red-50 text-red-900 dark:border-red-900 dark:bg-red-950 dark:text-red-100" : "border-zinc-200 text-zinc-600 dark:border-zinc-800 dark:text-zinc-400"
      }`}
    >
      <span role="status" className="flex items-center gap-2">
        <span aria-hidden="true" className={`h-2 w-2 shrink-0 rounded-full ${engaged ? "bg-red-600" : "bg-emerald-500"}`} />
        {engaged ? (
          <span><strong className="font-semibold">Paused.</strong> Nothing will run on X until you resume.</span>
        ) : (
          "Ready. The app only acts on X when you press a button."
        )}
      </span>
      <button type="button" disabled={busy} onClick={toggle} className={engaged ? btnDanger : btn} aria-pressed={engaged}>
        {engaged ? "Resume" : "Pause everything"}
      </button>
    </div>
  );
}
