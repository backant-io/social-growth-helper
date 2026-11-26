import { useEffect, useState } from "react";
import { btn, btnGhost, btnPrimary } from "../components/ui";
import { api } from "../lib/api";
import type { RulesStatus, SectionId } from "../lib/types";
import { PreferencesStep } from "./PreferencesStep";
import { RulesStep } from "./RulesStep";
import { SetupStep } from "./SetupSteps";
import { Spotlight } from "./Spotlight";
import { TOUR_STEPS, wizardOrder } from "./tourSteps";

/** full: first run (welcome, tour, setup). tour: replay. setup: resume setup after skipping. rules: read the rules. */
export type OnboardingMode = "full" | "tour" | "rules" | "setup";

interface Props {
  mode: OnboardingMode;
  rulesAcknowledged: boolean;
  onFinished: (status?: RulesStatus) => void;
  onRulesAcknowledged?: (status: RulesStatus) => void | Promise<void>;
  onDismiss?: () => void;
  /** Leaves setup unfinished and opens the app. Shown on every first-run and resume screen. */
  onSkip?: () => void;
  onNavigate?: (section: SectionId) => void;
}

function Welcome({ onStart, onSkip }: { onStart: () => void; onSkip?: () => void }) {
  return (
    <div className="atmosphere fixed inset-0 z-40 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-label="Welcome">
      <div className="w-full max-w-xl">
        <span className="eyebrow">Social Growth Helper</span>
        <h1 className="mt-3 text-[2.75rem] leading-[1.05] font-semibold tracking-tight">Grow on X in your own voice.</h1>
        <p className="mt-4 max-w-md text-[0.95rem] leading-relaxed text-zinc-600 dark:text-zinc-400">
          A local tool that finds posts worth answering and helps you draft. You read, you decide, you click. Nothing is posted for you.
        </p>
        <div className="mt-8 flex flex-wrap items-center gap-3">
          <button type="button" onClick={onStart} className={`${btnPrimary} px-5 py-2.5`} autoFocus>
            Take the 1 minute tour
          </button>
          {onSkip && (
            <button type="button" onClick={onSkip} className={`${btn} px-5 py-2.5`} data-testid="skip-setup">
              Skip setup and explore
            </button>
          )}
        </div>
        <p className="mt-4 text-xs text-zinc-500">Skipping keeps everything that touches X switched off. You can finish setup any time from the banner at the top.</p>
      </div>
    </div>
  );
}

export function Onboarding({ mode, rulesAcknowledged, onFinished, onRulesAcknowledged, onDismiss, onSkip, onNavigate }: Props) {
  const [steps] = useState(() => {
    if (mode === "rules") return [{ kind: "rules" as const }];
    if (mode === "tour") return wizardOrder().filter((s) => s.kind === "tour");
    if (mode === "setup") return wizardOrder(rulesAcknowledged).filter((s) => s.kind !== "tour");
    return wizardOrder(rulesAcknowledged);
  });
  const [welcomed, setWelcomed] = useState(mode !== "full");
  const [index, setIndex] = useState(0);
  const step = steps[index];
  const wizard = mode === "full" || mode === "setup";
  const back = () => setIndex((i) => Math.max(0, i - 1));
  const next = () => setIndex((i) => Math.min(steps.length - 1, i + 1));
  const tourStep = step.kind === "tour" && step.tourIndex !== undefined ? TOUR_STEPS[step.tourIndex] : null;

  useEffect(() => {
    if (welcomed && tourStep) onNavigate?.(tourStep.section);
  }, [welcomed, tourStep, onNavigate]);

  const finishTour = async () => {
    await api.completeTour();
    onFinished();
  };
  const isLast = index === steps.length - 1;
  const tourCount = steps.filter((s) => s.kind === "tour").length;

  if (!welcomed) return <Welcome onStart={() => setWelcomed(true)} onSkip={onSkip} />;

  if (tourStep) {
    return (
      <Spotlight
        target={tourStep.target}
        title={tourStep.title}
        body={tourStep.body}
        index={index}
        total={tourCount}
        onBack={back}
        onNext={mode === "tour" && isLast ? () => void finishTour() : next}
        onSkip={mode === "tour" ? onDismiss : onSkip}
        skipLabel={mode === "tour" ? "Close" : "Skip setup"}
        nextLabel={mode === "tour" && isLast ? "Done" : index === tourCount - 1 ? "Continue to setup" : undefined}
      />
    );
  }

  return (
    <div className="atmosphere fixed inset-0 z-40 flex items-center justify-center p-6" role="dialog" aria-modal="true" aria-label="Welcome">
      <div className="flex h-[min(660px,92vh)] w-full max-w-2xl flex-col">
        <div className="mb-5 flex items-center justify-between">
          <div className="eyebrow">
            {wizard ? "Setup" : "Rules"} / Step {index - tourCount + 1} of {steps.length - tourCount}
          </div>
          <div className="flex items-center gap-1">
            {wizard && onSkip && (
              <button type="button" onClick={onSkip} className={btnGhost} data-testid="skip-setup">
                Skip setup
              </button>
            )}
            {onDismiss && (
              <button type="button" onClick={onDismiss} className={btnGhost}>
                Close
              </button>
            )}
          </div>
        </div>
        <div className="min-h-0 flex-1">
          {step.kind === "preferences" && (
            <div className="flex h-full flex-col">
              <h2 className="mb-3 text-[1.75rem] leading-tight font-semibold">What do you want to be known for?</h2>
              <div className="min-h-0 flex-1">
                <PreferencesStep onBack={back} onSaved={next} />
              </div>
            </div>
          )}
          {(step.kind === "browser" || step.kind === "providers" || step.kind === "style" || step.kind === "routing") && (
            <SetupStep kind={step.kind} rulesAcknowledged={rulesAcknowledged} onBack={back} onNext={wizard && isLast ? () => void finishTour() : next} finish={wizard && isLast} />
          )}
          {step.kind === "rules" && (
            <RulesStep
              alreadyAcknowledged={rulesAcknowledged}
              onBack={wizard && index > 0 ? back : undefined}
              onClose={wizard ? next : onDismiss}
              closeLabel={wizard ? "Continue" : "Close"}
              onAcknowledged={async (status) => {
                if (wizard) {
                  await onRulesAcknowledged?.(status);
                  next();
                } else {
                  onFinished(status);
                }
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
