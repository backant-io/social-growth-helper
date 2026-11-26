import { useEffect, useState } from "react";
import { listen } from "../lib/api";
import { p3 } from "./api";
import { btn, btnDanger } from "./ui";

/** Always-visible kill switch (SHL-20). Engaging it stops scouting, queued publishes and every browser action. */
export function KillBar() {
  const [engaged, setEngaged] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    void p3.killSwitch().then(setEngaged);
    const off = listen<boolean>("kill:changed", setEngaged);
    return () => {
      void off.then((f) => f());
    };
  }, []);

  const toggle = async () => {
    setBusy(true);
    try {
      setEngaged(await p3.setKillSwitch(!engaged));
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
      <span role="status">{engaged ? "Paused. Nothing will run on X until you resume." : "The app only acts on X when you press a button."}</span>
      <button type="button" disabled={busy} onClick={toggle} className={engaged ? btn : btnDanger} aria-pressed={engaged}>
        {engaged ? "Resume" : "Pause everything"}
      </button>
    </div>
  );
}
