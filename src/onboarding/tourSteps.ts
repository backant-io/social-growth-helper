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

export type WizardStepKind = "tour" | "preferences" | "rules";

export interface WizardStep {
  kind: WizardStepKind;
  tourIndex?: number;
}

/** Order of the first-run wizard: the tour, then preferences, then the rules acknowledgement last. */
export function wizardOrder(): WizardStep[] {
  return [
    ...TOUR_STEPS.map((_, tourIndex) => ({ kind: "tour" as const, tourIndex })),
    { kind: "preferences" },
    { kind: "rules" },
  ];
}
