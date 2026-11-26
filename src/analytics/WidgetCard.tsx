import { WidgetChartRenderer } from "./charts/ChartPrimitives";
import type { EvaluatedWidget } from "./types";

interface WidgetCardProps {
  item: EvaluatedWidget;
  customizeMode: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onReprompt: () => void;
  onExportCsv: () => void;
  onRemove: () => void;
  onMove: (delta: -1 | 1) => void;
  onResizeWidth: (w: number) => void;
}

const WIDTH_CLASSES: Record<number, string> = {
  3: "md:col-span-3",
  4: "md:col-span-4",
  6: "md:col-span-6",
  8: "md:col-span-8",
  12: "md:col-span-12",
};

function colSpanClass(w: number): string {
  if (w <= 3) return WIDTH_CLASSES[3];
  if (w <= 4) return WIDTH_CLASSES[4];
  if (w <= 6) return WIDTH_CLASSES[6];
  if (w <= 8) return WIDTH_CLASSES[8];
  return WIDTH_CLASSES[12];
}

export function WidgetCard({
  item,
  customizeMode,
  canMoveUp,
  canMoveDown,
  onReprompt,
  onExportCsv,
  onRemove,
  onMove,
  onResizeWidth,
}: WidgetCardProps) {
  const { widget, data, error } = item;
  const isKpi = widget.spec.chart_type === "kpi";

  return (
    <section
      className={`col-span-12 ${colSpanClass(widget.position.w)} flex flex-col rounded-xl border border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900/80 p-4 shadow-sm transition-all ${
        customizeMode ? "ring-1 ring-sky-500/40" : ""
      }`}
    >
      <header className="mb-3 flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="text-sm font-semibold text-zinc-900 dark:text-zinc-100 truncate">
            {widget.title}
          </h3>
          {!isKpi && widget.spec.options.subtitle && (
            <p className="text-xs text-zinc-500 dark:text-zinc-400 truncate">
              {widget.spec.options.subtitle}
            </p>
          )}
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {customizeMode ? (
            <>
              <button
                type="button"
                disabled={!canMoveUp}
                onClick={() => onMove(-1)}
                title="Move earlier"
                className="rounded border border-zinc-200 dark:border-zinc-700 px-1.5 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30"
              >
                ←
              </button>
              <button
                type="button"
                disabled={!canMoveDown}
                onClick={() => onMove(1)}
                title="Move later"
                className="rounded border border-zinc-200 dark:border-zinc-700 px-1.5 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30"
              >
                →
              </button>
              <select
                aria-label={`Width for ${widget.title}`}
                value={widget.position.w <= 4 ? 4 : widget.position.w <= 6 ? 6 : widget.position.w <= 8 ? 8 : 12}
                onChange={(e) => onResizeWidth(Number(e.target.value))}
                className="rounded border border-zinc-200 dark:border-zinc-700 bg-transparent px-1 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300"
              >
                <option value={4}>1/3</option>
                <option value={6}>1/2</option>
                <option value={8}>2/3</option>
                <option value={12}>Full</option>
              </select>
              <button
                type="button"
                onClick={onRemove}
                title="Remove widget"
                className="rounded border border-rose-200 dark:border-rose-900 px-1.5 py-0.5 text-[11px] text-rose-600 dark:text-rose-400 hover:bg-rose-50 dark:hover:bg-rose-950"
              >
                Remove
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={onReprompt}
                title="Reprompt or edit widget spec"
                className="rounded px-2 py-0.5 text-[11px] text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
              >
                Reprompt
              </button>
              <button
                type="button"
                onClick={onExportCsv}
                title="Export CSV"
                className="rounded px-2 py-0.5 text-[11px] text-zinc-500 hover:bg-zinc-100 hover:text-zinc-800 dark:hover:bg-zinc-800 dark:hover:text-zinc-200"
              >
                CSV
              </button>
            </>
          )}
        </div>
      </header>

      <div className="flex-1">
        {error ? (
          <div className="rounded-md border border-rose-200 dark:border-rose-900 bg-rose-50/70 dark:bg-rose-950/40 p-3 text-xs text-rose-700 dark:text-rose-300">
            <div className="font-medium">Widget query blocked or failed</div>
            <p className="mt-0.5 opacity-90">{error}</p>
          </div>
        ) : data ? (
          <WidgetChartRenderer spec={widget.spec} data={data} />
        ) : null}
      </div>

      {!isKpi && data && (
        <footer className="mt-3 flex items-center justify-between border-t border-zinc-100 dark:border-zinc-800/70 pt-2 text-[10px] text-zinc-400">
          <span>
            {widget.spec.chart_type} · {widget.spec.view.replace("v_analytics_", "")}
          </span>
          <span>
            {data.row_count} {data.row_count === 1 ? "row" : "rows"}
            {data.truncated ? " (capped)" : ""}
          </span>
        </footer>
      )}
    </section>
  );
}
