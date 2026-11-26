import { useEffect, useState } from "react";

export interface ParsedLlmError {
  code: string | null;
  message: string;
  isOffline: boolean;
  isUnsetRoute: boolean;
  isRateGuard: boolean;
  isAuthError: boolean;
}

const PREFIX_RE = /^\[llm:([a-z0-9_]+)\]\s*(.*)$/i;

export function parseLlmError(raw: unknown): ParsedLlmError {
  const text = raw instanceof Error ? raw.message : typeof raw === "string" ? raw : String(raw);
  const match = text.match(PREFIX_RE);
  const code = match ? match[1].toLowerCase() : null;
  const message = match ? match[2] : text;
  const lower = message.toLowerCase();

  const isOffline =
    code === "offline" ||
    (!navigatorOnLineSafe() && (lower.includes("offline") || lower.includes("network") || lower.includes("reach")));

  return {
    code,
    message,
    isOffline,
    isUnsetRoute: code === "unset_route",
    isRateGuard: code === "rate_guard_blocked" || code === "rate_limited",
    isAuthError: code === "auth_expired" || code === "not_logged_in",
  };
}

export function navigatorOnLineSafe(): boolean {
  if (typeof navigator === "undefined" || typeof navigator.onLine !== "boolean") {
    return true;
  }
  return navigator.onLine;
}

export const OFFLINE_ACTION_HINT = "You're offline. Publishing and sending need a network connection.";

export function offlineActionHint(online: boolean): string | undefined {
  return online ? undefined : OFFLINE_ACTION_HINT;
}

export function useOnlineStatus(): boolean {
  const [online, setOnline] = useState<boolean>(navigatorOnLineSafe);

  useEffect(() => {
    const onOnline = () => setOnline(true);
    const onOffline = () => setOnline(false);
    window.addEventListener("online", onOnline);
    window.addEventListener("offline", onOffline);
    return () => {
      window.removeEventListener("online", onOnline);
      window.removeEventListener("offline", onOffline);
    };
  }, []);

  return online;
}
