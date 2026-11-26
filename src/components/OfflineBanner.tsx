interface Props {
  offline: boolean;
  detail?: string | null;
}

export function OfflineBanner({ offline, detail }: Props) {
  if (!offline) return null;
  return (
    <div
      role="status"
      aria-live="polite"
      className="mb-4 rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900 dark:border-amber-800 dark:bg-amber-950/40 dark:text-amber-200"
    >
      <div className="font-medium">You appear to be offline.</div>
      <p className="mt-0.5 text-xs opacity-90">
        {detail ||
          "Live AI generation, provider health checks, and X browser imports are paused. Your Style Guide, examples, and local settings remain editable with zero data loss."}
      </p>
    </div>
  );
}
