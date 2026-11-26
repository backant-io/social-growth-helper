const KEY = "sgh.setup_skipped";

const memory = new Map<string, string>();

function read(): string | null {
  try {
    if (typeof localStorage !== "undefined" && typeof localStorage.getItem === "function") return localStorage.getItem(KEY);
  } catch {
    /* storage blocked: fall back to memory */
  }
  return memory.get(KEY) ?? null;
}

function write(value: string | null): void {
  try {
    if (typeof localStorage !== "undefined" && typeof localStorage.setItem === "function") {
      if (value === null) localStorage.removeItem(KEY);
      else localStorage.setItem(KEY, value);
      return;
    }
  } catch {
    /* storage blocked: fall back to memory */
  }
  if (value === null) memory.delete(KEY);
  else memory.set(KEY, value);
}

export function isSetupSkipped(): boolean {
  return read() === "1";
}

export function markSetupSkipped(): void {
  write("1");
}

export function clearSetupSkipped(): void {
  write(null);
}

/**
 * Screens that need the rules acknowledgement stay locked until it is given, unless the user skipped
 * setup to look around. Skipping only unlocks the screens: the backend still refuses every action that
 * touches X until the rules are acknowledged, so a skipped setup cannot post or scout.
 */
export function isLocked(rulesAcknowledged: boolean, skipped: boolean): boolean {
  return !rulesAcknowledged && !skipped;
}

export function needsSetup(rulesAcknowledged: boolean): boolean {
  return !rulesAcknowledged;
}
