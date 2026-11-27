import { useEffect, useState } from "react";
import { Page, PageHeader, Tabs } from "../../components/ui";
import { api } from "../../lib/api";
import { FormatsPanel } from "./FormatsPanel";
import { SourcesPanel } from "./SourcesPanel";
import { WritePanel } from "./WritePanel";

type Tab = "write" | "formats" | "sources";

const TABS: Array<{ id: Tab; label: string }> = [
  { id: "write", label: "Write" },
  { id: "formats", label: "Formats" },
  { id: "sources", label: "Sources" },
];

export function CreatorPage({ locked, onGoToSettings }: { locked: boolean; onGoToSettings?: () => void }) {
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
    <Page>
      <PageHeader title="Creator" description="Turn a thought into a post." />
      <Tabs label="Creator sections" idPrefix="creator" tabs={TABS} active={tab} onChange={setTab} />
      <div role="tabpanel" id={`creator-panel-${tab}`} aria-labelledby={`creator-tab-${tab}`}>
        {tab === "write" && <WritePanel locked={locked} xRunning={xRunning} onGoToSettings={onGoToSettings} />}
        {tab === "formats" && <FormatsPanel />}
        {tab === "sources" && <SourcesPanel locked={locked} xRunning={xRunning} />}
      </div>
    </Page>
  );
}
