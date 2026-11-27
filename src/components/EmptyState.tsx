import type { Section } from "./sections";
import { btnPrimary } from "./ui";

interface Props {
  section: Section;
  locked: boolean;
  onReviewRules: () => void;
}

export function EmptyState({ section, locked, onReviewRules }: Props) {
  return (
    <div className="mx-auto flex h-full max-w-md flex-col items-center justify-center text-center">
      <span className="eyebrow mb-3">{locked ? "Locked" : "Nothing here yet"}</span>
      <h1 className="mb-2 font-display text-[1.75rem] font-semibold tracking-tight">{section.title}</h1>
      {locked ? (
        <>
          <p className="mb-4 text-sm text-zinc-600 dark:text-zinc-400">
            This area is locked until you read and acknowledge the rules of engagement.
          </p>
          <button type="button" onClick={onReviewRules} className={btnPrimary}>
            Read the rules
          </button>
        </>
      ) : (
        <p className="text-sm text-zinc-600 dark:text-zinc-400">{section.empty}</p>
      )}
    </div>
  );
}
