import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { btnGhost, btnPrimary } from "../components/ui";

interface Props {
  /** `data-tour` value of the element to highlight. A missing element falls back to a centred card. */
  target: string;
  title: string;
  body: string;
  index: number;
  total: number;
  onNext: () => void;
  onBack: () => void;
  onSkip?: () => void;
  skipLabel?: string;
  nextLabel?: string;
}

interface Box {
  top: number;
  left: number;
  width: number;
  height: number;
}

const PAD = 6;
const POP_W = 340;
const GAP = 18;

export function measure(target: string): Box | null {
  const el = document.querySelector<HTMLElement>(`[data-tour="${target}"]`);
  if (!el) return null;
  const r = el.getBoundingClientRect();
  if (r.width === 0 && r.height === 0) return null;
  return { top: r.top - PAD, left: r.left - PAD, width: r.width + PAD * 2, height: r.height + PAD * 2 };
}

/** Where the popover goes: beside the target when there is room, otherwise below it, otherwise centred. */
export function popoverPosition(box: Box | null, viewport: { w: number; h: number }, popH = 240): { top: number; left: number } {
  if (!box) return { top: Math.max(16, (viewport.h - popH) / 2), left: Math.max(16, (viewport.w - POP_W) / 2) };
  if (box.width > viewport.w * 0.5 && box.height > viewport.h * 0.5) {
    return { top: box.top + Math.min(72, box.height * 0.1), left: Math.max(16, box.left + (box.width - POP_W) / 2) };
  }
  const clampTop = (t: number) => Math.min(Math.max(16, t), viewport.h - popH - 16);
  if (box.left + box.width + GAP + POP_W <= viewport.w - 16) return { top: clampTop(box.top), left: box.left + box.width + GAP };
  if (box.top + box.height + GAP + popH <= viewport.h - 16) return { top: box.top + box.height + GAP, left: Math.min(Math.max(16, box.left), viewport.w - POP_W - 16) };
  return { top: clampTop(box.top - popH - GAP), left: Math.min(Math.max(16, box.left), viewport.w - POP_W - 16) };
}

/** Coach mark: dims the app, cuts a ring around the real control, and explains it next to it. */
export function Spotlight({ target, title, body, index, total, onNext, onBack, onSkip, skipLabel = "Skip setup", nextLabel }: Props) {
  const [box, setBox] = useState<Box | null>(null);
  const [view, setView] = useState({ w: window.innerWidth, h: window.innerHeight });
  const popRef = useRef<HTMLDivElement>(null);
  const [popH, setPopH] = useState(240);

  useLayoutEffect(() => {
    const update = () => {
      setBox(measure(target));
      setView({ w: window.innerWidth, h: window.innerHeight });
    };
    update();
    const timer = window.setInterval(update, 250);
    window.addEventListener("resize", update);
    return () => {
      window.clearInterval(timer);
      window.removeEventListener("resize", update);
    };
  }, [target]);

  useLayoutEffect(() => {
    if (popRef.current) setPopH(popRef.current.offsetHeight);
  }, [title, body, box]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowRight" || e.key === "Enter") {
        e.preventDefault();
        onNext();
      } else if (e.key === "ArrowLeft") onBack();
      else if (e.key === "Escape" && onSkip) onSkip();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onNext, onBack, onSkip]);

  const pos = popoverPosition(box, view, popH);
  const last = index === total - 1;

  return (
    <div className="fixed inset-0 z-40" role="dialog" aria-modal="true" aria-label={`Tour: ${title}`} data-testid="tour-spotlight">
      {box ? (
        <>
          <div
            aria-hidden="true"
            className="spotlight-ring pointer-events-none absolute rounded-xl border-2 border-white"
            style={{ top: box.top, left: box.left, width: box.width, height: box.height, boxShadow: "0 0 0 100vmax rgb(0 0 0 / 0.62), 0 0 28px 2px rgb(255 255 255 / 0.28)" }}
          />
          <button
            type="button"
            aria-label={`${title}. Click to continue`}
            data-testid="tour-target-hit"
            onClick={onNext}
            className="absolute cursor-pointer rounded-xl"
            style={{ top: box.top, left: box.left, width: box.width, height: box.height }}
          />
        </>
      ) : (
        <div aria-hidden="true" className="absolute inset-0 bg-black/60" />
      )}
      <div
        ref={popRef}
        key={target}
        className="coach-pop absolute rounded-2xl border border-zinc-200 bg-[var(--bg)] p-5 shadow-xl dark:border-zinc-700"
        style={{ top: pos.top, left: pos.left, width: POP_W }}
      >
        <div className="mb-3 flex items-center justify-between">
          <span className="eyebrow">
            Tour {index + 1} / {total}
          </span>
          <span aria-hidden="true" className="flex gap-1">
            {Array.from({ length: total }, (_, i) => (
              <span key={i} className={`h-1 rounded-full transition-all ${i === index ? "w-4 bg-zinc-950 dark:bg-zinc-100" : "w-1 bg-zinc-300 dark:bg-zinc-700"}`} />
            ))}
          </span>
        </div>
        <h2 className="mb-1.5 text-lg leading-snug font-semibold">{title}</h2>
        <p className="text-sm leading-relaxed text-zinc-600 dark:text-zinc-400">{body}</p>
        <p className="mt-3 text-xs text-zinc-500">{box ? "Click the highlighted area, or press Enter. Nothing real happens during the tour." : "Nothing real happens during the tour."}</p>
        <div className="mt-4 flex items-center justify-between border-t border-zinc-200 pt-4 dark:border-zinc-800">
          <div className="flex gap-1">
            <button type="button" onClick={onBack} disabled={index === 0} className={btnGhost}>
              Back
            </button>
            {onSkip && (
              <button type="button" onClick={onSkip} className={btnGhost}>
                {skipLabel}
              </button>
            )}
          </div>
          <button type="button" onClick={onNext} className={btnPrimary} autoFocus>
            {nextLabel ?? (last ? "Finish tour" : "Next")}
          </button>
        </div>
      </div>
    </div>
  );
}
