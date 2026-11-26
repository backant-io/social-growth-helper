import { useCallback, useEffect, useState } from "react";
import { AnalyticsPage } from "./analytics/AnalyticsPage";
import { EmptyState } from "./components/EmptyState";
import { OfflineBanner } from "./components/OfflineBanner";
import { QuitGuardModal } from "./components/QuitGuardModal";
import { Sidebar } from "./components/Sidebar";
import { SECTIONS } from "./components/sections";
import { api, listen } from "./lib/api";
import { useOnlineStatus } from "./lib/errors";
import type { OnboardingStatus, QuitGuardPayload, SectionId } from "./lib/types";
import { Onboarding, type OnboardingMode } from "./onboarding/Onboarding";
import { SettingsPage } from "./pages/SettingsPage";
import { StylePage } from "./pages/StylePage";
import { p3 } from "./phase3/api";
import { ActivityPage } from "./phase3/ActivityPage";
import { CommandPalette, ShortcutHelp } from "./phase3/CommandPalette";
import { KillBar } from "./phase3/KillBar";
import { matchShortcut, type PaletteCommand } from "./phase3/logic";
import { PublishSettings } from "./phase3/PublishSettings";
import { ReplyStudio } from "./phase3/ReplyStudio";
import { ScoutPage } from "./phase3/ScoutPage";

export default function App() {
  const [section, setSection] = useState<SectionId>("scout");
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [overlay, setOverlay] = useState<OnboardingMode | null>(null);
  const [quitJobs, setQuitJobs] = useState<string[] | null>(null);
  const online = useOnlineStatus();
  const [draftId, setDraftId] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [help, setHelp] = useState(false);

  const refresh = useCallback(async () => {
    setStatus(await api.onboardingStatus());
  }, []);

  useEffect(() => {
    refresh().then(() => undefined);
  }, [refresh]);

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
      listen<SectionId>("nav:go", (id) => setSection(id)),
    ];
    return () => {
      offs.forEach((p) => p.then((off) => off()));
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const a = matchShortcut(e);
      if (a === "palette") {
        e.preventDefault();
        setPalette((v) => !v);
      } else if (a === "help") {
        e.preventDefault();
        setHelp(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const commands: PaletteCommand[] = [
    ...SECTIONS.map((s) => ({ id: `go:${s.id}`, label: `Go to ${s.label}` })),
    { id: "scout:start", label: "Start a scout session", hint: "Scout Queue" },
    { id: "kill:toggle", label: "Pause or resume everything" },
    { id: "help", label: "Show keyboard shortcuts", hint: "?" },
    { id: "tour", label: "Replay the tour" },
  ];

  const runCommand = async (id: string) => {
    if (id.startsWith("go:")) setSection(id.slice(3) as SectionId);
    else if (id === "scout:start") setSection("scout");
    else if (id === "kill:toggle") await p3.setKillSwitch(!(await p3.killSwitch()));
    else if (id === "help") setHelp(true);
    else if (id === "tour") setOverlay("tour");
  };

  const openDraft = (id: string | null) => {
    setDraftId(id);
    if (id) setSection("reply");
  };

  const locked = status ? !status.rules.acknowledged : false;
  const active = SECTIONS.find((s) => s.id === section)!;

  return (
    <div className="flex h-full">
      <Sidebar active={section} onSelect={setSection} locked={locked} />
      <div className="flex min-w-0 flex-1 flex-col">
      <KillBar />
      <main className="min-w-0 flex-1 overflow-y-auto p-8">
        <OfflineBanner offline={!online} />
        {section === "settings" ? (
          <>
            <SettingsPage
              rules={status?.rules ?? null}
              offline={!online}
              onReplayTour={() => setOverlay("tour")}
              onReadRules={() => setOverlay("rules")}
            />
            <div className="mx-auto mt-6 max-w-3xl">
              <PublishSettings />
            </div>
          </>
        ) : section === "style" ? (
          <StylePage
            locked={locked}
            offline={!online}
            onReviewRules={() => setOverlay("rules")}
            onOpenSettings={() => setSection("settings")}
          />
        ) : section === "analytics" && !locked ? (
          <AnalyticsPage />
        ) : locked && ["scout", "reply", "activity"].includes(section) ? (
          <EmptyState section={active} locked={locked} onReviewRules={() => setOverlay("rules")} />
        ) : section === "scout" ? (
          <ScoutPage onOpenDraft={openDraft} />
        ) : section === "reply" ? (
          <ReplyStudio draftId={draftId} onOpenDraft={openDraft} onGoScout={() => setSection("scout")} />
        ) : section === "activity" ? (
          <ActivityPage />
        ) : (
          <EmptyState section={active} locked={locked} onReviewRules={() => setOverlay("rules")} />
        )}
      </main>
      </div>
      {palette && <CommandPalette commands={commands} onRun={(id) => void runCommand(id)} onClose={() => setPalette(false)} />}
      {help && <ShortcutHelp onClose={() => setHelp(false)} />}
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
