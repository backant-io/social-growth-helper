import type { SectionId } from "../lib/types";
import { SECTIONS } from "./sections";
import { useKillSwitch } from "./ui";

interface Props {
  active: SectionId;
  onSelect: (id: SectionId) => void;
  locked: boolean;
  badges?: Partial<Record<SectionId, number>>;
}

export function Sidebar({ active, onSelect, locked, badges }: Props) {
  const paused = useKillSwitch();
  return (
    <nav aria-label="Main" data-vibrancy-sidebar className="sidebar-surface flex w-60 shrink-0 flex-col border-r border-zinc-200 px-3 pt-5 pb-4 dark:border-zinc-800">
      <div className="mb-7 flex items-center gap-2.5 px-2">
        <span aria-hidden="true" className="grid h-7 w-7 place-items-center rounded-md bg-zinc-950 font-display text-sm font-bold text-white dark:bg-zinc-100 dark:text-zinc-950">
          S
        </span>
        <span className="font-display text-[0.95rem] leading-none font-semibold tracking-tight">
          Social Growth
          <br />
          <span className="font-medium text-zinc-500">Helper</span>
        </span>
      </div>
      <ul className="space-y-0.5">
        {SECTIONS.map((s) => {
          const isActive = s.id === active;
          return (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => onSelect(s.id)}
                aria-current={isActive ? "page" : undefined}
                data-tour={`nav-${s.id}`}
                className={`relative flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm transition-colors ${
                  isActive
                    ? "bg-zinc-200/70 font-medium text-zinc-950 dark:bg-zinc-800/70 dark:text-zinc-50"
                    : "text-zinc-600 hover:bg-zinc-200/40 hover:text-zinc-950 dark:text-zinc-400 dark:hover:bg-zinc-800/40 dark:hover:text-zinc-50"
                }`}
              >
                {isActive && <span aria-hidden="true" className="absolute top-2 bottom-2 left-0 w-0.5 rounded-full bg-zinc-950 dark:bg-zinc-100" />}
                <span>{s.label}</span>
                {!locked && !!badges?.[s.id] && (
                  <span aria-label={`${badges[s.id]} waiting`} className="rounded-full bg-zinc-950 px-1.5 text-[10px] font-semibold text-white dark:bg-zinc-100 dark:text-zinc-950">
                    {badges[s.id]}
                  </span>
                )}
                {locked && s.id !== "settings" && <span aria-label="locked" className="eyebrow">locked</span>}
              </button>
            </li>
          );
        })}
      </ul>
      {paused && (
        <div role="status" className="mt-auto flex items-center gap-2 rounded-lg border border-red-200 px-3 py-2 text-xs font-medium text-red-800 dark:border-red-900 dark:text-red-200">
          <span aria-hidden="true" className="h-2 w-2 rounded-full bg-red-600" />
          Paused
        </div>
      )}
    </nav>
  );
}
