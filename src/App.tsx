import { useCallback, useEffect, useRef, useState } from "react";
import { AnalyticsPage } from "./analytics/AnalyticsPage";
import { BrowserStateBanner } from "./components/BrowserStateBanner";
import { EmptyState } from "./components/EmptyState";
import { OfflineBanner } from "./components/OfflineBanner";
import { QuitGuardModal } from "./components/QuitGuardModal";
import { Sidebar } from "./components/Sidebar";
import { SECTIONS } from "./components/sections";
import { learningApi } from "./learning/api";
import { LearningPage } from "./learning/LearningPage";
import { api, listen } from "./lib/api";
import { useOnlineStatus } from "./lib/errors";
import type {
  OnboardingStatus,
  QuitGuardPayload,
  SectionId,
} from "./lib/types";
import { Onboarding, type OnboardingMode } from "./onboarding/Onboarding";
import {
  clearSetupSkipped,
  isLocked,
  isSetupSkipped,
  markSetupSkipped,
} from "./onboarding/setupSkip";
import { CreatorPage } from "./pages/creator/CreatorPage";
import { SettingsPage } from "./pages/SettingsPage";
import { StylePage } from "./pages/StylePage";
import { p3 } from "./phase3/api";
import { ActivityPage } from "./phase3/ActivityPage";
import { CommandPalette, ShortcutHelp } from "./phase3/CommandPalette";
import { matchShortcut, type PaletteCommand } from "./phase3/logic";
import { ReplyStudio } from "./phase3/ReplyStudio";
import { ScoutPage } from "./phase3/ScoutPage";

export default function App() {
  const [section, setSection] = useState<SectionId>("scout");
  const [status, setStatus] = useState<OnboardingStatus | null>(null);
  const [overlay, setOverlay] = useState<OnboardingMode | null>(null);
  const [skipped, setSkipped] = useState(isSetupSkipped);
  const returnTo = useRef<SectionId | null>(null);
  const sectionRef = useRef(section);
  sectionRef.current = section;
  const [quitJobs, setQuitJobs] = useState<string[] | null>(null);
  const online = useOnlineStatus();
  const [proposed, setProposed] = useState(0);
  const [draftId, setDraftId] = useState<string | null>(null);
  const [palette, setPalette] = useState(false);
  const [help, setHelp] = useState(false);

  const refresh = useCallback(async () => {
    setStatus(await api.onboardingStatus());
  }, []);

  useEffect(() => {
    void p3
      .killSwitch()
      .then((on) => (on ? p3.setKillSwitch(false) : undefined))
      .catch(() => undefined);
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
    if (!status || overlay || skipped) return;
    if (!status.tour_completed) setOverlay("full");
    // Re-prompt for the rules only when the notice version changed.
    else if (!status.rules.acknowledged) setOverlay("rules");
  }, [status, overlay, skipped]);

  const closeOverlay = () => {
    setOverlay(null);
    if (returnTo.current) setSection(returnTo.current);
    returnTo.current = null;
  };

  const openTour = () => {
    returnTo.current = sectionRef.current;
    setOverlay("tour");
  };

  const skipSetup = async () => {
    markSetupSkipped();
    setSkipped(true);
    returnTo.current = null;
    setOverlay(null);
    setSection("scout");
    await api.completeTour().catch(() => undefined);
    await refresh();
  };

  const resumeSetup = () => {
    clearSetupSkipped();
    setSkipped(false);
    setOverlay("setup");
  };

  useEffect(() => {
    const offs: Array<Promise<() => void>> = [
      listen<string>("menu-action", (a) => {
        if (a === "replay_tour") openTour();
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
    { id: "help", label: "Show keyboard shortcuts", hint: "?" },
    { id: "tour", label: "Replay the tour" },
  ];

  const runCommand = async (id: string) => {
    if (id.startsWith("go:")) setSection(id.slice(3) as SectionId);
    else if (id === "scout:start") setSection("scout");
    else if (id === "help") setHelp(true);
    else if (id === "tour") openTour();
  };

  const openDraft = (id: string | null) => {
    setDraftId(id);
    if (id) setSection("reply");
  };

  const locked = status ? isLocked(status.rules.acknowledged, skipped) : false;
  const setupIncomplete =
    !!status && !status.rules.acknowledged && skipped && !overlay;
  const active = SECTIONS.find((s) => s.id === section)!;

  return (
    <div className="flex h-full">
      <Sidebar
        active={section}
        onSelect={setSection}
        locked={locked}
        badges={{ learning: proposed }}
      />
      <div className="atmosphere flex min-w-0 flex-1 flex-col">
        <BrowserStateBanner />
        <main
          data-tour="main"
          className="min-w-0 flex-1 overflow-y-auto px-10 py-9"
        >
          <OfflineBanner offline={!online} />
          {section === "settings" ? (
            <SettingsPage
              onResumeSetup={setupIncomplete ? resumeSetup : undefined}
              rules={status?.rules ?? null}
              offline={!online}
              onReplayTour={openTour}
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
          ) : section === "analytics" && !locked ? (
            <AnalyticsPage />
          ) : locked && ["scout", "reply", "activity"].includes(section) ? (
            <EmptyState
              section={active}
              locked={locked}
              onReviewRules={() => setOverlay("rules")}
            />
          ) : section === "scout" ? (
            <ScoutPage onOpenDraft={openDraft} />
          ) : section === "reply" ? (
            <ReplyStudio
              draftId={draftId}
              onOpenDraft={openDraft}
              onGoScout={() => setSection("scout")}
              onGoToSettings={() => setSection("settings")}
            />
          ) : section === "activity" ? (
            <ActivityPage />
          ) : section === "creator" && !locked ? (
            <CreatorPage
              locked={locked}
              onGoToSettings={() => setSection("settings")}
            />
          ) : (
            <EmptyState
              section={active}
              locked={locked}
              onReviewRules={() => setOverlay("rules")}
            />
          )}
        </main>
      </div>
      {palette && (
        <CommandPalette
          commands={commands}
          onRun={(id) => void runCommand(id)}
          onClose={() => setPalette(false)}
        />
      )}
      {help && <ShortcutHelp onClose={() => setHelp(false)} />}
      {overlay && status && (
        <Onboarding
          mode={overlay}
          rulesAcknowledged={status.rules.acknowledged}
          onRulesAcknowledged={() => refresh()}
          onDismiss={
            overlay === "full" || overlay === "setup" ? undefined : closeOverlay
          }
          onSkip={
            overlay === "full" || overlay === "setup"
              ? () => void skipSetup()
              : undefined
          }
          onNavigate={setSection}
          onFinished={async () => {
            const wizard = overlay === "full" || overlay === "setup";
            closeOverlay();
            if (wizard) setSection("scout");
            clearSetupSkipped();
            setSkipped(false);
            await refresh();
          }}
        />
      )}
      {quitJobs && (
        <QuitGuardModal
          jobs={quitJobs}
          onCancel={() => setQuitJobs(null)}
          onQuit={() => void api.quitApp()}
        />
      )}
    </div>
  );
}
