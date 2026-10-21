import { Check, Combine, FileText, Filter, FilterX, Hand, Link2, Loader2, Play, Square, Rewind, AudioLines, Trash2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { api, type Match, type Word } from "@/lib/api";
import { useApp } from "@/lib/store";
import { useWords } from "@/lib/hooks";
import { play, sourceTime, usePlayer } from "@/lib/player";
import { audioPathOf, fileName, formatTime, isAdded, kindLabel, runLabel, sourceColor, sourceTitle, verseKey } from "@/lib/refs";

const CONTEXT_WORDS = 30;
const CONTEXT_SECONDS = 4;

export const kindClass = {
  exact: "bg-emerald-100 text-emerald-800 dark:bg-emerald-950 dark:text-emerald-300",
  close: "bg-sky-100 text-sky-800 dark:bg-sky-950 dark:text-sky-300",
  partial: "bg-amber-100 text-amber-800 dark:bg-amber-950 dark:text-amber-300",
} as const;

/** The transcript around a reading: what came before and after, muted, and the words that weren't the verse's marked */
export function TranscriptContext({
  words,
  from,
  match,
  now,
  onWord,
}: {
  words: Word[];
  from: number;
  match: Match;
  now: number | null;
  onWord?: (w: Word, index: number, e: React.MouseEvent) => void;
}) {
  const extra = new Set(match.extraWords);
  return (
    <p className="text-[13px] leading-relaxed">
      {words.map((w, k) => {
        const i = from + k;
        const inside = i >= match.wordStart && i <= match.wordEnd;
        const speaking = now !== null && now >= w.start && now < w.end + 0.05;
        return (
          <span key={i}>
            <span
              onClick={onWord ? (e) => onWord(w, i, e) : undefined}
              title={`${formatTime(w.start, true)}${w.timed ? "" : " (estimated)"}`}
              className={cn(
                "rounded-sm px-px",
                inside ? "text-foreground" : "text-muted-foreground/70",
                inside && extra.has(i) && "text-amber-600 italic dark:text-amber-400",
                speaking && "bg-primary text-primary-foreground",
                onWord && "cursor-pointer hover:bg-muted",
              )}
            >
              {w.text}
            </span>{" "}
          </span>
        );
      })}
    </p>
  );
}

/** The verse's text, with the words the speaker didn't say struck through */
export function HeardText({ text, heard }: { text: string; heard: boolean[] }) {
  return (
    <p className="text-[13px] leading-relaxed">
      {text.split(/\s+/).filter(Boolean).map((w, i) => (
        <span key={i}>
          <span className={cn(heard[i] === false && "text-red-600 line-through decoration-red-400 dark:text-red-400")}>{w}</span>{" "}
        </span>
      ))}
    </p>
  );
}

/**
A reading found for a verse. `part` is where it plays among the verse's chosen readings (0 first),
if chosen; `parts` is how many there are
*/
export function MatchCard({ match, verseText, part, parts }: { match: Match; verseText: string; part: number | null; parts: number }) {
  const { project, outline, runById, matchById, onlySermon, setOnlySermon, select, selectMany, join, openTranscript, removeAddedReading } = useApp();
  const selected = part !== null;
  const player = usePlayer();
  const source = project?.sources.find((s) => s.id === match.sourceId);
  const data = useWords(source?.transcriptPath, match.wordStart - CONTEXT_WORDS, match.wordEnd + CONTEXT_WORDS);
  if (!project || !source) return null;

  const run = match.runId ? runById.get(match.runId) : undefined;
  const runMatches = run ? run.matchIds.map((id) => matchById.get(id)).filter((m): m is Match => !!m) : [];
  const runChosen = runMatches.filter((m) => project.selections[verseKey(m.book, m.chapter, m.verse)]?.matchId === m.id).length;

  const clipId = `clip:${match.id}`;
  const contextId = `context:${match.id}`;
  const now = sourceTime(player, clipId) ?? sourceTime(player, contextId);
  const missed = match.verseWordsHeard.filter((h) => !h).length;

  const audioPath = audioPathOf(project, source);
  const playClip = () => play(clipId, () => api.renderClip({ audioPath, start: match.start, end: match.end }), match.start);
  const playContext = () => {
    const start = Math.max(0, match.start - CONTEXT_SECONDS);
    void play(contextId, () => api.renderClip({ audioPath, start, end: match.end + CONTEXT_SECONDS }), start);
  };

  return (
    <div className={cn("space-y-2 rounded-lg border bg-background p-3", selected && "border-emerald-500 ring-1 ring-emerald-500")}>
      <div className="flex items-center gap-2">
        <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, source.id) }} />
        <Tooltip>
          <TooltipTrigger asChild>
            <span className="min-w-0 truncate text-sm font-medium">{sourceTitle(project, source)}</span>
          </TooltipTrigger>
          <TooltipContent>
            <div className="flex items-center gap-1">
              <AudioLines className="size-3" /> {fileName(audioPath)}
            </div>
          </TooltipContent>
        </Tooltip>
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
          {formatTime(match.start)}–{formatTime(match.end)}
        </span>
        {isAdded(match) && (
          <span className="ml-auto flex shrink-0 items-center gap-0.5 text-[11px] text-muted-foreground" title="Added by hand, from what was said">
            <Hand className="size-3" /> Added
          </span>
        )}
        <span
          className={cn("shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium", !isAdded(match) && "ml-auto", kindClass[match.kind])}
          title={match.accuracy > 0 ? `${Math.round(match.accuracy * 100)}% accurate` : undefined}
        >
          {kindLabel[match.kind]} · {Math.round(match.score * 100)}%
        </span>
      </div>
      <div className="truncate text-[11px] text-muted-foreground" title={audioPath}>
        {fileName(audioPath)}
      </div>

      {run && (
        <div className="flex items-center gap-2 rounded-md bg-muted/60 px-2 py-1.5 text-xs">
          <Link2 className="size-3.5 shrink-0 text-primary" />
          <span className="min-w-0 flex-1">
            Read straight through: <span className="font-medium">{runLabel(outline, run)}</span>{" "}
            <span className="text-muted-foreground">({run.matchIds.length} verses)</span>
          </span>
          <Button
            size="xs"
            variant={runChosen === runMatches.length ? "secondary" : "outline"}
            disabled={runChosen === runMatches.length}
            onClick={() => selectMany(runMatches)}
          >
            {runChosen === runMatches.length ? "All chosen" : `Use for all ${runMatches.length}`}
          </Button>
        </div>
      )}

      <div className="rounded-md border bg-muted/30 p-2">
        {data ? (
          <TranscriptContext words={data.words} from={data.from} match={match} now={now} />
        ) : (
          <Loader2 className="size-4 animate-spin text-muted-foreground" />
        )}
      </div>

      {missed > 0 && verseText && (
        <div className="space-y-0.5">
          <div className="text-[11px] text-muted-foreground">
            {missed} word{missed === 1 ? "" : "s"} of the verse not heard:
          </div>
          <HeardText text={verseText} heard={match.verseWordsHeard} />
        </div>
      )}

      <div className="flex items-center gap-1.5">
        <Button size="sm" variant="outline" onClick={playClip}>
          {player.loading === clipId ? <Loader2 className="animate-spin" /> : player.id === clipId ? <Square /> : <Play />}
          Reading
        </Button>
        <Button size="sm" variant="ghost" onClick={playContext}>
          {player.loading === contextId ? <Loader2 className="animate-spin" /> : player.id === contextId ? <Square /> : <Rewind />}
          With context
        </Button>
        <Button size="sm" variant="ghost" title="Open the whole transcript here" onClick={() => openTranscript(source.id, match.start)}>
          <FileText /> In transcript
        </Button>
        {isAdded(match) && (
          <Button
            size="icon-sm"
            variant="ghost"
            title="Take this reading out (added by hand), and out of the verse's choice"
            onClick={() => removeAddedReading(match.id)}
          >
            <Trash2 />
          </Button>
        )}
        {onlySermon === source.id ? (
          <Button size="icon-sm" variant="secondary" title="Show every sermon's readings again" onClick={() => setOnlySermon(null)}>
            <FilterX />
          </Button>
        ) : (
          <Button
            size="icon-sm"
            variant="ghost"
            title="Show only this sermon's readings, in every verse (for now): to see what it reads straight through"
            onClick={() => setOnlySermon(source.id)}
          >
            <Filter />
          </Button>
        )}
        <div className="ml-auto">
          {selected ? (
            <Badge className="bg-emerald-600">
              <Check /> {parts > 1 ? `Chosen, part ${part + 1} of ${parts}` : "Chosen"}
            </Badge>
          ) : (
            <span className="flex items-center gap-1.5">
              {parts > 0 && (
                <Button
                  size="sm"
                  variant="outline"
                  title="Play this with the reading chosen, one after the other: for a verse read in pieces (its first half, then the rest later)"
                  onClick={() => join(match)}
                >
                  <Combine /> Join
                </Button>
              )}
              <Button size="sm" onClick={() => select(match)}>
                {parts > 0 ? "Use instead" : "Use this reading"}
              </Button>
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
