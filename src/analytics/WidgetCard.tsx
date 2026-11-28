import { useRef, useState } from "react";
import { WidgetChartRenderer } from "./charts/ChartPrimitives";
import type { EvaluatedWidget } from "./types";

interface WidgetCardProps {
  item: EvaluatedWidget;
  customizeMode: boolean;
  /** Render as a bare number tile (used for runs of KPI widgets outside customize mode). */
  tile?: boolean;
  isDragging: boolean;
  isDropTarget: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  onReprompt: () => void;
  onExportCsv: () => void;
  onRemove: () => void;
  onMove: (delta: -1 | 1) => void;
  onResize: (w: number, h: number) => void;
  onDragStart: () => void;
  onDragOver: () => void;
  onDrop: () => void;
  onDragEnd: () => void;
}

const WIDTH_CLASSES: Record<number, string> = {
  3: "md:col-span-3",
  4: "md:col-span-4",
  6: "md:col-span-6",
  8: "md:col-span-8",
  12: "md:col-span-12",
};

const HEIGHT_CLASSES: Record<number, string> = {
  1: "min-h-[112px]",
  2: "min-h-[240px]",
  3: "min-h-[340px]",
  4: "min-h-[440px]",
};

function colSpanClass(w: number): string {
  if (w <= 3) return WIDTH_CLASSES[3];
  if (w <= 4) return WIDTH_CLASSES[4];
  if (w <= 6) return WIDTH_CLASSES[6];
  if (w <= 8) return WIDTH_CLASSES[8];
  return WIDTH_CLASSES[12];
}

function normalizedWidth(w: number): number {
  if (w <= 4) return 4;
  if (w <= 6) return 6;
  if (w <= 8) return 8;
  return 12;
}

function normalizedHeight(h: number): number {
  return Math.max(1, Math.min(4, Math.round(h)));
}

function snapWidthSteps(currentW: number, deltaX: number): number {
  const steps = [4, 6, 8, 12];
  const idx = Math.max(0, steps.indexOf(normalizedWidth(currentW)));
  const shift = Math.round(deltaX / 110);
  return steps[Math.max(0, Math.min(steps.length - 1, idx + shift))];
}

function snapHeightSteps(currentH: number, deltaY: number): number {
  const shift = Math.round(deltaY / 90);
  return normalizedHeight(normalizedHeight(currentH) + shift);
}

export function WidgetCard({
  item,
  customizeMode,
  tile = false,
  isDragging,
  isDropTarget,
  canMoveUp,
  canMoveDown,
  onReprompt,
  onExportCsv,
  onRemove,
  onMove,
  onResize,
  onDragStart,
  onDragOver,
  onDrop,
  onDragEnd,
}: WidgetCardProps) {
  const { widget, data, error } = item;
  const [liveSize, setLiveSize] = useState<{ w: number; h: number } | null>(null);
  const startRef = useRef<{ x: number; y: number; w: number; h: number } | null>(null);

  const effectiveW = liveSize ? liveSize.w : normalizedWidth(widget.position.w);
  const effectiveH = liveSize ? liveSize.h : normalizedHeight(widget.position.h);

  function beginPointerResize(e: React.PointerEvent<HTMLButtonElement>) {
    e.preventDefault();
    e.stopPropagation();
    const startW = normalizedWidth(widget.position.w);
    const startH = normalizedHeight(widget.position.h);
    startRef.current = { x: e.clientX, y: e.clientY, w: startW, h: startH };
    let latestW = startW;
    let latestH = startH;

    const handleMove = (ev: PointerEvent) => {
      if (!startRef.current) return;
      latestW = snapWidthSteps(startRef.current.w, ev.clientX - startRef.current.x);
      latestH = snapHeightSteps(startRef.current.h, ev.clientY - startRef.current.y);
      setLiveSize({ w: latestW, h: latestH });
    };

    const handleUp = () => {
      window.removeEventListener("pointermove", handleMove);
      window.removeEventListener("pointerup", handleUp);
      startRef.current = null;
      setLiveSize(null);
      if (latestW !== startW || latestH !== startH) {
        onResize(latestW, latestH);
      }
    };

    window.addEventListener("pointermove", handleMove);
    window.addEventListener("pointerup", handleUp);
  }

  return (
    <section
      draggable={customizeMode && !liveSize}
      onDragStart={(e) => {
        if (!customizeMode) return;
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", widget.id);
        onDragStart();
      }}
      onDragOver={(e) => {
        if (!customizeMode) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "move";
        onDragOver();
      }}
      onDrop={(e) => {
        if (!customizeMode) return;
        e.preventDefault();
        onDrop();
      }}
      onDragEnd={onDragEnd}
      className={`group relative flex flex-col ${
        customizeMode
          ? `col-span-12 ${colSpanClass(effectiveW)} ${HEIGHT_CLASSES[effectiveH]} cursor-grab rounded-lg border bg-transparent p-5 active:cursor-grabbing ${
              isDropTarget ? "border-zinc-950 dark:border-zinc-100" : "border-zinc-200 dark:border-zinc-800"
            }`
          : tile
            ? "col-span-1 min-w-0"
            : `col-span-12 min-w-0 ${colSpanClass(effectiveW)} border-t border-zinc-200 pt-5 dark:border-zinc-800`
      } ${isDragging ? "opacity-45" : ""}`}
    >
      <header className={`flex items-start justify-between gap-2 ${tile && !customizeMode ? "mb-1" : "mb-4"}`}>
        <div className="flex items-start gap-1.5 min-w-0">
          {customizeMode && (
            <span
              title="Drag widget to reorder"
              aria-label="Drag handle"
              className="mt-0.5 select-none text-xs leading-none text-zinc-400"
            >
              ⋮⋮
            </span>
          )}
          <div className="min-w-0">
            <h3
              title={widget.spec.options.subtitle ?? undefined}
              className={`leading-snug ${tile && !customizeMode ? "text-sm text-zinc-600 dark:text-zinc-400" : "text-sm font-medium"}`}
            >
              {widget.title}
            </h3>
          </div>
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
                value={effectiveW}
                onChange={(e) => onResize(Number(e.target.value), effectiveH)}
                className="rounded border border-zinc-200 dark:border-zinc-700 bg-transparent px-1 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300"
              >
                <option value={4}>1/3</option>
                <option value={6}>1/2</option>
                <option value={8}>2/3</option>
                <option value={12}>Full</option>
              </select>
              <select
                aria-label={`Height for ${widget.title}`}
                value={effectiveH}
                onChange={(e) => onResize(effectiveW, Number(e.target.value))}
                className="rounded border border-zinc-200 dark:border-zinc-700 bg-transparent px-1 py-0.5 text-[11px] text-zinc-600 dark:text-zinc-300"
              >
                <option value={1}>1h</option>
                <option value={2}>2h</option>
                <option value={3}>3h</option>
                <option value={4}>4h</option>
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
            <div className="flex items-center gap-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
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
            </div>
          )}
        </div>
      </header>

      <div className="flex-1">
        {error ? (
          <p className="border-l-2 border-red-600 py-1 pl-3 text-sm text-zinc-700 dark:text-zinc-300">
            This chart could not load. <span className="text-zinc-500">{error}</span>
          </p>
        ) : data ? (
          <WidgetChartRenderer spec={widget.spec} data={data} />
        ) : null}
      </div>

      {customizeMode && (
        <button
          type="button"
          onPointerDown={beginPointerResize}
          title="Drag corner to resize width & height"
          aria-label={`Resize ${widget.title}`}
          className="absolute bottom-1 right-1 h-4 w-4 cursor-nwse-resize rounded-sm text-[10px] leading-none text-zinc-400 hover:text-sky-600 flex items-center justify-center"
        >
          ◢
        </button>
      )}
    </section>
  );
}
