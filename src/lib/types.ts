export interface Preferences {
  niche_topics: string[];
  prefer_accounts: string[];
  avoid_accounts: string[];
  languages: string[];
}

export interface RulesNotice {
  version: number;
  text: string;
}

export interface RulesStatus {
  current_version: number;
  acknowledged_version: number | null;
  acknowledged_at: string | null;
  acknowledged: boolean;
}

export interface OnboardingStatus {
  rules: RulesStatus;
  tour_completed: boolean;
  preferences_saved: boolean;
}

export interface DistroSteps {
  name: string;
  steps: string[];
}

export interface SetupGuide {
  title: string;
  summary: string;
  no_file_fallback: boolean;
  options: DistroSteps[];
  verify: string;
}

export interface SecretsStatus {
  platform: "macos" | "linux" | "other";
  available: boolean;
  backend: string;
  issue: string | null;
  guide: SetupGuide | null;
}

export interface BrowserInstall {
  path: string;
  kind: "chrome" | "chromium" | "brave" | "custom";
  from_override: boolean;
}

export interface BrowserInfo {
  install: BrowserInstall | null;
  problem: string | null;
  override_path: string | null;
  running: boolean;
}

export interface VisibleTweet {
  id: string | null;
  handle: string | null;
  author: string;
  text: string;
  promoted: boolean;
  time: string | null;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  db_path: string;
  schema_version: number;
  platform: string;
  arch: string;
}

export type SectionId = "scout" | "reply" | "creator" | "analytics" | "learning" | "style" | "settings";

export type QuitGuardPayload = { kind: "needs_confirmation"; jobs: string[] } | { kind: "proceed" };
