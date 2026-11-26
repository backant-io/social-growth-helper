import { useEffect, useRef } from "react";
import { matchShortcut, type ShortcutAction } from "./logic";

/** Page-level shortcut handler; mounted pages register while visible. */
export function useShortcuts(handler: (a: ShortcutAction) => boolean | void) {
  const ref = useRef(handler);
  ref.current = handler;
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const a = matchShortcut(e);
      if (a && ref.current(a) !== false) e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

export function useAsyncError() {
  return (e: unknown) => (e instanceof Error ? e.message : String(e));
}
