import { useCallback, useEffect, useState } from "react";
import { EmptyState } from "./components/EmptyState";
import { OfflineBanner } from "./components/OfflineBanner";
import { QuitGuardModal } from "./components/QuitGuardModal";
import { Sidebar } from "./components/Sidebar";
import { SECTIONS } from "./components/sections";
import { learningApi } from "./learning/api";
import { LearningPage } from "./learning/LearningPage";
import { api, listen } from "./lib/api";
import { useOnlineStatus } from "./lib/errors";
import type { OnboardingStatus, QuitGuardPayload, SectionId } from "./lib/types";
import { Onboarding, type OnboardingMode } from "./onboarding/Onboarding";
import { SettingsPage } from "./pages/SettingsPage";
import { StylePage } from "./pages/StylePage";

export default function App() {
  const [section, setSection] = useState<SectionId>("scout");
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [overlay, setOverlay] = useState<OnboardingMode | null>(null);
  const [quitJobs, setQuitJobs] = useState<string[] | null>(null);
  const online = useOnlineStatus();
  const [proposed, setProposed] = useState(0);

  const refresh = useCallback(async () => {
    setStatus(await api.onboardingStatus());
  }, []);

  useEffect(() => {
    refresh().then(() => undefined);
  }, [refresh]);

  const acknowledged = status?.rules.acknowledged ?? false;
  useEffect(() => {
    if (!acknowledged) return;
    learningApi
      .reflectIfDue()
      .catch(() => null)
      .then(() => learningApi.proposedCount())
      .then(setProposed)
      .catch(() => undefined);
  }, [acknowledged]);

  useEffect(() => {
    if (status && !status.tour_completed && !overlay) setOverlay("full");
    // Re-prompt for the rules only when the notice version changed.
    else if (status && status.tour_completed && !status.rules.acknowledged && !overlay) setOverlay("rules");
  }, [status, overlay]);

  useEffect(() => {
    const offs: Array<Promise<() => void>> = [
      listen<string>("menu-action", (a) => {
        if (a === "replay_tour") setOverlay("tour");
        if (a === "read_rules") setOverlay("rules");
      }),
      listen<QuitGuardPayload>("quit-guard", (p) => {
        if (p.kind === "needs_confirmation") setQuitJobs(p.jobs);
      }),
    ];
    return () => {
      offs.forEach((p) => p.then((off) => off()));
    };
  }, []);

  const locked = status ? !status.rules.acknowledged : false;
  const active = SECTIONS.find((s) => s.id === section)!;

  return (
    <div className="flex h-full">
      <Sidebar active={section} onSelect={setSection} locked={locked} badges={{ learning: proposed }} />
      <main className="min-w-0 flex-1 overflow-y-auto p-8">
        <OfflineBanner offline={!online} />
        {section === "settings" ? (
          <SettingsPage
            rules={status?.rules ?? null}
            offline={!online}
            onReplayTour={() => setOverlay("tour")}
            onReadRules={() => setOverlay("rules")}
            onDataChanged={() => {
              setProposed(0);
              void refresh();
            }}
          />
        ) : section === "learning" && !locked ? (
          <LearningPage onProposedChange={setProposed} />
        ) : section === "style" ? (
          <StylePage
            locked={locked}
            offline={!online}
            onReviewRules={() => setOverlay("rules")}
            onOpenSettings={() => setSection("settings")}
          />
        ) : (
          <EmptyState section={active} locked={locked} onReviewRules={() => setOverlay("rules")} />
        )}
      </main>
      {overlay && status && (
        <Onboarding
          mode={overlay}
          rulesAcknowledged={status.rules.acknowledged}
          onDismiss={overlay === "full" ? undefined : () => setOverlay(null)}
          onFinished={async () => {
            setOverlay(null);
            await refresh();
          }}
        />
      )}
      {quitJobs && <QuitGuardModal jobs={quitJobs} onCancel={() => setQuitJobs(null)} onQuit={() => void api.quitApp()} />}
    </div>
  );
}
