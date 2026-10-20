import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, Combine, Loader2, Play, Scissors, Sparkles, Square, Trash2, CheckCircle2, Link2, Volume2 } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { api, type Match, type Selection, type VerseTime } from "@/lib/api";
import { partsOf, useApp } from "@/lib/store";
import { play, stop, usePlayer } from "@/lib/player";
import { bookName, chapterStatus, verseKey, versesNeeded, sourceColor, runLabel } from "@/lib/refs";

/** Whether `b` carries on straight from `a` in the same sermon (no cut between them) */
export function continues(a: Selection | undefined, b: Selection | undefined): boolean {
  if (!a || !b) return false;
  const last = partsOf(a).pop()!;
  if (last.sourceId !== b.sourceId) return false;
  return Math.abs(last.end + last.trimEnd - (b.start + b.trimStart)) < 0.6;
}

/** For each verse without a choice: its best reading, preferring one that carries on from the verse before */
function autoPick(verses: number[], book: number, chapter: number, selections: Record<string, Selection>, matchesByVerse: Map<string, Match[]>, runLength: (id: string | null) => number, minScore: number) {
  const picks: Match[] = [];
  let previous: { runId: string | null; sourceId: string } | null = null;
  for (const v of verses) {
    const key = verseKey(book, chapter, v);
    const existing = selections[key];
    if (existing) {
      // What's heard last is what the next verse would carry on from
      const last = partsOf(existing).pop()!;
      const m = matchesByVerse.get(key)?.find((x) => x.id === last.matchId);
      previous = { runId: m?.runId ?? null, sourceId: last.sourceId };
      continue;
    }
    const candidates = (matchesByVerse.get(key) ?? []).filter((m) => m.score >= minScore);
    let best: Match | null = null;
    let bestScore = -1;
    for (const m of candidates) {
      let s = m.score + Math.min(runLength(m.runId), 10) * 0.01;
      if (previous && m.runId && m.runId === previous.runId) s += 0.15;
      else if (previous && m.sourceId === previous.sourceId) s += 0.03;
      if (s > bestScore) [best, bestScore] = [m, s];
    }
    if (best) picks.push(best);
    previous = best ? { runId: best.runId, sourceId: best.sourceId } : null;
  }
  return picks;
}

export function ChapterView() {
  const { project, outline, book, chapter, verse, bookText, matchesByVerse, runById, matchById, go, setVerse, selectMany, update, flush } =
    useApp();
  const player = usePlayer();
  const b = outline.find((x) => x.id === book);
  const text = book ? bookText[book]?.[chapter! - 1] : undefined;

  const verses = useMemo(() => (b && chapter ? versesNeeded(b, chapter) : []), [b, chapter]);
  const playId = `chapter:${book}.${chapter}`;
  // The verse being heard while the chapter plays
  const [timeline, setTimeline] = useState<{ id: string; times: VerseTime[] } | null>(null);
  const playing = player.id === playId && timeline?.id === playId ? timeline.times : null;
  const hearing = playing ? (playing.filter((t) => t.start <= player.time).pop()?.verse ?? null) : null;
  const rows = useRef(new Map<number, HTMLElement>());
  useEffect(() => {
    if (hearing !== null) rows.current.get(hearing)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [hearing]);
  if (!project || !b || !chapter) return null;
  const status = chapterStatus(project, b, chapter, matchesByVerse);

  const nav = (delta: number) => {
    const idx = outline.findIndex((x) => x.id === b.id);
    let c = chapter + delta;
    let target = b;
    if (c < 1) {
      target = outline[idx - 1];
      if (!target) return;
      c = target.chapters.length;
    } else if (c > b.chapters.length) {
      target = outline[idx + 1];
      if (!target) return;
      c = 1;
    }
    if (project.book !== null && target.id !== project.book) return;
    go(target.id, c);
  };

  const runLength = (id: string | null) => (id ? runById.get(id)?.matchIds.length ?? 0 : 0);
  const pick = (minScore: number) => {
    const picks = autoPick(verses, b.id, chapter, project.selections, matchesByVerse, runLength, minScore);
    if (picks.length === 0) toast.info("No more readings to choose here");
    else {
      selectMany(picks);
      toast.success(`Chose ${picks.length} verse${picks.length === 1 ? "" : "s"}`);
    }
  };

  const clear = () => {
    if (!confirm(`Unchoose every verse in ${b.name} ${chapter}?`)) return;
    update((p) => ({
      ...p,
      selections: Object.fromEntries(Object.entries(p.selections).filter(([k]) => !k.startsWith(`${b.id}.${chapter}.`))),
    }));
  };

  const preview = async () => {
    await flush();
    const p = useApp.getState().project!;
    if (player.id !== playId) {
      api.chapterTimeline(p, b.id, chapter).then(
        (times) => setTimeline({ id: playId, times }),
        () => setTimeline(null),
      );
    }
    void play(playId, () => api.renderChapter(p, b.id, chapter));
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex items-center gap-2 border-b px-4 py-2">
        <Button variant="ghost" size="icon-sm" onClick={() => nav(-1)}>
          <ChevronLeft />
        </Button>
        <h2 className="text-lg font-semibold">
          {bookName(outline, b.id)} {chapter}
        </h2>
        <Button variant="ghost" size="icon-sm" onClick={() => nav(1)}>
          <ChevronRight />
        </Button>
        <div className="ml-2 flex items-center gap-1.5 text-xs">
          <Badge variant={status.selected === status.total ? "default" : "secondary"} className={cn(status.selected === status.total && "bg-emerald-600")}>
            {status.selected}/{status.total} chosen
          </Badge>
          <Badge variant="outline">{status.withMatches} with readings</Badge>
          {status.needsTrim > 0 && (
            <Badge variant="outline" className="border-amber-400 text-amber-700 dark:text-amber-300">
              <Scissors /> {status.needsTrim} to trim
            </Badge>
          )}
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm">
                <Sparkles /> Choose best
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => pick(0.9)}>Exact readings only</DropdownMenuItem>
              <DropdownMenuItem onClick={() => pick(0.75)}>Exact and close readings</DropdownMenuItem>
              <DropdownMenuItem onClick={() => pick(0)}>Anything found</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
          <Button variant="outline" size="sm" disabled={status.selected === 0} onClick={preview}>
            {player.loading === playId ? <Loader2 className="animate-spin" /> : player.id === playId ? <Square /> : <Play />}
            {player.id === playId ? "Stop" : "Play chapter"}
          </Button>
          <Button variant="ghost" size="icon-sm" disabled={status.selected === 0} onClick={clear} title="Unchoose all">
            <Trash2 />
          </Button>
        </div>
      </div>
      {player.id === playId && (
        <div className="h-0.5 bg-muted">
          <div className="h-full bg-primary" style={{ width: `${player.duration ? (player.time / player.duration) * 100 : 0}%` }} />
        </div>
      )}
      <ScrollArea className="min-h-0 flex-1">
        <div className="mx-auto max-w-3xl py-3 pr-4 pl-2">
          {!text && (
            <div className="flex justify-center p-10">
              <Loader2 className="animate-spin text-muted-foreground" />
            </div>
          )}
          {text &&
            verses.map((v, i) => {
              const key = verseKey(b.id, chapter, v);
              const sel = project.selections[key];
              const matches = matchesByVerse.get(key) ?? [];
              const prevSel = i > 0 ? project.selections[verseKey(b.id, chapter, verses[i - 1])] : undefined;
              const nextSel = i < verses.length - 1 ? project.selections[verseKey(b.id, chapter, verses[i + 1])] : undefined;
              const joinUp = continues(prevSel, sel);
              const joinDown = continues(sel, nextSel);
              const color = sel ? sourceColor(project, sel.sourceId) : undefined;
              const source = sel ? project.sources.find((s) => s.id === sel.sourceId) : undefined;
              const chosen = sel ? matchById.get(sel.matchId) : undefined;
              const run = chosen?.runId ? runById.get(chosen.runId) : undefined;
              const best = matches[0];
              const bestRun = best?.runId ? runById.get(best.runId) : undefined;
              const counts = { exact: 0, close: 0, partial: 0 };
              for (const m of matches) counts[m.kind]++;
              return (
                <div
                  key={v}
                  ref={(el) => {
                    if (el) rows.current.set(v, el);
                    else rows.current.delete(v);
                  }}
                >
                  {sel && !joinUp && joinDown && (
                    <div className="mt-2 mb-0.5 ml-6 flex items-center gap-1.5 text-[11px] font-medium" style={{ color }}>
                      <Link2 className="size-3" /> Read straight through · {source?.label}
                      {run && <span className="font-normal text-muted-foreground">({runLabel(outline, run)} in this sermon)</span>}
                    </div>
                  )}
                  <button
                    onClick={() => setVerse(verse === v ? null : v)}
                    className={cn(
                      "group grid w-full grid-cols-[6px_2rem_1fr_auto] items-start gap-x-2 rounded-md py-1.5 pr-2 text-left hover:bg-muted/60",
                      verse === v && "bg-muted",
                      hearing === v && "bg-primary/10 ring-1 ring-primary/30",
                    )}
                  >
                    <span
                      className={cn("w-1.5 self-stretch", !joinUp && "mt-1 rounded-t-full", !joinDown && "mb-1 rounded-b-full", joinUp && "-mt-1.5", joinDown && "-mb-1.5")}
                      style={{ background: color ?? "transparent" }}
                    />
                    <span className="pt-0.5 text-right text-xs font-semibold text-muted-foreground tabular-nums">
                      {hearing === v ? <Volume2 className="ml-auto size-3.5 text-primary" /> : v}
                    </span>
                    <span className={cn("text-[15px] leading-relaxed", !sel && matches.length === 0 && "text-muted-foreground")}>
                      {text[v - 1]}
                    </span>
                    <span className="flex flex-col items-end gap-1 pt-0.5">
                      {sel ? (
                        <span className="flex items-center gap-1">
                          {sel.gainDb !== 0 && (
                            <span className="text-[10px] text-muted-foreground tabular-nums">
                              {sel.gainDb > 0 ? "+" : ""}
                              {sel.gainDb} dB
                            </span>
                          )}
                          {sel.joined && sel.joined.length > 0 && (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <span className="flex items-center gap-0.5 text-[10px] text-muted-foreground tabular-nums">
                                  <Combine className="size-3.5" />
                                  {sel.joined.length + 1}
                                </span>
                              </TooltipTrigger>
                              <TooltipContent>{sel.joined.length + 1} readings joined</TooltipContent>
                            </Tooltip>
                          )}
                          {sel.needsTrim && (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <Scissors className="size-4 text-amber-500" />
                              </TooltipTrigger>
                              <TooltipContent>Marked to trim</TooltipContent>
                            </Tooltip>
                          )}
                          <CheckCircle2 className="size-4 text-emerald-600" />
                        </span>
                      ) : null}
                      {matches.length > 0 ? (
                        <span className="flex items-center gap-1 text-[11px] whitespace-nowrap text-muted-foreground">
                          {bestRun && !sel && <Link2 className="size-3" />}
                          <span
                            className={cn(
                              "rounded px-1.5 py-0.5 font-medium tabular-nums",
                              counts.exact > 0
                                ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300"
                                : counts.close > 0
                                  ? "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300"
                                  : "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300",
                            )}
                            title={`${counts.exact} exact · ${counts.close} close · ${counts.partial} partial`}
                          >
                            {matches.length} {matches.length === 1 ? "reading" : "readings"}
                          </span>
                        </span>
                      ) : (
                        !sel && <span className="text-[11px] text-muted-foreground/60">none found</span>
                      )}
                    </span>
                  </button>
                </div>
              );
            })}
        </div>
      </ScrollArea>
      {player.id && player.id !== playId && (
        <div className="flex items-center justify-between border-t bg-muted/40 px-4 py-1.5 text-xs">
          <span>Playing…</span>
          <Button size="xs" variant="ghost" onClick={stop}>
            <Square /> Stop
          </Button>
        </div>
      )}
    </div>
  );
}
