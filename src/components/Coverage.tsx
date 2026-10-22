import { CheckCircle2 } from "lucide-react";
import { cn } from "@/lib/utils";

/** How much of a total there is, as a whole percent: 100 only when it's all there, and never 0 when some is */
export function percentOf(n: number, total: number): number {
  if (total <= 0) return 0;
  if (n >= total) return 100;
  return n <= 0 ? 0 : Math.min(99, Math.max(1, Math.floor((n / total) * 100)));
}

/** "9 of 25 verses · 36%", green with a check when all of them are there, and a bar if asked for */
export function Coverage({
  n,
  total,
  noun = "verses",
  bar = false,
  className,
}: {
  n: number;
  total: number;
  /** What they are ("verses"), or nothing for just "9/25" */
  noun?: string;
  bar?: boolean;
  className?: string;
}) {
  const pct = percentOf(n, total);
  const full = total > 0 && n >= total;
  return (
    <span
      className={cn("inline-flex items-center gap-1.5 tabular-nums", className, full && "font-medium text-emerald-600 dark:text-emerald-400")}
      title={`${n.toLocaleString()} of ${total.toLocaleString()}${noun ? ` ${noun}` : ""} (${((n / (total || 1)) * 100).toFixed(1)}%)`}
    >
      {noun ? `${n.toLocaleString()} of ${total.toLocaleString()} ${noun}` : `${n.toLocaleString()}/${total.toLocaleString()}`}
      <span>· {pct}%</span>
      {full && <CheckCircle2 className="size-3.5" />}
      {bar && (
        <span className="h-1 w-12 overflow-hidden rounded-full bg-muted">
          <span className={cn("block h-full rounded-full", full ? "bg-emerald-500" : "bg-sky-400")} style={{ width: `${pct}%` }} />
        </span>
      )}
    </span>
  );
}
