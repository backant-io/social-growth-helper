import type { ReactNode } from "react";

interface Props {
  title: string;
  children: ReactNode;
  actions: ReactNode;
}

export function Modal({ title, children, actions }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-6" role="dialog" aria-modal="true" aria-label={title}>
      <div className="max-h-full w-full max-w-lg overflow-y-auto rounded-2xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-800 dark:bg-zinc-950">
        <h2 className="mb-3 text-lg font-semibold">{title}</h2>
        {children}
        <div className="mt-4 flex justify-end gap-2">{actions}</div>
      </div>
    </div>
  );
}
