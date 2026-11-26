interface Props {
  jobs: string[];
  onCancel: () => void;
  onQuit: () => void;
}

export function QuitGuardModal({ jobs, onCancel, onQuit }: Props) {
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-6" role="alertdialog" aria-modal="true" aria-label="Quit while work is running">
      <div className="w-full max-w-md rounded-2xl border border-zinc-200 bg-white p-6 shadow-xl dark:border-zinc-800 dark:bg-zinc-950">
        <h2 className="mb-2 text-lg font-semibold">Work is still running</h2>
        <p className="mb-3 text-sm text-zinc-600 dark:text-zinc-400">
          Quitting stops these jobs at the next safe point and closes the X window.
        </p>
        <ul className="mb-4 list-disc pl-5 text-sm">
          {jobs.map((j, i) => (
            <li key={`${j}-${i}`}>{j}</li>
          ))}
        </ul>
        <div className="flex justify-end gap-2">
          <button type="button" onClick={onCancel} className="rounded-lg px-4 py-2 text-sm text-zinc-700 dark:text-zinc-300">
            Keep working
          </button>
          <button type="button" onClick={onQuit} className="rounded-lg bg-red-600 px-4 py-2 text-sm text-white">
            Stop and quit
          </button>
        </div>
      </div>
    </div>
  );
}
