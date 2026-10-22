import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronLeft, ChevronRight, CircleDashed, Combine, Filter, Loader2, Pause, Play, Scissors, SkipBack, SkipForward, Sparkles, Square, Trash2, CheckCircle2, ArrowUpCircle, Link2, Volume2, X } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuLabel, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { api, errorMessage, type Project, type Selection, type VerseTime } from "@/lib/api";
import { partsOf, useApp } from "@/lib/store";
import { play, resume, seek, stop, togglePause, usePlayer } from "@/lib/player";
import { audioPathOf, betterKind, bookName, chosenQuality, formatTime, kindLabel, readingKept, chapterStatus, passageHasChapter, verseKey, versesNeeded, sourceColor, sourceTitle, runLabel } from "@/lib/refs";
import { Coverage, percentOf } from "./Coverage";
import { kindClass } from "./MatchCard";

/** Whether `b` carries on straight from `a` in the same sermon (no cut between them) */
export function continues(a: Selection | undefined, b: Selection | undefined): boolean {
  if (!a || !b) return false;
  const last = partsOf(a).pop()!;
  if (last.sourceId !== b.sourceId) return false;
  return Math.abs(last.end + last.trimEnd - (b.start + b.trimStart)) < 0.6;
}

/** The last chapter rendered, to play again at once when nothing it's made from has changed */
let rendered: { key: string; wav: ArrayBuffer } | null = null;

async function renderChapter(p: Project, book: number, chapter: number): Promise<ArrayBuffer> {
  const prefix = `${book}.${chapter}.`;
  const key = JSON.stringify([
    book,
    chapter,
    Object.entries(p.selections).filter(([k]) => k.startsWith(prefix)),
    p.settings,
    p.sources.map((s) => [s.id, audioPathOf(p, s)]),
  ]);
  if (rendered?.key !== key) rendered = { key, wav: await api.renderChapter(p.id, book, chapter) };
  // Playing takes the bytes it's given, so it gets a copy
  return rendered.wav.slice(0);
}

export function ChapterView() {
  const {
    project,
    outline,
    book,
    chapter,
    verse,
    onlySermon,
    setOnlySermon,
    bookText,
    matchesByVerse,
    verseCounts,
    loadedChapters,
    loadChapter,
    runById,
    matchById,
    go,
    setVerse,
    selectMany,
    update,
    flush,
  } = useApp();
  const player = usePlayer();
  const b = outline.find((x) => x.id === book);
  const text = book ? bookText[book]?.[chapter! - 1] : undefined;

  const verses = useMemo(() => (b && chapter ? versesNeeded(b, chapter, project) : []), [b, chapter, project]);
  const playId = `chapter:${book}.${chapter}`;
  // The verse being heard while the chapter plays
  const [timeline, setTimeline] = useState<{ id: string; times: VerseTime[] } | null>(null);
  const [choosing, setChoosing] = useState(false);
  const playing = player.id === playId && timeline?.id === playId ? timeline.times : null;
  const hearing = playing ? (playing.filter((t) => t.start <= player.time).pop()?.verse ?? null) : null;
  const rows = useRef(new Map<number, HTMLElement>());
  useEffect(() => {
    if (hearing !== null) rows.current.get(hearing)?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [hearing]);
  // Space pauses and resumes the chapter, unless something's being typed
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if (e.key !== " " || usePlayer.getState().id !== playId) return;
      const target = e.target as HTMLElement | null;
      if (target?.closest("input, textarea, select, [contenteditable=true], button, [role=button]")) return;
      e.preventDefault();
      togglePause();
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, [playId]);
  // Each chapter's readings are read when it's shown
  useEffect(() => {
    if (book && chapter) void loadChapter(book, chapter);
  }, [book, chapter, loadChapter]);
  if (!project || !b || !chapter) return null;
  const status = chapterStatus(project, b, chapter, verseCounts);
  const loaded = loadedChapters.has(`${b.id}.${chapter}`);
  // A verse's readings shown: only one sermon's, if it alone is shown
  const shownMatches = (key: string) => {
    const all = (matchesByVerse.get(key) ?? []).filter((m) => readingKept(project, m));
    return onlySermon ? all.filter((m) => m.sourceId === onlySermon) : all;
  };
  const onlySource = onlySermon ? project.sources.find((s) => s.id === onlySermon) : undefined;
  const readHere = onlySermon ? verses.filter((v) => shownMatches(verseKey(b.id, chapter, v)).length > 0).length : 0;

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
    if (!passageHasChapter(project, target.id, c)) return;
    go(target.id, c);
  };

  /**
   * The best reading for each verse with none (of the best kind, then read straight through), in
   * this chapter or every chapter, from the sermon shown alone if one is: one step to undo
   */
  const pick = async (minScore: number, everyChapter: boolean) => {
    setChoosing(true);
    try {
      await flush();
      const picks = await api.bestReadings(project.id, everyChapter ? [] : [{ book: b.id, chapter }], minScore, onlySermon);
      const where = everyChapter ? "" : " here";
      if (picks.length === 0) toast.info(`No more readings to choose${where}`);
      else {
        selectMany(picks);
        const chapters = new Set(picks.map((m) => `${m.book}.${m.chapter}`)).size;
        toast.success(`Chose ${picks.length.toLocaleString()} verse${picks.length === 1 ? "" : "s"}${everyChapter ? ` in ${chapters.toLocaleString()} chapter${chapters === 1 ? "" : "s"}` : ""}`);
      }
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setChoosing(false);
    }
  };

  const clear = () => {
    if (!confirm(`Unchoose every verse in ${b.name} ${chapter}?`)) return;
    update((p) => ({
      ...p,
      selections: Object.fromEntries(Object.entries(p.selections).filter(([k]) => !k.startsWith(`${b.id}.${chapter}.`))),
    }), `unchoose ${b.name} ${chapter}`);
  };

  /** Plays the chapter as it will be exported, from a verse if given (or, if it's playing, goes to it) */
  const playFrom = async (from: number | null) => {
    const startOf = (times: VerseTime[]) => (from === null ? 0 : (times.find((t) => t.verse === from)?.start ?? 0));
    // As it is now, not as it was when last drawn (it may have just been stopped)
    const now = usePlayer.getState();
    if (now.id === playId && timeline?.id === playId) {
      seek(startOf(timeline.times));
      if (now.paused) resume();
      return;
    }
    await flush();
    const p = useApp.getState().project!;
    const times = await api.chapterTimeline(p.id, b.id, chapter).catch(() => [] as VerseTime[]);
    setTimeline({ id: playId, times });
    void play(playId, () => renderChapter(p, b.id, chapter), 0, 0, startOf(times));
  };
  /** Opens the next verse with no reading chosen: after the one open, in this chapter or a later one of the project's */
  const nextToChoose = () => {
    const from = outline.findIndex((x) => x.id === b.id);
    for (const bk of outline.slice(from)) {
      if (project.book !== null && bk.id !== project.book) continue;
      for (let c = bk.id === b.id ? chapter : 1; c <= bk.chapters.length; c++) {
        if (!passageHasChapter(project, bk.id, c)) continue;
        const after = bk.id === b.id && c === chapter ? (verse ?? 0) : 0;
        const next = versesNeeded(bk, c, project).find((v) => v > after && !project.selections[verseKey(bk.id, c, v)]);
        if (next !== undefined) {
          go(bk.id, c, next);
          return;
        }
      }
    }
    toast.success("Every verse after this one has a reading chosen");
  };
  /** Goes to the verse before or after the one heard (before: its start, if it's been heard a while) */
  const jump = (delta: number) => {
    const times = playing ?? [];
    const i = times.findIndex((t) => t.verse === hearing);
    if (i < 0) return;
    const target = delta < 0 && player.time - times[i].start > 2 ? times[i] : times[i + delta];
    if (target) seek(target.start);
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
            {status.selected}/{status.total} chosen · {percentOf(status.selected, status.total)}%
          </Badge>
          <Badge
            variant="outline"
            className={cn(status.total > 0 && status.withMatches === status.total && "border-emerald-500 text-emerald-700 dark:text-emerald-300")}
          >
            {status.withMatches}/{status.total} with readings · {percentOf(status.withMatches, status.total)}%
          </Badge>
          {status.needsTrim > 0 && (
            <Badge variant="outline" className="border-amber-400 text-amber-700 dark:text-amber-300">
              <Scissors /> {status.needsTrim} to trim
            </Badge>
          )}
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          <Button variant="ghost" size="sm" onClick={nextToChoose} title="Open the next verse with no reading chosen">
            <CircleDashed /> Next to choose
          </Button>
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button variant="outline" size="sm" disabled={choosing}>
                {choosing ? <Loader2 className="animate-spin" /> : <Sparkles />} Choose best
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {([false, true] as const).map((every) => (
                <div key={String(every)}>
                  {every && <DropdownMenuSeparator />}
                  <DropdownMenuLabel className="text-xs text-muted-foreground">
                    {every ? "Every chapter" : `${bookName(outline, b.id)} ${chapter}`}
                  </DropdownMenuLabel>
                  <DropdownMenuItem onClick={() => pick(0.9, every)}>Exact readings only</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => pick(0.75, every)}>Exact and close readings</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => pick(0, every)}>Anything found</DropdownMenuItem>
                </div>
              ))}
            </DropdownMenuContent>
          </DropdownMenu>
          {player.id === playId ? (
            <>
              <Button variant="outline" size="sm" onClick={togglePause} title="Space">
                {player.paused ? <Play /> : <Pause />} {player.paused ? "Resume" : "Pause"}
              </Button>
              <Button variant="ghost" size="icon-sm" onClick={stop} title="Stop">
                <Square />
              </Button>
            </>
          ) : (
            <Button variant="outline" size="sm" disabled={status.selected === 0 || player.loading === playId} onClick={() => playFrom(null)}>
              {player.loading === playId ? <Loader2 className="animate-spin" /> : <Play />} Play chapter
            </Button>
          )}
          <Button variant="ghost" size="icon-sm" disabled={status.selected === 0} onClick={clear} title="Unchoose all">
            <Trash2 />
          </Button>
        </div>
      </div>
      {onlySermon && (
        <div className="flex items-center gap-2 border-b bg-muted/40 px-4 py-1.5 text-xs">
          <Filter className="size-3.5 shrink-0 text-muted-foreground" />
          <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, onlySermon) }} />
          <span className="min-w-0 truncate">
            Only readings from <span className="font-medium">{sourceTitle(project, onlySource)}</span>
          </span>
          <span className="shrink-0 text-muted-foreground">
            {loaded ? <Coverage n={readHere} total={verses.length} className="text-muted-foreground" /> : "Reading…"}
          </span>
          <Button size="xs" variant="ghost" className="ml-auto shrink-0" onClick={() => setOnlySermon(null)}>
            <X /> Show every sermon
          </Button>
        </div>
      )}
      {player.id === playId && (
        <div className="flex items-center gap-1.5 border-b px-4 py-1 text-xs">
          <Button size="icon-xs" variant="ghost" onClick={() => jump(-1)} title="The verse before (or this one's start)">
            <SkipBack />
          </Button>
          <Button size="icon-xs" variant="ghost" onClick={togglePause} title={player.paused ? "Resume (Space)" : "Pause (Space)"}>
            {player.paused ? <Play /> : <Pause />}
          </Button>
          <Button size="icon-xs" variant="ghost" onClick={() => jump(1)} title="The next verse">
            <SkipForward />
          </Button>
          <span className="w-24 shrink-0 text-muted-foreground tabular-nums">
            {formatTime(player.time)} / {formatTime(player.duration)}
          </span>
          {/* Clicked to go there; a mark where each verse begins */}
          <div
            className="relative h-2 min-w-0 flex-1 cursor-pointer overflow-hidden rounded-full bg-muted"
            onClick={(e) => {
              const box = e.currentTarget.getBoundingClientRect();
              seek(((e.clientX - box.left) / box.width) * player.duration);
            }}
          >
            <div className="h-full bg-primary" style={{ width: `${player.duration ? (player.time / player.duration) * 100 : 0}%` }} />
            {player.duration > 0 &&
              playing?.map((t) => (
                <span
                  key={t.verse}
                  className="absolute top-0 h-full w-px bg-background/80"
                  style={{ left: `${(t.start / player.duration) * 100}%` }}
                  title={`Verse ${t.verse}`}
                />
              ))}
          </div>
          <span className="w-16 shrink-0 text-right text-muted-foreground">{hearing !== null && `Verse ${hearing}`}</span>
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
              const matches = shownMatches(key);
              // Until the chapter's readings are read, how many there are (of every sermon)
              const found = loaded ? matches.length : onlySermon ? 0 : (verseCounts.get(key) ?? 0);
              const prevSel = i > 0 ? project.selections[verseKey(b.id, chapter, verses[i - 1])] : undefined;
              const nextSel = i < verses.length - 1 ? project.selections[verseKey(b.id, chapter, verses[i + 1])] : undefined;
              const joinUp = continues(prevSel, sel);
              const joinDown = continues(sel, nextSel);
              const color = sel ? sourceColor(project, sel.sourceId) : undefined;
              const source = sel ? project.sources.find((s) => s.id === sel.sourceId) : undefined;
              const chosen = sel ? matchById.get(sel.matchId) : undefined;
              const run = chosen?.runId ? runById.get(chosen.runId) : undefined;
              const best = matches[0];
              const quality = sel ? chosenQuality(sel, matchById) : null;
              // A reading of a better kind, or scoring well above, than the one chosen
              const better = quality && best && (betterKind(best.kind, quality.kind) || best.score - quality.score >= 0.05) ? best : undefined;
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
                      <Link2 className="size-3" /> Read straight through · {sourceTitle(project, source)}
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
                      {hearing === v ? (
                        <Volume2 className="ml-auto size-3.5 text-primary" />
                      ) : sel ? (
                        // The row opens the verse; this plays the chapter from it
                        <span
                          role="button"
                          title="Play the chapter from here"
                          className="inline-flex"
                          onClick={(e) => {
                            e.stopPropagation();
                            void playFrom(v);
                          }}
                        >
                          <span className="group-hover:hidden">{v}</span>
                          <Play className="hidden size-3.5 text-primary group-hover:inline" />
                        </span>
                      ) : (
                        v
                      )}
                    </span>
                    <span className={cn("text-[15px] leading-relaxed", !sel && found === 0 && "text-muted-foreground")}>
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
                          {quality ? (
                            <Tooltip>
                              <TooltipTrigger asChild>
                                <span className={cn("flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px] font-medium whitespace-nowrap tabular-nums", kindClass[quality.kind])}>
                                  <CheckCircle2 className="size-3.5" />
                                  {kindLabel[quality.kind]} · {Math.round(quality.score * 100)}%
                                  {better && <ArrowUpCircle className="size-3.5 text-sky-600 dark:text-sky-400" />}
                                </span>
                              </TooltipTrigger>
                              <TooltipContent>
                                Chosen: {kindLabel[quality.kind].toLowerCase()}, {Math.round(quality.score * 100)}%
                                {quality.joined > 1 && ` (the least of ${quality.joined} readings joined)`}
                                {better && (
                                  <>
                                    <br />A better reading was found: {kindLabel[better.kind].toLowerCase()}, {Math.round(better.score * 100)}%
                                  </>
                                )}
                              </TooltipContent>
                            </Tooltip>
                          ) : (
                            <CheckCircle2 className="size-4 text-emerald-600" />
                          )}
                        </span>
                      ) : null}
                      {found > 0 ? (
                        <span className="flex items-center gap-1 text-[11px] whitespace-nowrap text-muted-foreground">
                          {bestRun && !sel && <Link2 className="size-3" />}
                          <span
                            className={cn(
                              "rounded px-1.5 py-0.5 font-medium tabular-nums",
                              !loaded
                                ? "bg-muted text-muted-foreground"
                                : counts.exact > 0
                                ? "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300"
                                : counts.close > 0
                                  ? "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300"
                                  : "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300",
                            )}
                            title={`${counts.exact} exact · ${counts.close} close · ${counts.partial} partial`}
                          >
                            {found} {found === 1 ? "reading" : "readings"}
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
