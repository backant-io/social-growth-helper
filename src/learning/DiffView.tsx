import type { DiffLine } from "./types";

const styles: Record<DiffLine["kind"], string> = {
  add: "bg-emerald-50 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300",
  remove: "bg-red-50 text-red-800 line-through dark:bg-red-950 dark:text-red-300",
  same: "text-zinc-500",
};
const marks: Record<DiffLine["kind"], string> = { add: "+", remove: "-", same: " " };

export function DiffView({ diff, label }: { diff: DiffLine[]; label: string }) {
  if (!diff.length) return <p className="text-xs text-zinc-500">No changes to show.</p>;
  return (
    <ul aria-label={label} className="overflow-hidden rounded-lg border border-zinc-200 font-mono text-xs dark:border-zinc-800">
      {diff.map((d, i) => (
        <li key={i} className={`flex gap-2 px-2 py-1 ${styles[d.kind]}`}>
          <span aria-hidden className="select-none">{marks[d.kind]}</span>
          <span className="min-w-0 break-words">{d.text}</span>
        </li>
      ))}
    </ul>
  );
}
