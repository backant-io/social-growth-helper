import { useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "../lib/api";
import { p3 } from "../phase3/api";

export const panel = "border-t border-zinc-200 pt-5 pb-1 dark:border-zinc-800";
export const btn =
  "inline-flex shrink-0 items-center whitespace-nowrap justify-center gap-1.5 rounded-lg border border-zinc-300 bg-transparent px-3.5 py-1.5 text-sm font-medium transition-colors hover:border-zinc-900 hover:bg-zinc-100 disabled:pointer-events-none disabled:opacity-40 dark:border-zinc-700 dark:hover:border-zinc-300 dark:hover:bg-zinc-900";
export const btnPrimary =
  "inline-flex shrink-0 items-center whitespace-nowrap justify-center gap-1.5 rounded-lg border border-zinc-950 bg-zinc-950 px-3.5 py-1.5 text-sm font-medium text-white transition-colors hover:bg-zinc-700 disabled:pointer-events-none disabled:opacity-40 dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-950 dark:hover:bg-zinc-300";
export const btnGhost =
  "inline-flex shrink-0 items-center whitespace-nowrap justify-center gap-1.5 rounded-lg px-3 py-1.5 text-sm text-zinc-600 transition-colors hover:bg-zinc-100 hover:text-zinc-950 disabled:pointer-events-none disabled:opacity-40 dark:text-zinc-400 dark:hover:bg-zinc-900 dark:hover:text-zinc-50";
export const btnDanger =
  "inline-flex shrink-0 items-center whitespace-nowrap justify-center gap-1.5 rounded-lg border border-red-600 bg-red-600 px-3.5 py-1.5 text-sm font-medium text-white transition-colors hover:bg-red-500 disabled:pointer-events-none disabled:opacity-40";
export const input =
  "min-w-0 rounded-lg border border-zinc-300 bg-transparent px-3 py-1.5 text-sm transition-colors placeholder:text-zinc-400 hover:border-zinc-400 focus:border-zinc-900 focus:outline-none dark:border-zinc-700 dark:hover:border-zinc-500 dark:focus:border-zinc-100";
export const field = `${input} w-full`;
export const muted = "text-zinc-600 dark:text-zinc-400";
export const danger = "text-red-600 dark:text-red-400";
export const sectionTitle = "font-display text-base font-semibold tracking-tight";
export const subTitle = "text-sm font-medium";

export function chip(active: boolean): string {
  return `rounded-full border px-3 py-1 text-sm transition-colors ${
    active ? "border-zinc-950 bg-zinc-950 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-950" : "border-zinc-300 hover:border-zinc-900 dark:border-zinc-700 dark:hover:border-zinc-300"
  }`;
}

export function Modal({ title, children, onClose, wide, actions }: { title: string; children: ReactNode; onClose: () => void; wide?: boolean; actions?: ReactNode }) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-zinc-950/50 p-6 backdrop-blur-[2px]" role="dialog" aria-modal="true" aria-label={title}>
      <div ref={ref} tabIndex={-1} className={`max-h-[90vh] w-full overflow-y-auto rounded-2xl border border-zinc-200 bg-[var(--bg)] p-7 shadow-xl outline-none dark:border-zinc-800 ${wide ? "max-w-2xl" : "max-w-md"}`}>
        <h2 className="mb-3 text-lg font-semibold">{title}</h2>
        {children}
        {actions && <div className="mt-4 flex justify-end gap-2">{actions}</div>}
      </div>
    </div>
  );
}

export function Notice({ tone, children }: { tone: "info" | "warn" | "error" | "ok"; children: ReactNode }) {
  const c = {
    info: "border-sky-200 bg-sky-50 text-sky-900 dark:border-sky-900 dark:bg-sky-950 dark:text-sky-200",
    warn: "border-amber-200 bg-amber-50 text-amber-900 dark:border-amber-900 dark:bg-amber-950 dark:text-amber-200",
    error: "border-red-200 bg-red-50 text-red-900 dark:border-red-900 dark:bg-red-950 dark:text-red-200",
    ok: "border-emerald-200 bg-emerald-50 text-emerald-900 dark:border-emerald-900 dark:bg-emerald-950 dark:text-emerald-200",
  }[tone];
  return (
    <div role={tone === "error" ? "alert" : "status"} className={`rounded-lg border px-3.5 py-2.5 text-sm ${c}`}>
      {children}
    </div>
  );
}

/** Page title block: one h1 size everywhere, optional one-line description and right-aligned actions. */
export function PageHeader({ title, description, actions }: { title: string; description?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="flex flex-wrap items-start justify-between gap-3 pb-1">
      <div className="min-w-0">
        <h1 className="font-display text-[1.75rem] leading-tight font-semibold tracking-tight">{title}</h1>
        {description && <p className={`mt-1.5 max-w-2xl text-sm leading-relaxed ${muted}`}>{description}</p>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
    </header>
  );
}

export interface TabItem<T extends string> {
  id: T;
  label: string;
}

/** Underline tabs used by every page that has sub-sections (Creator, Learning, Style, Settings). */
export function Tabs<T extends string>({ label, tabs, active, onChange, idPrefix }: { label: string; tabs: Array<TabItem<T>>; active: T; onChange: (id: T) => void; idPrefix?: string }) {
  return (
    <div role="tablist" aria-label={label} className="flex flex-wrap gap-1 border-b border-zinc-200 dark:border-zinc-800">
      {tabs.map((t) => (
        <button
          key={t.id}
          type="button"
          role="tab"
          id={idPrefix ? `${idPrefix}-tab-${t.id}` : undefined}
          aria-selected={active === t.id}
          aria-controls={idPrefix ? `${idPrefix}-panel-${t.id}` : undefined}
          onClick={() => onChange(t.id)}
          className={`-mb-px border-b-2 px-3 py-2 text-sm transition-colors ${
            active === t.id ? "border-zinc-950 font-medium dark:border-zinc-100" : "border-transparent text-zinc-500 hover:text-zinc-950 dark:hover:text-zinc-100"
          }`}
        >
          {t.label}
        </button>
      ))}
    </div>
  );
}

/** Dashed placeholder used when a list has nothing in it yet. Say what the list is for and what to do next. */
export function EmptyBlock({ title, children, action }: { title?: string; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="rounded-xl border border-dashed border-zinc-300 bg-[radial-gradient(circle_at_1px_1px,var(--dot)_1px,transparent_0)] bg-[length:18px_18px] p-8 text-center text-sm dark:border-zinc-700">
      {title && <p className="font-display text-base font-semibold tracking-tight">{title}</p>}
      <p className={`${title ? "mt-1" : ""} ${muted}`}>{children}</p>
      {action && <div className="mt-3 flex justify-center gap-2">{action}</div>}
    </div>
  );
}

export function LoadingBlock({ label = "Loading..." }: { label?: string }) {
  return (
    <div role="status" aria-live="polite" className="space-y-3" aria-label={label}>
      <div className="h-16 animate-pulse rounded-xl bg-zinc-100 dark:bg-zinc-900" />
      <div className="h-16 animate-pulse rounded-xl bg-zinc-100 dark:bg-zinc-900" />
      <span className="sr-only">{label}</span>
    </div>
  );
}

/** Live kill-switch state shared by the shell bar, the sidebar and every screen that can write to X. */
export function useKillSwitch(): boolean {
  const [engaged, setEngaged] = useState(false);
  useEffect(() => {
    let alive = true;
    p3.killSwitch().then((v) => alive && setEngaged(v)).catch(() => undefined);
    const off = listen<boolean>("kill:changed", setEngaged);
    const onPreview = (e: Event) => setEngaged((e as CustomEvent<boolean>).detail);
    window.addEventListener("sgh:kill", onPreview);
    return () => {
      alive = false;
      window.removeEventListener("sgh:kill", onPreview);
      void off.then((f) => f());
    };
  }, []);
  return engaged;
}
