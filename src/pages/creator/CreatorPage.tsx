import { useEffect, useState } from "react";
import { api } from "../../lib/api";
import { FormatsPanel } from "./FormatsPanel";
import { SourcesPanel } from "./SourcesPanel";
import { WritePanel } from "./WritePanel";

type Tab = "write" | "formats" | "sources";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "write", label: "Write" },
  { id: "formats", label: "Formats" },
  { id: "sources", label: "Sources and settings" },
];

export function CreatorPage({ locked }: { locked: boolean }) {
  const [tab, setTab] = useState<Tab>("write");
  const [xRunning, setXRunning] = useState(false);

  useEffect(() => {
    let alive = true;
    const check = () => api.browserInfo().then((i) => alive && setXRunning(i.running)).catch(() => undefined);
    void check();
    const t = setInterval(check, 5000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  return (
    <div className="mx-auto max-w-3xl space-y-4">
      <h1 className="text-xl font-semibold">Creator</h1>
      <div role="tablist" aria-label="Creator sections" className="flex gap-1 border-b border-zinc-200 dark:border-zinc-800">
        {TABS.map((t) => (
          <button
            key={t.id}
            type="button"
            role="tab"
            id={`tab-${t.id}`}
            aria-selected={tab === t.id}
            aria-controls={`panel-${t.id}`}
            onClick={() => setTab(t.id)}
            className={`-mb-px border-b-2 px-3 py-2 text-sm ${tab === t.id ? "border-zinc-900 font-medium dark:border-zinc-100" : "border-transparent text-zinc-500 hover:text-zinc-900 dark:hover:text-zinc-100"}`}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div role="tabpanel" id={`panel-${tab}`} aria-labelledby={`tab-${tab}`}>
        {tab === "write" && <WritePanel locked={locked} xRunning={xRunning} />}
        {tab === "formats" && <FormatsPanel />}
        {tab === "sources" && <SourcesPanel locked={locked} xRunning={xRunning} />}
      </div>
    </div>
  );
}
