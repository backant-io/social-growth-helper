import { useState } from "react";
import { api } from "../lib/api";
import type { RulesStatus } from "../lib/types";
import { PreferencesStep } from "./PreferencesStep";
import { RulesStep } from "./RulesStep";
import { SampleCard } from "./SampleCard";
import { TOUR_STEPS, wizardOrder } from "./tourSteps";

export type OnboardingMode = "full" | "tour" | "rules";

interface Props {
  mode: OnboardingMode;
  rulesAcknowledged: boolean;
  onFinished: (status?: RulesStatus) => void;
  onDismiss?: () => void;
}

export function Onboarding({ mode, rulesAcknowledged, onFinished, onDismiss }: Props) {
  const steps = mode === "rules" ? [{ kind: "rules" as const }] : mode === "tour" ? wizardOrder().filter((s) => s.kind === "tour") : wizardOrder();
  const [index, setIndex] = useState(0);
  const step = steps[index];
  const back = () => setIndex((i) => Math.max(0, i - 1));
  const next = () => setIndex((i) => Math.min(steps.length - 1, i + 1));

  const finishTour = async () => {
    await api.completeTour();
    onFinished();
  };

  return (
    <div className="fixed inset-0 z-40 flex items-center justify-center bg-black/40 p-6" role="dialog" aria-modal="true" aria-label="Welcome">
      <div className="flex h-[min(640px,90vh)] w-full max-w-2xl flex-col rounded-2xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-800 dark:bg-zinc-950">
        <div className="mb-4 flex items-center justify-between">
          <div className="text-xs text-zinc-500">
            Step {index + 1} of {steps.length}
          </div>
          {onDismiss && (
            <button type="button" onClick={onDismiss} className="text-xs text-zinc-500 hover:underline">
              Close
            </button>
          )}
        </div>
        <div className="min-h-0 flex-1">
          {step.kind === "tour" && step.tourIndex !== undefined && (
            <div className="flex h-full flex-col">
              <div className="flex-1 space-y-4 overflow-y-auto">
                <h2 className="text-xl font-semibold">{TOUR_STEPS[step.tourIndex].title}</h2>
                <p className="text-sm leading-relaxed text-zinc-600 dark:text-zinc-400">{TOUR_STEPS[step.tourIndex].body}</p>
                <SampleCard kind={TOUR_STEPS[step.tourIndex].id} />
                <p className="text-xs text-zinc-500">
                  This is a sample. No live X action and no model call happens during the tour.
                </p>
              </div>
              <div className="mt-4 flex justify-between border-t border-zinc-200 pt-4 dark:border-zinc-800">
                <button type="button" onClick={back} disabled={index === 0} className="rounded-lg px-4 py-2 text-sm text-zinc-600 disabled:opacity-30 dark:text-zinc-400">
                  Back
                </button>
                {mode === "tour" && index === steps.length - 1 ? (
                  <button type="button" onClick={finishTour} className="rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900">
                    Done
                  </button>
                ) : (
                  <button type="button" onClick={next} className="rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white dark:bg-zinc-100 dark:text-zinc-900">
                    Next
                  </button>
                )}
              </div>
            </div>
          )}
          {step.kind === "preferences" && (
            <div className="flex h-full flex-col">
              <h2 className="mb-3 text-xl font-semibold">What do you want to be known for?</h2>
              <div className="min-h-0 flex-1">
                <PreferencesStep onBack={back} onSaved={next} />
              </div>
            </div>
          )}
          {step.kind === "rules" && (
            <RulesStep
              alreadyAcknowledged={rulesAcknowledged}
              onBack={mode === "full" ? back : undefined}
              onClose={onDismiss}
              onAcknowledged={async (status) => {
                if (mode === "full") await api.completeTour();
                onFinished(status);
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
