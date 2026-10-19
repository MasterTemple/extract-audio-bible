import { useEffect, useRef, useState } from "react";
import type { Word } from "@/lib/api";
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
  onWord?: (w: Word, e: React.MouseEvent) => void;
}

const HEIGHT = 80;

/** A sermon's waveform around a clip, with handles to drag its start and end */
export function Waveform({ peaks, peaksStart, perSecond, start, end, clipStart, clipEnd, now, words, onChange, onWord }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(480);
  const [drag, setDrag] = useState<"start" | "end" | null>(null);
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
    c.height = HEIGHT * dpr;
    const ctx = c.getContext("2d")!;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, width, HEIGHT);
    const [inner, outer] = ["#059669", "#94a3b8"];
    const max = Math.max(0.05, ...peaks);
    const mid = HEIGHT / 2;
    for (let px = 0; px < width; px++) {
      const t0 = t(px);
      const t1 = t(px + 1);
      const i0 = Math.floor((t0 - peaksStart) * perSecond);
      const i1 = Math.max(i0 + 1, Math.ceil((t1 - peaksStart) * perSecond));
      let p = 0;
      for (let i = Math.max(0, i0); i < Math.min(peaks.length, i1); i++) p = Math.max(p, peaks[i]);
      const h = Math.max(1, (p / max) * (mid - 2));
      const inside = t0 >= clipStart && t0 <= clipEnd;
      ctx.fillStyle = inside ? inner : outer;
      ctx.globalAlpha = inside ? 0.85 : 0.5;
      ctx.fillRect(px, mid - h, 1, h * 2);
    }
    ctx.globalAlpha = 1;
  }, [peaks, peaksStart, perSecond, width, start, end, clipStart, clipEnd]);

  const onMove = (e: React.PointerEvent) => {
    if (!drag) return;
    const rect = wrap.current!.getBoundingClientRect();
    onChange(drag, t(Math.min(width, Math.max(0, e.clientX - rect.left))));
  };

  return (
    <div className="space-y-1 select-none">
      <div
        ref={wrap}
        className="relative overflow-hidden rounded-md border bg-background"
        style={{ height: HEIGHT }}
        onPointerMove={onMove}
        onPointerUp={() => setDrag(null)}
        onPointerLeave={() => setDrag(null)}
      >
        <canvas ref={canvas} style={{ width, height: HEIGHT }} className="absolute inset-0" />
        <div className="absolute inset-y-0 bg-primary/8" style={{ left: x(clipStart), width: Math.max(0, x(clipEnd) - x(clipStart)) }} />
        {(["start", "end"] as const).map((edge) => (
          <div
            key={edge}
            onPointerDown={(e) => {
              (e.target as HTMLElement).setPointerCapture(e.pointerId);
              setDrag(edge);
            }}
            className="group absolute inset-y-0 -ml-2 w-4 cursor-ew-resize"
            style={{ left: x(edge === "start" ? clipStart : clipEnd) }}
          >
            <div className={cn("mx-auto h-full w-0.5 bg-emerald-600", drag === edge && "w-1")} />
            <div
              className={cn(
                "absolute top-0 h-4 w-2.5 bg-emerald-600",
                edge === "start" ? "left-1/2 rounded-r-sm" : "right-1/2 rounded-l-sm",
              )}
            />
          </div>
        ))}
        {now !== null && now >= start && now <= end && (
          <div className="pointer-events-none absolute inset-y-0 w-px bg-red-500" style={{ left: x(now) }} />
        )}
      </div>
      <div className="relative h-9 overflow-hidden text-[10px]">
        {words
          .filter((w) => w.end >= start && w.start <= end)
          .map((w, i) => {
            const inside = w.start >= clipStart - 0.01 && w.end <= clipEnd + 0.01;
            const speaking = now !== null && now >= w.start && now < w.end;
            return (
              <span
                key={i}
                onClick={onWord ? (e) => onWord(w, e) : undefined}
                title={`${w.text} · ${w.start.toFixed(2)}s`}
                className={cn(
                  "absolute cursor-pointer truncate rounded-sm border-l px-0.5 leading-4 hover:bg-muted",
                  i % 2 ? "top-4" : "top-0",
                  inside ? "border-emerald-500 text-foreground" : "border-border text-muted-foreground",
                  speaking && "bg-primary text-primary-foreground",
                )}
                style={{ left: x(w.start), maxWidth: Math.max(14, x(w.end) - x(w.start) + 40) }}
              >
                {w.text}
              </span>
            );
          })}
      </div>
    </div>
  );
}
