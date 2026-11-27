import type { SectionId } from "../lib/types";

export type SampleKind = "scout" | "reply" | "publish" | "creator" | "dashboard" | "learning" | "models";

export interface TourStep {
  id: SampleKind;
  title: string;
  body: string;
  /** Phase in which this surface becomes real; until then the tour shows sample data only. */
  phase: number;
  /** `data-tour` value of the UI element the coach mark points at. */
  target: string;
  /** Screen shown behind the coach mark while this step is active. */
  section: SectionId;
}

export const TOUR_STEPS: TourStep[] = [
  {
    id: "scout",
    title: "Scout finds posts worth answering",
    body: "Scout reads your own home feed in a dedicated Chrome window and ranks posts for fit, freshness and reply headroom. It only reads; nothing is posted.",
    phase: 3,
    target: "nav-scout",
    section: "scout",
  },
  {
    id: "reply",
    title: "Replies are drafted inline in Scout",
    body: "Each recommended post shows its images and videos alongside an editable AI reply in your voice. You can reply, quote, like, repost, bookmark or follow—every action is a click you make.",
    phase: 4,
    target: "nav-scout",
    section: "scout",
  },
  {
    id: "publish",
    title: "Nothing goes out without you",
    body: "Pressing Send reply dispatches your exact edited text in the background browser and records what you sent in Activity.",
    phase: 4,
    target: "main",
    section: "scout",
  },
  {
    id: "creator",
    title: "Creator turns your ideas into original posts",
    body: "Start from a topic or a viral format, write with help, and keep only what still sounds like you.",
    phase: 5,
    target: "nav-creator",
    section: "creator",
  },
  {
    id: "dashboard",
    title: "Analytics shows what actually worked",
    body: "A dashboard of your own numbers: impressions, replies, follows gained, and which formats and times performed.",
    phase: 6,
    target: "nav-analytics",
    section: "analytics",
  },
  {
    id: "learning",
    title: "Learning keeps the loop honest",
    body: "Weekly insights compare your drafts with what you posted and what performed, then suggest changes to your rubric. You approve every change.",
    phase: 7,
    target: "nav-learning",
    section: "learning",
  },
  {
    id: "models",
    title: "You choose the model for each task",
    body: "Seven tasks, each with its own model. Keys live in your OS keychain and are never written to disk in plain text.",
    phase: 2,
    target: "nav-settings",
    section: "settings",
  },
];

export type WizardStepKind = "tour" | "preferences" | "browser" | "providers" | "style" | "rules";

export interface WizardStep {
  kind: WizardStepKind;
  tourIndex?: number;
}

/**
 * Order of the first-run wizard: the tour, the rules acknowledgement, then preferences and guided
 * setup (Chrome and X login, one AI connection, style). The acknowledgement comes before setup so
 * the X window can be opened for the login. Pass `rulesAcknowledged` to skip a step already done.
 */
export function wizardOrder(rulesAcknowledged = false): WizardStep[] {
  return [
    ...TOUR_STEPS.map((_, tourIndex) => ({ kind: "tour" as const, tourIndex })),
    ...(rulesAcknowledged ? [] : [{ kind: "rules" as const }]),
    { kind: "preferences" },
    { kind: "browser" },
    { kind: "providers" },
    { kind: "style" },
  ];
}

export const SETUP_STEP_COPY: Record<"browser" | "providers" | "style", { title: string; intro: string }> = {
  browser: {
    title: "Set up the X browser window",
    intro:
      "Scout reads X in your own Chrome, in a dedicated window with a separate profile. Check that Chrome is detected here. Open the X window below and log in to X there once; the login is kept. The button stays available in Settings under General.",
  },
  providers: {
    title: "Connect an AI",
    intro: "Pick one. A sensible model is chosen for every task automatically, and API providers list their live models. Keys go to your OS keychain. You can do this later in Settings under AI models.",
  },
  style: {
    title: "Write your style guide",
    intro: "A few sentences about how you write. Drafts follow this guide. Examples, banned phrases and the test bench live in the Style section.",
  },
};

export interface SetupHint {
  done: boolean;
  text: string;
}

export function providerHint(providers: Array<{ status: string }>): SetupHint {
  const n = providers.filter((p) => p.status === "connected").length;
  return n > 0
    ? { done: true, text: `${n} provider${n === 1 ? "" : "s"} connected. Default models are set.` }
    : { done: false, text: "Nothing connected yet. You can skip and do this later." };
}

export function browserHint(info: { install: unknown | null; running: boolean } | null, rulesAcknowledged: boolean): SetupHint {
  if (!info) return { done: false, text: "Checking for Chrome..." };
  if (!info.install) return { done: false, text: "No Chrome, Chromium or Brave found. Install one or set its path below." };
  if (info.running) return { done: true, text: "X window is open. Log in to X there once; the login is kept." };
  return rulesAcknowledged
    ? { done: true, text: "Chrome found. Open the X window and log in to X once, or skip and do it later in Settings." }
    : { done: true, text: "Chrome found. Logging in unlocks after you acknowledge the rules." };
}
