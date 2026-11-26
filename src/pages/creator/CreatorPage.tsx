import { useEffect, useState } from "react";
import { PageHeader, Tabs, useKillSwitch } from "../../components/ui";
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

export function CreatorPage({ locked, onGoToSettings }: { locked: boolean; onGoToSettings?: () => void }) {
  const [tab, setTab] = useState<Tab>("write");
  const [xRunning, setXRunning] = useState(false);
  const paused = useKillSwitch();

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
    <div className="mx-auto max-w-4xl space-y-8">
      <PageHeader title="Creator" description="Turn a rough thought into an original post, using formats that have worked for others. You approve every post before it goes to X." />
      <Tabs label="Creator sections" idPrefix="creator" tabs={TABS} active={tab} onChange={setTab} />
      <div role="tabpanel" id={`creator-panel-${tab}`} aria-labelledby={`creator-tab-${tab}`}>
        {tab === "write" && <WritePanel locked={locked} xRunning={xRunning} paused={paused} onGoToSettings={onGoToSettings} />}
        {tab === "formats" && <FormatsPanel />}
        {tab === "sources" && <SourcesPanel locked={locked} xRunning={xRunning} paused={paused} />}
      </div>
    </div>
  );
}
