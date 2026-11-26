export type SampleKind = "scout" | "reply" | "publish" | "creator" | "dashboard" | "learning" | "models";

export interface TourStep {
  id: SampleKind;
  title: string;
  body: string;
  /** Phase in which this surface becomes real; until then the tour shows sample data only. */
  phase: number;
}

export const TOUR_STEPS: TourStep[] = [
  {
    id: "scout",
    title: "Scout finds posts worth answering",
    body: "Scout reads your own home feed in a dedicated Chrome window and ranks posts for fit, freshness and reply headroom. It only reads; nothing is posted.",
    phase: 3,
  },
  {
    id: "reply",
    title: "Reply Studio drafts in your voice",
    body: "Pick a post and get a few drafts shaped by your style guide. Every post has a six-action bar: reply, quote, like, repost, bookmark and follow. Each one is a click you make.",
    phase: 4,
  },
  {
    id: "publish",
    title: "Nothing goes out without you",
    body: "Publishing shows exactly what will be sent and waits for your confirmation. An audit log records what you sent and when.",
    phase: 4,
  },
  {
    id: "creator",
    title: "Creator turns your ideas into original posts",
    body: "Start from a topic or a viral format, write with help, and keep only what still sounds like you.",
    phase: 5,
  },
  {
    id: "dashboard",
    title: "Analytics shows what actually worked",
    body: "A dashboard of your own numbers: impressions, replies, follows gained, and which formats and times performed.",
    phase: 6,
  },
  {
    id: "learning",
    title: "Learning keeps the loop honest",
    body: "Weekly insights compare your drafts with what you posted and what performed, then suggest changes to your rubric. You approve every change.",
    phase: 7,
  },
  {
    id: "models",
    title: "You choose the model for each task",
    body: "Seven tasks, each with its own model. Keys live in your OS keychain and are never written to disk in plain text.",
    phase: 2,
  },
];

export type WizardStepKind = "tour" | "preferences" | "browser" | "providers" | "style" | "routing" | "rules";

export interface WizardStep {
  kind: WizardStepKind;
  tourIndex?: number;
}

/** Order of the first-run wizard: the tour, preferences, guided setup (Chrome, providers, style, routing), then the rules acknowledgement last. */
export function wizardOrder(): WizardStep[] {
  return [
    ...TOUR_STEPS.map((_, tourIndex) => ({ kind: "tour" as const, tourIndex })),
    { kind: "preferences" },
    { kind: "browser" },
    { kind: "providers" },
    { kind: "style" },
    { kind: "routing" },
    { kind: "rules" },
  ];
}

export const SETUP_STEP_COPY: Record<"browser" | "providers" | "style" | "routing", { title: string; intro: string }> = {
  browser: {
    title: "Set up the X browser window",
    intro:
      "Scout reads X in your own Chrome, in a dedicated window with a separate profile. Check that Chrome is detected here. Logging in to X happens in that window; the Open X window button unlocks once you acknowledge the rules on the last step, and it stays available in Settings under General & Browser.",
  },
  providers: {
    title: "Connect an AI provider",
    intro: "Pick at least one provider so drafts and scoring can run. Keys go to your OS keychain. You can skip this and connect later in Settings under AI Providers.",
  },
  style: {
    title: "Write your style guide",
    intro: "A few sentences about how you write. Drafts follow this guide. Examples, banned phrases and the test bench live in the Style section.",
  },
  routing: {
    title: "Choose a model for each task",
    intro: "Each task can use a different provider and model. You can change this any time in Settings under Task Routing.",
  },
};

export interface SetupHint {
  done: boolean;
  text: string;
}

export function providerHint(providers: Array<{ status: string }>): SetupHint {
  const n = providers.filter((p) => p.status === "connected").length;
  return n > 0
    ? { done: true, text: `${n} provider${n === 1 ? "" : "s"} connected.` }
    : { done: false, text: "No provider connected yet." };
}

export function routingHint(routes: Array<{ provider_account_id: string | null; model_id: string | null }>): SetupHint {
  const set = routes.filter((r) => r.provider_account_id && r.model_id).length;
  return set > 0
    ? { done: true, text: `${set} of ${routes.length} tasks have a model.` }
    : { done: false, text: "No task has a model yet. Unset tasks ask you to choose when first used." };
}

export function browserHint(info: { install: unknown | null; running: boolean } | null, rulesAcknowledged: boolean): SetupHint {
  if (!info) return { done: false, text: "Checking for Chrome..." };
  if (!info.install) return { done: false, text: "No Chrome, Chromium or Brave found. Install one or set its path below." };
  if (info.running) return { done: true, text: "X window is open. Log in to X there once; the login is kept." };
  return rulesAcknowledged
    ? { done: true, text: "Chrome found. Open the X window and log in to X once." }
    : { done: true, text: "Chrome found. Logging in unlocks after you acknowledge the rules on the last step." };
}
