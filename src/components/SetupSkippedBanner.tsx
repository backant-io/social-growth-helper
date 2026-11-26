import { btn } from "./ui";

/** Shown while setup is skipped: the app is open for browsing, but nothing can act on X yet. */
export function SetupSkippedBanner({ onResume }: { onResume: () => void }) {
  return (
    <div role="status" data-testid="setup-skipped-banner" className="flex items-center justify-between gap-3 border-b border-zinc-200 bg-zinc-100/70 px-10 py-2 text-sm dark:border-zinc-800 dark:bg-zinc-900/70">
      <span className="text-zinc-700 dark:text-zinc-300">
        <strong className="font-semibold">Setup skipped.</strong> You are browsing. Scout, publishing and other X actions stay off until you acknowledge the rules.
      </span>
      <button type="button" onClick={onResume} className={btn}>
        Finish setup
      </button>
    </div>
  );
}
