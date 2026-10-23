import { useEffect, useMemo, useRef, useState } from "react";
import { Play, Scissors, SquareSplitHorizontal, Undo2, X } from "lucide-react";
import type { Cut, Word } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface Props {
  peaks: number[];
  /** The time of the first peak */
  peaksStart: number;
  perSecond: number;
  /** The window shown */
  start: number;
  end: number;
  clipStart: number;
  clipEnd: number;
  now: number | null;
  words: Word[];
  onChange: (edge: "start" | "end", time: number) => void;
  /** When a bound is picked up (true) and let go (false), to keep the window still meanwhile */
  onDrag?: (dragging: boolean) => void;
  onWord?: (w: Word, e: React.MouseEvent) => void;
  /** Stretches cut out of the clip, shown struck out */
  cuts?: Cut[];
  /** Given, a stretch dragged across can be cut out */
  onCut?: (cut: Cut) => void;
  /** Puts back a cut, by its index in `cuts` */
  onUncut?: (index: number) => void;
  /** A cut's bounds dragged, by its index in `cuts` */
  onCutChange?: (index: number, cut: Cut) => void;
  /** Clicked (not dragged across): the time clicked, to play from */
  onSeek?: (time: number) => void;
  /** Hears a stretch marked: it alone, or the second either side of it joined as they'd be without it */
  onPreview?: (cut: Cut, how: "cut" | "join") => void;
  /** The waveform's height, in pixels */
  height?: number;
}

const DEFAULT_HEIGHT = 120;
/** Rows of words under the waveform, and each one's height */
const ROWS = 3;
const ROW_HEIGHT = 20;

let measurer: CanvasRenderingContext2D | null = null;
/** How wide a word's label is, in pixels (12px text, its padding and border) */
function labelWidth(text: string): number {
  measurer ??= document.createElement("canvas").getContext("2d");
  if (!measurer) return text.length * 7 + 6;
  measurer.font = `12px ${getComputedStyle(document.body).fontFamily}`;
  return Math.ceil(measurer.measureText(text).width) + 6;
}

interface Label {
  word: Word;
  row: number;
  left: number;
  width: number;
}

/**
Places each word's label at its start in the first row with room; when no row has, the label
before it in the row that frees up first is shortened to make room, so no two overlap
*/
function layout(words: Word[], x: (t: number) => number): Label[] {
  const last: (Label | null)[] = Array(ROWS).fill(null);
  const labels: Label[] = [];
  for (const word of words) {
    const left = x(word.start);
    const end = (l: Label | null) => (l ? l.left + l.width : -Infinity);
    let row = last.findIndex((l) => end(l) <= left);
    if (row < 0) {
      row = last.reduce((best, l, i) => (end(l) < end(last[best]) ? i : best), 0);
      const before = last[row]!;
      before.width = Math.max(0, left - before.left - 1);
    }
    const label = { word, row, left, width: labelWidth(word.text) };
    labels.push(label);
    last[row] = label;
  }
  return labels;
}

/** A bound that can be dragged: the clip's start or end, a cut's, or the stretch marked's */
type Bound = { of: "clip"; edge: "start" | "end" } | { of: "cut"; index: number; edge: "start" | "end" } | { of: "mark"; edge: "from" | "to" };

/** How near (in pixels) a press must be to a bound to pick it up */
const GRAB = 10;

/** A sermon's waveform around a clip, with its bounds (and its cuts') to drag */
export function Waveform({
  peaks,
  peaksStart,
  perSecond,
  start,
  end,
  clipStart,
  clipEnd,
  now,
  words,
  onChange,
  onDrag,
  onWord,
  cuts = [],
  onCut,
  onUncut,
  onCutChange,
  onSeek,
  onPreview,
  height = DEFAULT_HEIGHT,
}: Props) {
  const cutsKey = JSON.stringify(cuts);
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(480);
  const [drag, setDragState] = useState<Bound | null>(null);
  // The bound under the pointer, to show it can be dragged
  const [hover, setHover] = useState<Bound | null>(null);
  // A stretch being marked (dragged across), or marked, to cut out: where it began and where it is now
  const [marked, setMarked] = useState<{ from: number; to: number; marking: boolean } | null>(null);
  const isCut = (time: number) => cuts.some((c) => time >= Math.min(c.start, c.end) && time <= Math.max(c.start, c.end));
  const setDrag = (bound: Bound | null) => {
    if ((bound !== null) !== (drag !== null)) onDrag?.(bound !== null);
    setDragState(bound);
  };
  const span = Math.max(0.1, end - start);
  const x = (t: number) => ((t - start) / span) * width;
  const t = (px: number) => start + (px / width) * span;

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setWidth(el.clientWidth));
    ro.observe(el);
    setWidth(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = width * dpr;
    c.height = height * dpr;
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, height);
    const [inner, outer] = ["#059669", "#94a3b8"];
    const max = Math.max(0.05, ...peaks);
    const mid = height / 2;
    for (let px = 0; px < width; px++) {
      const t0 = t(px);
      const t1 = t(px + 1);
      const i0 = Math.floor((t0 - peaksStart) * perSecond);
      const i1 = Math.max(i0 + 1, Math.ceil((t1 - peaksStart) * perSecond));
      let p = 0;
      for (let i = Math.max(0, i0); i < Math.min(peaks.length, i1); i++) p = Math.max(p, peaks[i]);
      const h = Math.max(1, (p / max) * (mid - 2));
      const inside = t0 >= clipStart && t0 <= clipEnd && !isCut(t0);
      ctx.fillStyle = inside ? inner : outer;
      ctx.globalAlpha = inside ? 0.85 : 0.5;
      ctx.fillRect(px, mid - h, 1, h * 2);
    }
    ctx.globalAlpha = 1;
  }, [peaks, peaksStart, perSecond, width, start, end, clipStart, clipEnd, height, cutsKey]);

  const labels = useMemo(
    () => layout(words.filter((w) => w.end >= start && w.start <= end), (time) => ((time - start) / span) * width),
    [words, start, end, span, width],
  );

  const mark = marked && { from: Math.min(marked.from, marked.to), to: Math.max(marked.from, marked.to) };
  const px = (e: React.PointerEvent) => e.clientX - wrap.current!.getBoundingClientRect().left;
  /** The time under the pointer, within the window, or (`beyond`) past it too, as when a clip's bound is dragged out of it */
  const at = (e: React.PointerEvent, beyond = false) => Math.max(0, t(beyond ? px(e) : Math.min(width, Math.max(0, px(e)))));
  /** The bound nearest a point, if one's near enough to pick up: the clip's first, then a marked stretch's, then a cut's */
  const boundAt = (point: number): Bound | null => {
    const near: [Bound, number][] = [
      [{ of: "clip", edge: "start" }, clipStart],
      [{ of: "clip", edge: "end" }, clipEnd],
    ];
    if (mark && !marked.marking)
      near.push(
        [{ of: "mark", edge: "from" }, mark.from],
        [{ of: "mark", edge: "to" }, mark.to],
      );
    if (onCutChange)
      cuts.forEach((c, index) =>
        near.push(
          [{ of: "cut", index, edge: "start" }, c.start],
          [{ of: "cut", index, edge: "end" }, c.end],
        ),
      );
    let best: Bound | null = null;
    let distance = GRAB;
    for (const [bound, time] of near) {
      const d = Math.abs(x(time) - point);
      if (d <= distance) [best, distance] = [bound, d];
    }
    return best;
  };
  const same = (a: Bound | null, b: Bound | null) => JSON.stringify(a) === JSON.stringify(b);
  const onDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const bound = boundAt(px(e));
    if (bound) {
      // A marked stretch's other end stays where it is, its `from`; the end picked up moves
      if (bound.of === "mark" && mark) setMarked(bound.edge === "from" ? { from: mark.to, to: mark.from, marking: false } : { from: mark.from, to: mark.to, marking: false });
      setDrag(bound);
      return;
    }
    if (!onCut && !onSeek) return;
    const time = at(e);
    setMarked({ from: time, to: time, marking: true });
  };
  const onMove = (e: React.PointerEvent) => {
    if (!drag) {
      if (marked?.marking) setMarked({ ...marked, to: at(e) });
      else {
        const bound = boundAt(px(e));
        if (!same(bound, hover)) setHover(bound);
      }
      return;
    }
    if (drag.of === "clip") onChange(drag.edge, at(e, true));
    else if (drag.of === "mark") setMarked(marked && { ...marked, to: at(e) });
    else {
      const c = cuts[drag.index];
      if (!c || !onCutChange) return;
      const time = at(e);
      onCutChange(drag.index, drag.edge === "start" ? { start: Math.min(time, c.end - 0.02), end: c.end } : { start: c.start, end: Math.max(time, c.start + 0.02) });
    }
  };
  const onUp = () => {
    setDrag(null);
    if (!marked?.marking) return;
    // A click, not a drag, marks nothing: it plays from there
    if (Math.abs(marked.to - marked.from) * (width / span) < 4) {
      setMarked(null);
      onSeek?.(marked.from);
    } else setMarked({ ...marked, marking: false });
  };
  const resizing = drag ?? hover;

  return (
    <div className="space-y-1 select-none">
      <div
        ref={wrap}
        className="relative overflow-hidden rounded-md border bg-background"
        style={{ height }}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        onPointerLeave={() => !drag && !marked?.marking && setHover(null)}
      >
        <canvas
          ref={canvas}
          style={{ width, height }}
          className={cn("absolute inset-0", resizing ? "cursor-ew-resize" : (onCut || onSeek) && "cursor-crosshair")}
        />
        <div className="pointer-events-none absolute inset-y-0 bg-primary/8" style={{ left: x(clipStart), width: Math.max(0, x(clipEnd) - x(clipStart)) }} />
        {cuts.map((c, i) => {
          const [a, b] = [Math.min(c.start, c.end), Math.max(c.start, c.end)];
          return (
            <div
              key={i}
              className="absolute inset-y-0 border-x border-red-500/70 bg-[repeating-linear-gradient(135deg,transparent_0_5px,rgb(239_68_68/0.18)_5px_10px)]"
              style={{ left: x(a), width: Math.max(1, x(b) - x(a)) }}
              title={`Cut out: ${a.toFixed(2)}–${b.toFixed(2)}s (${(b - a).toFixed(2)}s)`}
            >
              {onUncut && (
                <button
                  className="absolute bottom-1 left-1/2 flex -translate-x-1/2 items-center rounded-sm bg-background/90 p-0.5 text-red-600 shadow-sm hover:bg-background"
                  title="Put it back"
                  onPointerDown={(e) => e.stopPropagation()}
                  onClick={() => onUncut(i)}
                >
                  <Undo2 className="size-3" />
                </button>
              )}
            </div>
          );
        })}
        {mark && (
          <div className="pointer-events-none absolute inset-y-0 border-x border-red-500 bg-red-500/15" style={{ left: x(mark.from), width: Math.max(1, x(mark.to) - x(mark.from)) }} />
        )}
        {mark && !marked.marking && onCut && (
          <div
            className="absolute top-1 z-20 flex -translate-x-1/2 items-center gap-0.5 rounded-md border bg-background p-0.5 shadow-sm"
            style={{ left: Math.min(width - 150, Math.max(150, x((mark.from + mark.to) / 2))) }}
            onPointerDown={(e) => e.stopPropagation()}
          >
            {onPreview && (
              <>
                <Button size="icon-xs" variant="ghost" title="Hear what's marked" onClick={() => onPreview({ start: mark.from, end: mark.to }, "cut")}>
                  <Play />
                </Button>
                <Button
                  size="xs"
                  variant="ghost"
                  title="Hear a second either side, joined as they'd be without it"
                  onClick={() => onPreview({ start: mark.from, end: mark.to }, "join")}
                >
                  <SquareSplitHorizontal /> Without it
                </Button>
              </>
            )}
            <Button
              size="xs"
              variant="destructive"
              onClick={() => {
                onCut({ start: mark.from, end: mark.to });
                setMarked(null);
              }}
            >
              <Scissors /> Cut out {(mark.to - mark.from).toFixed(2)}s
            </Button>
            <Button size="icon-xs" variant="ghost" title="Unmark" onClick={() => setMarked(null)}>
              <X />
            </Button>
          </div>
        )}
        {(["start", "end"] as const).map((edge) => {
          const active = resizing?.of === "clip" && resizing.edge === edge;
          return (
            // Drawn only: a press near it picks it up (`boundAt`)
            <div key={edge} className="pointer-events-none absolute inset-y-0 -ml-2 w-4" style={{ left: x(edge === "start" ? clipStart : clipEnd) }}>
              <div className={cn("mx-auto h-full bg-emerald-600", active ? "w-1" : "w-0.5")} />
              <div
                className={cn(
                  "absolute top-0 h-5 w-3 bg-emerald-600",
                  edge === "start" ? "left-1/2 rounded-r-sm" : "right-1/2 rounded-l-sm",
                )}
              />
              <div
                className={cn(
                  "absolute top-1/2 left-1/2 flex h-8 w-2.5 -translate-x-1/2 -translate-y-1/2 flex-col items-center justify-center gap-0.5 rounded-sm bg-emerald-600 shadow-sm",
                  active && "ring-2 ring-emerald-300",
                )}
              >
                <span className="h-px w-1.5 bg-white/80" />
                <span className="h-px w-1.5 bg-white/80" />
                <span className="h-px w-1.5 bg-white/80" />
              </div>
            </div>
          );
        })}
        {now !== null && now >= start && now <= end && (
          <div className="pointer-events-none absolute inset-y-0 w-px bg-red-500" style={{ left: x(now) }} />
        )}
      </div>
      <div className="relative overflow-hidden text-xs" style={{ height: ROWS * ROW_HEIGHT }}>
        {labels.map(({ word: w, row, left, width: labelW }, i) => {
          const inside = w.start >= clipStart - 0.01 && w.end <= clipEnd + 0.01;
          const cut = isCut((w.start + w.end) / 2);
          const speaking = now !== null && now >= w.start && now < w.end;
          return (
            <span
              key={i}
              onClick={onWord ? (e) => onWord(w, e) : undefined}
              title={`${w.text} · ${w.start.toFixed(2)}s`}
              className={cn(
                "absolute cursor-pointer truncate rounded-sm border-l px-0.5 hover:z-10 hover:w-auto! hover:bg-muted",
                inside ? "border-emerald-500 text-foreground" : "border-border text-muted-foreground",
                cut && "border-red-400 text-red-600/80 line-through dark:text-red-400/80",
                speaking && "bg-primary text-primary-foreground",
              )}
              style={{ left, top: row * ROW_HEIGHT, width: labelW, height: ROW_HEIGHT - 2, lineHeight: `${ROW_HEIGHT - 2}px` }}
            >
              {w.text}
            </span>
          );
        })}
      </div>
    </div>
  );
}
