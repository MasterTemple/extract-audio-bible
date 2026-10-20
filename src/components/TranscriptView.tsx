import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2, Play, Square } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { api, errorMessage, type Match, type Word } from "@/lib/api";
import { partsOf, useApp } from "@/lib/store";
import { play, sourceTime, stop, usePlayer } from "@/lib/player";
import { audioPathOf, fileIdOf, fileMetadataOf, formatTime, sourceTitle } from "@/lib/refs";

/** How long playing from a word goes on, in seconds */
const PLAY_SECONDS = 300;

type Full = Awaited<ReturnType<typeof api.transcript>>;

/** A reading over some words of the transcript, and whether it's chosen */
interface Mark {
  match: Match;
  chosen: boolean;
  /** Its first word: where its verse is shown */
  first: boolean;
}

interface ParagraphProps {
  from: number;
  words: Word[];
  marks: Map<number, Mark>;
  /** The time being heard, if it's in this paragraph */
  now: number | null;
  /** A word to point out (where the transcript was opened) */
  target: number | null;
  verseLabel: (m: Match) => string;
  onWord: (time: number) => void;
  onVerse: (m: Match) => void;
}

/** A paragraph: its time, and its words, the readings among them marked */
const Paragraph = memo(function Paragraph({ from, words, marks, now, target, verseLabel, onWord, onVerse }: ParagraphProps) {
  return (
    <div className="grid grid-cols-[4.5rem_1fr] gap-3" data-from={from}>
      <button
        className="pt-0.5 text-right text-xs text-muted-foreground tabular-nums hover:text-foreground"
        title="Play from here"
        onClick={() => onWord(words[0].start)}
      >
        {formatTime(words[0].start)}
      </button>
      <p className="text-[15px] leading-relaxed">
        {words.map((w, k) => {
          const i = from + k;
          const mark = marks.get(i);
          const speaking = now !== null && now >= w.start && now < w.end + 0.05;
          return (
            <span key={i}>
              {mark?.first && (
                <button
                  className={cn(
                    "mr-1 rounded px-1 py-px align-[1px] text-[10px] font-semibold",
                    mark.chosen ? "bg-emerald-600 text-white" : "bg-amber-200 text-amber-900 dark:bg-amber-800 dark:text-amber-50",
                  )}
                  title={mark.chosen ? "Chosen for this verse: go to it" : "Found here: go to the verse"}
                  onClick={() => onVerse(mark.match)}
                >
                  {verseLabel(mark.match)}
                </button>
              )}
              <span
                data-word={i}
                onClick={() => onWord(w.start - 0.1)}
                title={`${formatTime(w.start, true)}${w.timed ? "" : " (estimated)"}`}
                className={cn(
                  "cursor-pointer rounded-sm hover:underline",
                  mark && (mark.chosen ? "bg-emerald-100 dark:bg-emerald-950" : "bg-amber-100 dark:bg-amber-950/60"),
                  i === target && "ring-2 ring-primary",
                  speaking && "bg-primary text-primary-foreground",
                )}
              >
                {w.text}
              </span>{" "}
            </span>
          );
        })}
      </p>
    </div>
  );
});

/** A sermon's whole transcript: its readings marked, and played from any word */
export function TranscriptView() {
  const { project, reading, openTranscript, matchSet, outline, go } = useApp();
  const player = usePlayer();
  const source = project?.sources.find((s) => s.id === reading?.sourceId);
  const [data, setData] = useState<{ path: string; full: Full } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const body = useRef<HTMLDivElement>(null);

  const path = source?.transcriptPath;
  useEffect(() => {
    if (!path) return;
    let live = true;
    setError(null);
    api.transcript(path).then(
      (full) => live && setData({ path, full }),
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [path]);
  const full = data?.path === path ? data?.full : undefined;

  // The readings found in this sermon, by word
  const marks = useMemo(() => {
    const out = new Map<number, Mark>();
    if (!project || !source) return out;
    const chosen = new Set(Object.values(project.selections).flatMap((s) => partsOf(s).map((p) => p.matchId)));
    const found = matchSet.matches.filter((m) => m.sourceId === source.id);
    // Chosen ones last, so they win where readings overlap
    found.sort((a, b) => Number(chosen.has(a.id)) - Number(chosen.has(b.id)));
    for (const m of found) {
      for (let i = m.wordStart; i <= m.wordEnd; i++) out.set(i, { match: m, chosen: chosen.has(m.id), first: i === m.wordStart });
    }
    return out;
  }, [project, source, matchSet]);

  const paragraphs = useMemo(() => {
    if (!full) return [];
    const starts = full.paragraphs.length ? full.paragraphs : [0];
    return starts.map((from, k) => ({ from, words: full.words.slice(from, starts[k + 1] ?? full.words.length) })).filter((p) => p.words.length);
  }, [full]);

  // The word at the time it was opened at, scrolled to
  const target = useMemo(() => {
    if (!full || reading?.at == null) return null;
    const i = full.words.findIndex((w) => w.end > reading.at!);
    return i < 0 ? null : i;
  }, [full, reading?.at]);
  useEffect(() => {
    if (target === null) return;
    // After the dialog has laid out
    const id = requestAnimationFrame(() =>
      body.current?.querySelector(`[data-word="${target}"]`)?.scrollIntoView({ block: "center" }),
    );
    return () => cancelAnimationFrame(id);
  }, [target, paragraphs]);

  const playId = source ? `transcript:${source.id}` : "";
  const now = source ? sourceTime(player, playId) : null;
  const audioPath = project && source ? audioPathOf(project, source) : "";
  const onWord = useCallback(
    (time: number) => {
      if (!audioPath) return;
      const start = Math.max(0, time);
      if (usePlayer.getState().id === playId) stop();
      void play(playId, () => api.renderClip({ audioPath, start, end: start + PLAY_SECONDS }), start);
    },
    [audioPath, playId],
  );
  const verseLabel = useCallback(
    (m: Match) => `${outline.find((b) => b.id === m.book)?.osis ?? m.book} ${m.chapter}:${m.verse}`,
    [outline],
  );
  const onVerse = useCallback(
    (m: Match) => {
      openTranscript(null);
      go(m.book, m.chapter, m.verse);
    },
    [go, openTranscript],
  );

  if (!project) return null;
  const meta = source ? fileMetadataOf(project, source) : {};
  const readingCount = new Set([...marks.values()].map((m) => m.match.id)).size;

  return (
    <Dialog open={!!reading} onOpenChange={(open) => !open && openTranscript(null)}>
      <DialogContent className="flex h-[88vh] max-w-4xl flex-col gap-0 p-0 sm:max-w-4xl">
        <DialogHeader className="border-b px-5 py-3">
          <DialogTitle>{source ? sourceTitle(project, source) : "Transcript"}</DialogTitle>
          <DialogDescription className="flex flex-wrap items-center gap-x-3 gap-y-1">
            {source && <span>{fileIdOf(source)}</span>}
            {meta.passage && <span>{meta.passage}</span>}
            {full && <span>{formatTime(full.words[full.words.length - 1]?.end ?? 0)}</span>}
            {full && <span>{full.words.length.toLocaleString()} words</span>}
            <span>{readingCount} readings found</span>
            <span className="flex items-center gap-1">
              <span className="size-2.5 rounded-sm bg-emerald-200 dark:bg-emerald-800" /> chosen
            </span>
            <span className="flex items-center gap-1">
              <span className="size-2.5 rounded-sm bg-amber-200 dark:bg-amber-800" /> found
            </span>
          </DialogDescription>
          {audioPath && (
            <div className="flex items-center gap-2 pt-1 text-xs text-muted-foreground">
              {player.id === playId ? (
                <Button size="xs" variant="outline" onClick={stop}>
                  <Square /> Stop at {formatTime(now ?? 0)}
                </Button>
              ) : player.loading === playId ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Button size="xs" variant="outline" onClick={() => onWord(0)}>
                  <Play /> Play from the start
                </Button>
              )}
              Click any word or time to play from there.
            </div>
          )}
        </DialogHeader>
        <div ref={body} className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4">
          {error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : !full ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Reading the transcript…
            </div>
          ) : (
            paragraphs.map((p) => {
              const last = p.words[p.words.length - 1];
              const inside = now !== null && now >= p.words[0].start - 0.05 && now < last.end + 0.05;
              return (
                <Paragraph
                  key={p.from}
                  from={p.from}
                  words={p.words}
                  marks={marks}
                  now={inside ? now : null}
                  target={target !== null && target >= p.from && target < p.from + p.words.length ? target : null}
                  verseLabel={verseLabel}
                  onWord={onWord}
                  onVerse={onVerse}
                />
              );
            })
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
