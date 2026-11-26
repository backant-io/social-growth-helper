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
    <nav aria-label="Main" data-vibrancy-sidebar className="flex w-56 shrink-0 flex-col border-r border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-900">
      <div className="mb-4 px-2 pt-2 text-sm font-semibold tracking-tight">Social Growth Helper</div>
      <ul className="space-y-1">
        {SECTIONS.map((s) => {
          const isActive = s.id === active;
          return (
            <li key={s.id}>
              <button
                type="button"
                onClick={() => onSelect(s.id)}
                aria-current={isActive ? "page" : undefined}
                className={`flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm ${
                  isActive
                    ? "bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900"
                    : "text-zinc-700 hover:bg-zinc-200 dark:text-zinc-300 dark:hover:bg-zinc-800"
                }`}
              >
                <span>{s.label}</span>
                {!locked && !!badges?.[s.id] && (
                  <span aria-label={`${badges[s.id]} waiting`} className="rounded-full bg-amber-500 px-1.5 text-[10px] font-semibold text-white">
                    {badges[s.id]}
                  </span>
                )}
                {locked && s.id !== "settings" && <span aria-label="locked" className="text-[10px] uppercase opacity-60">locked</span>}
              </button>
            </li>
          );
        })}
      </ul>
      {paused && (
        <div role="status" className="mt-auto flex items-center gap-2 rounded-lg bg-red-50 px-3 py-2 text-xs font-medium text-red-900 dark:bg-red-950 dark:text-red-100">
          <span aria-hidden="true" className="h-2 w-2 rounded-full bg-red-600" />
          Paused
        </div>
      )}
    </nav>
  );
}
