import { useMemo, useState } from "react";
import { ChevronRight, Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { useApp } from "@/lib/store";
import { chapterStatus, chapterTone, type ChapterStatus, type Tone } from "@/lib/refs";
import { ChapterView } from "./ChapterView";
import { VerseDetail } from "./VerseDetail";

export const toneClass: Record<Tone, string> = {
  done: "bg-emerald-500 text-white border-emerald-600",
  trim: "bg-amber-400 text-amber-950 border-amber-500",
  partial: "bg-background text-foreground border-emerald-500",
  available: "bg-sky-100 text-sky-900 border-sky-300 dark:bg-sky-950 dark:text-sky-200 dark:border-sky-800",
  none: "bg-muted text-muted-foreground/60 border-transparent",
};

function ChapterCell({ status, n, active, onClick }: { status: ChapterStatus; n: number; active: boolean; onClick: () => void }) {
  const tone = chapterTone(status);
  const fill = status.total ? (status.selected / status.total) * 100 : 0;
  return (
    <button
      onClick={onClick}
      title={`Chapter ${n}: ${status.selected}/${status.total} chosen · ${status.withMatches} verses have readings (${status.matches} total)${status.needsTrim ? ` · ${status.needsTrim} to trim` : ""}`}
      className={cn(
        "relative h-7 overflow-hidden rounded-md border text-[11px] font-medium tabular-nums transition hover:brightness-95",
        toneClass[tone],
        active && "ring-2 ring-primary ring-offset-1 ring-offset-background",
      )}
    >
      {tone === "partial" && (
        <span className="absolute inset-x-0 bottom-0 bg-emerald-500/30" style={{ height: `${fill}%` }} />
      )}
      <span className="relative">{n}</span>
      {status.needsTrim > 0 && tone !== "trim" && (
        <span className="absolute top-0.5 right-0.5 size-1.5 rounded-full bg-amber-500" />
      )}
    </button>
  );
}

function Navigator() {
  const { project, outline, verseCounts, book, chapter, go } = useApp();
  const [filter, setFilter] = useState("");
  const [onlyMatched, setOnlyMatched] = useState(true);
  const [expanded, setExpanded] = useState<Set<number>>(() => new Set(book ? [book] : []));

  const statuses = useMemo(() => {
    const map = new Map<number, { chapters: ChapterStatus[]; total: ChapterStatus }>();
    if (!project) return map;
    for (const b of outline) {
      const chapters = b.chapters.map((_, i) => chapterStatus(project, b, i + 1, verseCounts));
      const total = chapters.reduce(
        (a, s) => ({
          total: a.total + s.total,
          selected: a.selected + s.selected,
          withMatches: a.withMatches + s.withMatches,
          needsTrim: a.needsTrim + s.needsTrim,
          matches: a.matches + s.matches,
        }),
        { total: 0, selected: 0, withMatches: 0, needsTrim: 0, matches: 0 },
      );
      map.set(b.id, { chapters, total });
    }
    return map;
  }, [project, outline, verseCounts]);

  const focus = project?.book ?? null;
  const books = outline.filter((b) => {
    if (focus !== null) return b.id === focus;
    const s = statuses.get(b.id)?.total;
    if (onlyMatched && s && s.withMatches === 0 && s.selected === 0) return false;
    return !filter || b.name.toLowerCase().includes(filter.toLowerCase());
  });

  const toggle = (id: number) =>
    setExpanded((e) => {
      const next = new Set(e);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  return (
    <aside className="flex w-72 shrink-0 flex-col border-r">
      <div className={cn("space-y-2 border-b p-2", focus !== null && "hidden")}>
        <div className="relative">
          <Search className="absolute top-2 left-2 size-4 text-muted-foreground" />
          <Input className="pl-8" placeholder="Find a book" value={filter} onChange={(e) => setFilter(e.target.value)} />
        </div>
        <div className="flex items-center gap-2 px-1">
          <Switch id="only" checked={onlyMatched} onCheckedChange={setOnlyMatched} />
          <Label htmlFor="only" className="text-xs font-normal text-muted-foreground">
            Only books with readings
          </Label>
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-0.5 p-2">
          {books.length === 0 && (
            <p className="p-4 text-center text-sm text-muted-foreground">
              {onlyMatched ? "No readings found yet. Add sermons and search them." : "No books"}
            </p>
          )}
          {books.map((b) => {
            const s = statuses.get(b.id)!;
            const open = expanded.has(b.id) || !!filter || focus !== null;
            const pct = s.total.total ? (s.total.selected / s.total.total) * 100 : 0;
            return (
              <div key={b.id}>
                <button
                  onClick={() => focus === null && toggle(b.id)}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-muted",
                    book === b.id && "font-medium",
                  )}
                >
                  {focus === null && (
                    <ChevronRight className={cn("size-3.5 shrink-0 text-muted-foreground transition", open && "rotate-90")} />
                  )}
                  <span className="min-w-0 flex-1 truncate">{b.name}</span>
                  <span className="text-[11px] text-muted-foreground tabular-nums">
                    {s.total.selected}/{s.total.total}
                  </span>
                </button>
                <div className="mx-2 mb-1 h-1 overflow-hidden rounded-full bg-muted">
                  <div className="h-full rounded-full bg-emerald-500" style={{ width: `${pct}%` }} />
                  <div
                    className="-mt-1 h-full rounded-full bg-sky-300/60"
                    style={{ width: `${s.total.total ? (s.total.withMatches / s.total.total) * 100 : 0}%` }}
                  />
                </div>
                {open && (
                  <div className="grid grid-cols-7 gap-1 px-2 pt-1 pb-3">
                    {s.chapters.map((cs, i) => (
                      <ChapterCell
                        key={i}
                        n={i + 1}
                        status={cs}
                        active={book === b.id && chapter === i + 1}
                        onClick={() => go(b.id, i + 1)}
                      />
                    ))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </ScrollArea>
      <div className="grid grid-cols-2 gap-x-3 gap-y-1 border-t p-3 text-[11px] text-muted-foreground">
        {(
          [
            ["done", "Complete"],
            ["partial", "Some chosen"],
            ["trim", "Needs trimming"],
            ["available", "Has readings"],
          ] as const
        ).map(([tone, label]) => (
          <div key={tone} className="flex items-center gap-1.5">
            <span className={cn("size-3 rounded-sm border", toneClass[tone])} /> {label}
          </div>
        ))}
      </div>
    </aside>
  );
}

export function VersesPanel() {
  const { book, chapter, verse } = useApp();
  return (
    <div className="flex h-full">
      <Navigator />
      <div className="min-w-0 flex-1">
        {book && chapter ? (
          <ChapterView key={`${book}.${chapter}`} />
        ) : (
          <div className="p-10 text-center text-muted-foreground">Choose a chapter</div>
        )}
      </div>
      {book && chapter && verse && <VerseDetail key={`${book}.${chapter}.${verse}`} />}
    </div>
  );
}
