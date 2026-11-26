import type { BrowserInfo } from "./types";

export interface BrowserNotice {
  id: "chrome-version" | "window-placement";
  text: string;
}

/** Warnings shown above the browser controls: unsupported Chrome version (BRW-15) and Wayland placement fallback (BRW-14). */
export function browserNotices(info: BrowserInfo): BrowserNotice[] {
  const notices: BrowserNotice[] = [];
  const check = info.version_check;
  if (check && check.status !== "ok" && check.warning) {
    notices.push({ id: "chrome-version", text: check.warning });
  }
  if (info.placement_hint) {
    notices.push({ id: "window-placement", text: info.placement_hint });
  }
  return notices;
}
