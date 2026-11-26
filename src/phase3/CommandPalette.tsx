import { useEffect, useMemo, useRef, useState } from "react";
import { filterCommands, SHORTCUTS, type PaletteCommand } from "./logic";
import { input, Modal, muted } from "./ui";

interface Props {
  commands: PaletteCommand[];
  onRun: (id: string) => void;
  onClose: () => void;
}

export function CommandPalette({ commands, onRun, onClose }: Props) {
  const [q, setQ] = useState("");
  const [i, setI] = useState(0);
  const ref = useRef<HTMLInputElement>(null);
  const shown = useMemo(() => filterCommands(commands, q), [commands, q]);
  useEffect(() => ref.current?.focus(), []);
  useEffect(() => setI(0), [q]);

  const run = (id: string) => {
    onClose();
    onRun(id);
  };

  return (
    <Modal title="Command palette" onClose={onClose}>
      <input
        ref={ref}
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder="Type a command"
        aria-label="Command"
        className={`${input} w-full`}
        onKeyDown={(e) => {
          if (e.key === "ArrowDown") {
            e.preventDefault();
            setI((n) => Math.min(shown.length - 1, n + 1));
          } else if (e.key === "ArrowUp") {
            e.preventDefault();
            setI((n) => Math.max(0, n - 1));
          } else if (e.key === "Enter" && shown[i]) run(shown[i].id);
        }}
      />
      <ul className="mt-3 max-h-72 overflow-y-auto" role="listbox" aria-label="Commands">
        {shown.map((c, n) => (
          <li key={c.id} role="option" aria-selected={n === i}>
            <button type="button" onClick={() => run(c.id)} className={`flex w-full items-center justify-between rounded-lg px-3 py-2 text-left text-sm ${n === i ? "bg-zinc-100 dark:bg-zinc-800" : ""}`}>
              <span>{c.label}</span>
              {c.hint && <span className={`text-xs ${muted}`}>{c.hint}</span>}
            </button>
          </li>
        ))}
        {shown.length === 0 && <li className={`px-3 py-2 text-sm ${muted}`}>No matching command.</li>}
      </ul>
    </Modal>
  );
}

export function ShortcutHelp({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="Keyboard shortcuts" onClose={onClose}>
      <dl className="space-y-2 text-sm">
        {SHORTCUTS.map((s) => (
          <div key={s.keys} className="flex items-baseline justify-between gap-4">
            <dt className="font-mono text-xs">{s.keys}</dt>
            <dd className={muted}>{s.label}</dd>
          </div>
        ))}
      </dl>
    </Modal>
  );
}
