import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown, ChevronUp, Loader2, Play, Search, Square } from "lucide-react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api, errorMessage, type Match, type Said, type Word } from "@/lib/api";
import { partsOf, useApp } from "@/lib/store";
import { play, sourceTime, stop, usePlayer } from "@/lib/player";
import { audioPathOf, fileIdOf, fileMetadataOf, formatTime, sourceTitle } from "@/lib/refs";

/** How long playing from a word goes on, in seconds */
const PLAY_SECONDS = 300;

/** The paragraph (by index) a word is in: the last to begin at or before it */
function paragraphOf(paragraphs: { from: number }[], word: number): number {
  let [lo, hi] = [0, paragraphs.length - 1];
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (paragraphs[mid].from <= word) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

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
  /** Words to point out (where the transcript was opened, or the place found), if in this paragraph */
  target: [number, number] | null;
  /** The words of every place found, by word */
  found: Set<number>;
  verseLabel: (m: Match) => string;
  onWord: (time: number) => void;
  onVerse: (m: Match) => void;
}

/** A paragraph: its time, and its words, the readings among them marked */
const Paragraph = memo(function Paragraph({ from, words, marks, now, target, found, verseLabel, onWord, onVerse }: ParagraphProps) {
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
                  found.has(i) && "bg-yellow-200 dark:bg-yellow-700/60",
                  target && i >= target[0] && i <= target[1] && "ring-2 ring-primary",
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
  const { project, reading, openTranscript, summary, outline, go } = useApp();
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

  // This sermon's readings, read again after a search
  const projectId = project?.id;
  const openId = source?.id;
  const [readings, setReadings] = useState<{ sourceId: string; matches: Match[] } | null>(null);
  useEffect(() => {
    if (!projectId || !openId) return;
    let live = true;
    api.sermonMatches(projectId, openId).then(
      (set) => live && setReadings({ sourceId: openId, matches: set.matches }),
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [projectId, openId, summary]);

  // The readings found in this sermon, by word
  const marks = useMemo(() => {
    const out = new Map<number, Mark>();
    if (!project || !source) return out;
    const chosen = new Set(Object.values(project.selections).flatMap((s) => partsOf(s).map((p) => p.matchId)));
    const these = readings?.sourceId === source.id ? [...readings.matches] : [];
    // Chosen ones last, so they win where readings overlap
    these.sort((a, b) => Number(chosen.has(a.id)) - Number(chosen.has(b.id)));
    for (const m of these) {
      for (let i = m.wordStart; i <= m.wordEnd; i++) out.set(i, { match: m, chosen: chosen.has(m.id), first: i === m.wordStart });
    }
    return out;
  }, [project, source, readings]);

  const paragraphs = useMemo(() => {
    if (!full) return [];
    const starts = full.paragraphs.length ? full.paragraphs : [0];
    return starts.map((from, k) => ({ from, words: full.words.slice(from, starts[k + 1] ?? full.words.length) })).filter((p) => p.words.length);
  }, [full]);

  // Finding a phrase in it: every place, and the one shown
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Said[]>([]);
  const [current, setCurrent] = useState(-1);
  const findBox = useRef<HTMLInputElement>(null);
  useEffect(() => setQuery(reading?.query ?? ""), [reading]);
  const sourceId = source?.id;
  useEffect(() => {
    const p = useApp.getState().project;
    if (!p || !sourceId || !query.trim()) {
      setHits([]);
      setCurrent(-1);
      return;
    }
    let live = true;
    const timer = setTimeout(() => {
      api.searchTranscripts(p.id, query, [sourceId], 5000, { key: "transcript" }).then(
        (r) => {
          if (!live) return;
          setHits(r.found);
          // The first place at or after where it was opened
          const at = useApp.getState().reading?.at ?? 0;
          const first = r.found.findIndex((s) => s.end >= at);
          setCurrent(r.found.length ? Math.max(0, first) : -1);
        },
        () => live && setHits([]),
      );
    }, 150);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, sourceId]);
  const found = useMemo(() => {
    const words = new Set<number>();
    for (const s of hits) for (let i = s.wordStart; i <= s.wordEnd; i++) words.add(i);
    return words;
  }, [hits]);
  const step = (by: number) => hits.length && setCurrent((c) => (c + by + hits.length) % hits.length);

  // What to point out and scroll to: the place found, or the word at the time it was opened at
  const target = useMemo((): [number, number] | null => {
    const hit = hits[current];
    if (hit) return [hit.wordStart, hit.wordEnd];
    if (!full || reading?.at == null) return null;
    const i = full.words.findIndex((w) => w.end > reading.at!);
    return i < 0 ? null : [i, i];
  }, [hits, current, full, reading?.at]);
  // Only the paragraphs on screen are drawn (a sermon can be tens of thousands of words)
  const virtual = useVirtualizer({
    count: paragraphs.length,
    getScrollElement: () => body.current,
    // Measured once drawn; until then, by how many words it has
    estimateSize: (i) => 36 + Math.ceil(paragraphs[i].words.length / 13) * 26,
    getItemKey: (i) => paragraphs[i].from,
    overscan: 4,
  });
  useEffect(() => {
    if (target === null || paragraphs.length === 0) return;
    // Its paragraph drawn, then the word itself centered once it's there
    virtual.scrollToIndex(paragraphOf(paragraphs, target[0]), { align: "center" });
    let tries = 0;
    let id = 0;
    const center = () => {
      const word = body.current?.querySelector(`[data-word="${target[0]}"]`);
      if (word) word.scrollIntoView({ block: "center" });
      else if (tries++ < 20) id = requestAnimationFrame(center);
    };
    id = requestAnimationFrame(center);
    return () => cancelAnimationFrame(id);
    // `virtual` is made again each time it's drawn; what it scrolls is the same
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
      <DialogContent
        className="flex h-[88vh] max-w-4xl flex-col gap-0 p-0 sm:max-w-4xl"
        onKeyDown={(e) => {
          if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
            e.preventDefault();
            findBox.current?.select();
          }
        }}
      >
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
          <div className="flex items-center gap-1.5 pt-2">
            <div className="relative max-w-sm flex-1">
              <Search className="absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
              <Input
                ref={findBox}
                className="h-8 pl-8 text-sm"
                placeholder="Find in this sermon"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    step(e.shiftKey ? -1 : 1);
                  }
                }}
              />
            </div>
            {query.trim() && (
              <span className="w-20 text-xs text-muted-foreground tabular-nums">
                {hits.length ? `${current + 1} of ${hits.length}` : "Not found"}
              </span>
            )}
            <Button size="icon-sm" variant="ghost" title="Previous (Shift+Enter)" disabled={!hits.length} onClick={() => step(-1)}>
              <ChevronUp />
            </Button>
            <Button size="icon-sm" variant="ghost" title="Next (Enter)" disabled={!hits.length} onClick={() => step(1)}>
              <ChevronDown />
            </Button>
          </div>
        </DialogHeader>
        <div ref={body} className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {error ? (
            <p className="text-sm text-destructive">{error}</p>
          ) : !full ? (
            <div className="flex items-center gap-2 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Reading the transcript…
            </div>
          ) : (
            <div className="relative w-full" style={{ height: virtual.getTotalSize() }}>
              {virtual.getVirtualItems().map((item) => {
                const p = paragraphs[item.index];
                const last = p.words[p.words.length - 1];
                const inside = now !== null && now >= p.words[0].start - 0.05 && now < last.end + 0.05;
                return (
                  <div
                    key={item.key}
                    data-index={item.index}
                    ref={virtual.measureElement}
                    className="absolute top-0 left-0 w-full pb-3"
                    style={{ transform: `translateY(${item.start}px)` }}
                  >
                    <Paragraph
                      from={p.from}
                      words={p.words}
                      marks={marks}
                      now={inside ? now : null}
                      target={target !== null && target[1] >= p.from && target[0] < p.from + p.words.length ? target : null}
                      found={found}
                      verseLabel={verseLabel}
                      onWord={onWord}
                      onVerse={onVerse}
                    />
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
}
