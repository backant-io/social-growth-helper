export type BrowserState = "closed" | "crashed" | "ready" | "logged_out" | "challenge" | "suspended" | "locked";

export interface BrowserStatus {
  state: BrowserState;
  message: string;
  /** The job a halt stopped: "scout" or "publish". */
  job: "scout" | "publish" | null;
  can_restart_job: boolean;
}

export type BannerAction = "focus" | "resume" | "abort" | "relaunch";

export interface Banner {
  tone: "warn" | "error" | "info";
  title: string;
  body: string;
  actions: BannerAction[];
  resumeLabel: string;
}

const TITLES: Record<Exclude<BrowserState, "closed" | "ready">, string> = {
  logged_out: "Logged out of X",
  challenge: "X needs a human check",
  suspended: "X account suspended",
  locked: "X account locked",
  crashed: "The X window closed unexpectedly",
};

function resumeLabel(s: BrowserStatus): string {
  if (s.job === "scout" && s.can_restart_job) return "Resume Scout";
  return s.job ? "Resume" : "Re-check";
}

/** What the persistent browser banner shows for a status (BRW-06, BRW-08, BRW-11). `null` means nothing to show. */
export function bannerFor(s: BrowserStatus | null): Banner | null {
  if (!s) return null;
  if (s.state === "closed") return null;
  if (s.state === "ready") {
    if (!s.can_restart_job) return null;
    return {
      tone: "info",
      title: "X is ready again",
      body: "Scout stopped while X needed you. Resume it with the same settings, or leave it stopped.",
      actions: ["resume", "abort"],
      resumeLabel: resumeLabel(s),
    };
  }
  if (s.state === "crashed") {
    return { tone: "error", title: TITLES.crashed, body: s.message, actions: s.job ? ["relaunch", "abort"] : ["relaunch"], resumeLabel: resumeLabel(s) };
  }
  return {
    tone: s.state === "suspended" ? "error" : "warn",
    title: TITLES[s.state],
    body: s.message,
    actions: s.job ? ["focus", "resume", "abort"] : ["focus", "resume"],
    resumeLabel: resumeLabel(s),
  };
}

/** Same banner content means no re-render churn when a poll returns an unchanged status. */
export function sameStatus(a: BrowserStatus | null, b: BrowserStatus | null): boolean {
  return a === b || (!!a && !!b && a.state === b.state && a.job === b.job && a.can_restart_job === b.can_restart_job && a.message === b.message);
}
