import type {
  AppInfo,
  BrowserInfo,
  OnboardingStatus,
  Preferences,
  RulesNotice,
  RulesStatus,
  SecretsStatus,
  VisibleTweet,
} from "./types";
import rulesText from "../../src-tauri/src/onboarding/rules_of_engagement.md?raw";

export const NOTICE_VERSION = 1;

export function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import("@tauri-apps/api/core");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e));
  }
}

const MOCK_KEY = "sgh-mock-state";

interface MockState {
  tour_completed: boolean;
  preferences: Preferences | null;
  ack_version: number | null;
  ack_at: string | null;
}

function mockState(): MockState {
  const raw = localStorage.getItem(MOCK_KEY);
  return raw
    ? (JSON.parse(raw) as MockState)
    : { tour_completed: false, preferences: null, ack_version: null, ack_at: null };
}

function saveMock(s: MockState) {
  localStorage.setItem(MOCK_KEY, JSON.stringify(s));
}

function mockRules(s: MockState): RulesStatus {
  return {
    current_version: NOTICE_VERSION,
    acknowledged_version: s.ack_version,
    acknowledged_at: s.ack_at,
    acknowledged: (s.ack_version ?? 0) >= NOTICE_VERSION,
  };
}

const emptyPrefs: Preferences = { niche_topics: [], prefer_accounts: [], avoid_accounts: [], languages: ["en"] };

/** In a plain browser (vite dev, vitest, screenshots) the backend is replaced by localStorage so the UI stays explorable. */
const mock = {
  app_info: (): AppInfo => ({
    version: "0.1.0",
    data_dir: "(browser preview)",
    db_path: "(browser preview)",
    schema_version: 2,
    platform: "preview",
    arch: "n/a",
  }),
  secrets: (): SecretsStatus => ({
    platform: "macos",
    available: true,
    backend: "browser preview (no real keychain)",
    issue: null,
    guide: null,
  }),
  browser: (): BrowserInfo => ({ install: null, problem: "Browser control is only available in the desktop app.", override_path: null, running: false }),
};

export const api = {
  async appInfo(): Promise<AppInfo> {
    return inTauri() ? call("get_app_info") : mock.app_info();
  },
  async onboardingStatus(): Promise<OnboardingStatus> {
    if (inTauri()) return call("get_onboarding_status");
    const s = mockState();
    return { rules: mockRules(s), tour_completed: s.tour_completed, preferences_saved: s.preferences !== null };
  },
  async rulesNotice(): Promise<RulesNotice> {
    return inTauri() ? call("get_rules_notice") : { version: NOTICE_VERSION, text: rulesText };
  },
  async acknowledgeRules(version: number, confirmed: boolean): Promise<RulesStatus> {
    if (inTauri()) return call("acknowledge_rules", { version, confirmed });
    if (!confirmed) throw new Error("tick the checkbox to confirm you have read the rules");
    const s = mockState();
    s.ack_version = version;
    s.ack_at = new Date().toISOString();
    saveMock(s);
    return mockRules(s);
  },
  async completeTour(): Promise<void> {
    if (inTauri()) return call("complete_tour");
    const s = mockState();
    s.tour_completed = true;
    saveMock(s);
  },
  async getPreferences(): Promise<Preferences> {
    if (inTauri()) return call("get_preferences");
    return mockState().preferences ?? emptyPrefs;
  },
  async savePreferences(preferences: Preferences): Promise<Preferences> {
    if (inTauri()) return call("save_preferences", { preferences });
    const s = mockState();
    s.preferences = preferences;
    saveMock(s);
    return preferences;
  },
  async secretsStatus(): Promise<SecretsStatus> {
    return inTauri() ? call("secrets_status") : mock.secrets();
  },
  async browserInfo(): Promise<BrowserInfo> {
    return inTauri() ? call("browser_info") : mock.browser();
  },
  async setBrowserPath(path: string | null): Promise<void> {
    return inTauri() ? call("set_browser_path", { path }) : undefined;
  },
  async launchX(): Promise<boolean> {
    return call("browser_launch_x");
  },
  async closeX(): Promise<void> {
    return call("browser_close");
  },
  async readVisibleTweets(): Promise<VisibleTweet[]> {
    return call("browser_read_visible_tweets");
  },
  async quitApp(): Promise<void> {
    return call("quit_app");
  },
};

export async function listen<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!inTauri()) return () => {};
  const { listen: tauriListen } = await import("@tauri-apps/api/event");
  return tauriListen<T>(event, (e) => handler(e.payload));
}
