import { useCallback, useEffect, useState } from "react";
import { api, listen } from "../lib/api";
import { bannerFor, sameStatus, type BannerAction, type BrowserStatus } from "../lib/browserState";
import { btn } from "../phase3/ui";

const POLL_MS = 5000;

const TONES = {
  warn: "border-amber-300 bg-amber-50 text-amber-900 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200",
  error: "border-red-300 bg-red-50 text-red-900 dark:border-red-900 dark:bg-red-950 dark:text-red-100",
  info: "border-sky-300 bg-sky-50 text-sky-900 dark:border-sky-800 dark:bg-sky-950/40 dark:text-sky-200",
};

/** Persistent X state: logged out, challenge, suspended, locked, closed unexpectedly (BRW-06, BRW-08, BRW-11). */
export function BrowserStateBanner() {
  const [status, setStatus] = useState<BrowserStatus | null>(null);
  const [busy, setBusy] = useState<BannerAction | null>(null);
  const [error, setError] = useState<string | null>(null);

  const update = useCallback((next: BrowserStatus | null) => setStatus((cur) => (sameStatus(cur, next) ? cur : next)), []);
  const refresh = useCallback(() => {
    api.browserStatus().then(update).catch(() => undefined);
  }, [update]);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, POLL_MS);
    const off = listen("browser:halt", refresh);
    return () => {
      window.clearInterval(timer);
      void off.then((f) => f());
    };
  }, [refresh]);

  const banner = bannerFor(status);
  if (!banner) return null;

  const run = async (action: BannerAction) => {
    setBusy(action);
    setError(null);
    try {
      if (action === "focus") await api.focusX();
      else if (action === "resume") update(await api.resumeBrowser());
      else if (action === "abort") update(await api.abortBrowserJob());
      else {
        await api.launchX();
        refresh();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const label: Record<BannerAction, string> = { focus: "Show X window", resume: banner.resumeLabel, abort: "Abort", relaunch: "Relaunch X" };

  return (
    <div role="alert" className={`flex flex-wrap items-center justify-between gap-3 border-b px-6 py-2 text-sm ${TONES[banner.tone]}`}>
      <span>
        <strong className="font-medium">{banner.title}.</strong> {banner.body}
        {error && <span className="ml-2 font-medium">{error}</span>}
      </span>
      <span className="flex gap-2">
        {banner.actions.map((a) => (
          <button key={a} type="button" className={btn} disabled={busy !== null} onClick={() => void run(a)}>
            {label[a]}
          </button>
        ))}
      </span>
    </div>
  );
}
