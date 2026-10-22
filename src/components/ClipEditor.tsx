import { useState } from "react";
import { ChevronDown, ChevronUp, Loader2, Maximize2, Play, RotateCcw, Scissors, Square, SquareSplitHorizontal, Trash2, Undo2, Volume2, Rewind, X, ZoomIn, ZoomOut } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { api, type Cut, type Part, type Selection } from "@/lib/api";
import { partsOf, savedProjectId, useApp } from "@/lib/store";
import { usePeaks, useWords } from "@/lib/hooks";
import { play, sourceTime, stop, usePlayer } from "@/lib/player";
import { audioPathOf, clipOf, fileName, formatTime, gapsOf, kindLabel, playedSeconds, sourceColor, sourceTitle, spansOf } from "@/lib/refs";
import { cn } from "@/lib/utils";
import { kindClass } from "./MatchCard";
import { Waveform } from "./Waveform";

function Nudge({ label, value, onChange }: { label: string; value: number; onChange: (v: number) => void }) {
  const step = (d: number) => onChange(Math.round((value + d) * 1000) / 1000);
  return (
    <div className="space-y-1">
      <div className="text-[11px] text-muted-foreground">{label}</div>
      <div className="flex items-center gap-0.5">
        <Button size="xs" variant="outline" onClick={() => step(-0.25)}>
          −.25
        </Button>
        <Button size="xs" variant="outline" onClick={() => step(-0.05)}>
          −.05
        </Button>
        <span className="w-14 text-center text-xs tabular-nums">
          {value > 0 ? "+" : ""}
          {value.toFixed(2)}s
        </span>
        <Button size="xs" variant="outline" onClick={() => step(0.05)}>
          +.05
        </Button>
        <Button size="xs" variant="outline" onClick={() => step(0.25)}>
          +.25
        </Button>
      </div>
    </div>
  );
}

function Seconds({ id, label, value, onChange }: { id: string; label: string; value: number; onChange: (v: number) => void }) {
  return (
    <div className="space-y-1">
      <Label htmlFor={id} className="text-[11px] font-normal text-muted-foreground">
        {label}
      </Label>
      <div className="flex items-center gap-1">
        <Input
          id={id}
          type="number"
          min={0}
          step={0.1}
          className="h-7 w-20 text-xs"
          value={value}
          onChange={(e) => onChange(Math.max(0, Number(e.target.value) || 0))}
        />
        <span className="text-xs text-muted-foreground">s</span>
      </div>
    </div>
  );
}

/** Each part's color, in the order they play */
const PART_COLORS = ["#059669", "#2563eb", "#9333ea", "#d97706", "#db2777", "#0891b2"];
const partColor = (i: number) => PART_COLORS[i % PART_COLORS.length];

/** The verse's words, each in the color of the part that says it; words no part says are struck through */
function JoinedText({ text, heard }: { text: string; heard: (boolean[] | undefined)[] }) {
  const words = text.split(/\s+/).filter(Boolean);
  const said = words.map((_, i) => heard.flatMap((h, k) => (h?.[i] ? [k] : [])));
  const missing = said.filter((by) => by.length === 0).length;
  return (
    <div className="space-y-1">
      <p className="text-[13px] leading-relaxed">
        {words.map((w, i) => {
          const by = said[i];
          return (
            <span key={i}>
              <span
                title={by.length === 0 ? "Not said in any part" : `Said in part ${by.map((k) => k + 1).join(" and ")}`}
                className={cn(
                  "font-medium",
                  by.length === 0 && "text-red-600 line-through decoration-red-400 dark:text-red-400",
                  by.length > 1 && "underline decoration-dotted underline-offset-2",
                )}
                style={by.length > 0 ? { color: partColor(by[0]) } : undefined}
              >
                {w}
              </span>{" "}
            </span>
          );
        })}
      </p>
      {missing > 0 && (
        <p className="text-[11px] text-muted-foreground">
          {missing} word{missing === 1 ? "" : "s"} of the verse {missing === 1 ? "isn't" : "aren't"} in any part.
        </p>
      )}
    </div>
  );
}

/** How many seconds are shown either side of a reading: zoomed in and out by these */
const MARGINS = [1, 3, 8, 20, 60];

/** Where a part is cut and what's cut out of it, on its sermon's waveform, with its words */
function TrimArea({
  verseKey,
  part,
  index,
  height,
  startMargin,
  now,
}: {
  verseKey: string;
  part: Part;
  index: number;
  height: number;
  /** The seconds shown either side of the reading at first (one of MARGINS) */
  startMargin: number;
  now: number | null;
}) {
  const { project, matchById, editPart } = useApp();
  const player = usePlayer();
  const source = project?.sources.find((s) => s.id === part.sourceId);
  const match = matchById.get(part.matchId);
  const [clipStart, clipEnd] = clipOf(part);
  const [margin, setMargin] = useState(startMargin);
  // The window is around the reading's own cut points, in whole seconds, growing only when the
  // clip comes near its edge; while a handle is dragged it stays put, so the waveform doesn't
  // move under the pointer
  const [held, setHeld] = useState<[number, number] | null>(null);
  const [winStart, winEnd] = held ?? [
    Math.max(0, Math.floor(Math.min(part.start - margin, clipStart - 1))),
    Math.ceil(Math.max(part.end + margin, clipEnd + 1)),
  ];
  const audioPath = source && project ? audioPathOf(project, source) : undefined;
  const { data: peaks, error } = usePeaks(audioPath, winStart, winEnd);
  // Enough words either side for the seconds shown (a few a second)
  const around = 25 + margin * 4;
  const words = useWords(source?.transcriptPath, (match?.wordStart ?? 0) - around, (match?.wordEnd ?? 0) + around);
  if (!project || !source) return null;

  const setPart = (change: Partial<Part>) => editPart(verseKey, index, change);
  const setEdge = (edge: "start" | "end", time: number) => {
    if (edge === "start") setPart({ trimStart: Math.min(time, clipEnd - 0.1) - part.start });
    else setPart({ trimEnd: Math.max(time, clipStart + 0.1) - part.end });
  };
  const cuts = part.cuts ?? [];
  // What it plays itself (from a click, or a stretch marked), followed on the waveform
  const playPrefix = `trim:${verseKey}:${part.matchId}:`;
  const own = player.id?.startsWith(playPrefix) ? sourceTime(player, player.id) : null;
  const playing = player.id?.startsWith(playPrefix) || player.loading?.startsWith(playPrefix);
  /** Plays `from` to `to` of the sermon, without `left` (stretches left out) */
  const hear = (what: string, from: number, to: number, left: Cut[]) => {
    const spans = spansOf({ matchId: "", sourceId: "", start: from, end: to, trimStart: 0, trimEnd: 0, cuts: left });
    void play(playPrefix + what, () => api.renderClip({ audioPath: audioPath!, start: from, end: to, cuts: left }), spans[0][0], 0, 0, gapsOf(spans));
  };
  /** From where it's clicked to the end of what's shown, as it will sound (without its cuts) */
  const playFrom = (time: number) => hear(`from:${time}`, time, Math.max(winEnd, time + 1), cuts);
  /** A stretch to cut out alone, or the second either side of it joined as they'd be without it (and the cuts made) */
  const preview = (cut: Cut, how: "cut" | "join") =>
    how === "cut"
      ? hear(`cut:${cut.start}`, cut.start, cut.end, [])
      : hear(`join:${cut.start}`, Math.max(0, cut.start - 1), cut.end + 1, [...cuts.filter((c) => c.start !== cut.start || c.end !== cut.end), cut]);
  const zoom = (by: number) => {
    const i = MARGINS.indexOf(margin);
    setMargin(MARGINS[Math.min(MARGINS.length - 1, Math.max(0, (i < 0 ? 1 : i) + by))]);
  };

  return (
    <div className="space-y-2">
      {peaks ? (
        <Waveform
          peaks={peaks.peaks}
          peaksStart={peaks.start}
          perSecond={peaks.perSecond}
          start={winStart}
          end={winEnd}
          clipStart={clipStart}
          clipEnd={clipEnd}
          now={own ?? now}
          words={words?.words ?? []}
          height={height}
          onSeek={playFrom}
          onPreview={preview}
          onChange={setEdge}
          onDrag={(edge) => setHeld(edge ? [winStart, winEnd] : null)}
          onWord={(w, e) => (e.shiftKey ? setEdge("end", w.end + 0.08) : setEdge("start", w.start - 0.08))}
          cuts={cuts}
          onCut={(cut) => setPart({ cuts: [...cuts, cut].sort((a, b) => a.start - b.start) })}
          onUncut={(i) => setPart({ cuts: cuts.filter((_, k) => k !== i) })}
        />
      ) : (
        <div className="flex items-center justify-center gap-2 rounded-md border text-xs text-muted-foreground" style={{ height: height + 64 }}>
          {error ? (
            error
          ) : (
            <>
              <Loader2 className="size-4 animate-spin" />{" "}
              {/^https?:\/\//.test(audioPath ?? "") ? "Downloading the sermon's audio (once)…" : "Reading the sermon's audio (once per file)…"}
            </>
          )}
        </div>
      )}
      <div className="flex items-start gap-2">
        <p className="flex-1 text-[11px] text-muted-foreground">
          Click the waveform to play from there. Drag the green handles, or click a word to start just before it (shift-click to end just
          after it). Drag across the waveform to mark a slip the reader corrected: hear it, hear it left out, and cut it out.
        </p>
        <span className="flex shrink-0 items-center gap-0.5">
          {playing && (
            <Button size="icon-xs" variant="ghost" title="Stop" onClick={stop}>
              <Square />
            </Button>
          )}
          <Button size="icon-xs" variant="ghost" title="Show more around it" disabled={margin >= MARGINS[MARGINS.length - 1]} onClick={() => zoom(1)}>
            <ZoomOut />
          </Button>
          <span className="w-12 text-center text-[11px] text-muted-foreground tabular-nums">±{margin}s</span>
          <Button size="icon-xs" variant="ghost" title="Show less around it" disabled={margin <= MARGINS[0]} onClick={() => zoom(-1)}>
            <ZoomIn />
          </Button>
        </span>
      </div>

      {cuts.length > 0 && (
        <div className="space-y-0.5">
          {cuts.map((c, i) => (
            <div key={i} className="flex items-center gap-2 text-xs">
              <Scissors className="size-3.5 shrink-0 text-red-500" />
              <span className="flex-1 tabular-nums">
                Cut out {formatTime(c.start, true)}–{formatTime(c.end, true)}{" "}
                <span className="text-muted-foreground">({(c.end - c.start).toFixed(2)}s)</span>
              </span>
              <Button size="icon-xs" variant="ghost" title="Hear what's cut out" onClick={() => preview(c, "cut")}>
                <Play />
              </Button>
              <Button size="xs" variant="ghost" title="Hear a second either side, joined without it" onClick={() => preview(c, "join")}>
                <SquareSplitHorizontal /> Joined
              </Button>
              <Button size="xs" variant="ghost" onClick={() => setPart({ cuts: cuts.filter((_, k) => k !== i) })}>
                <Undo2 /> Put back
              </Button>
            </div>
          ))}
        </div>
      )}

      <div className="flex flex-wrap items-end gap-x-5 gap-y-2">
        <Nudge label="Start" value={part.trimStart} onChange={(v) => setPart({ trimStart: v })} />
        <Nudge label="End" value={part.trimEnd} onChange={(v) => setPart({ trimEnd: v })} />
        {(part.trimStart !== 0 || part.trimEnd !== 0 || cuts.length > 0) && (
          <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setPart({ trimStart: 0, trimEnd: 0, cuts: [] })}>
            <RotateCcw /> Reset cuts
          </Button>
        )}
      </div>
    </div>
  );
}

/** Stretches a part's clip leaves out, as the player skips them */
const skipsOf = (part: Part) => gapsOf(spansOf(part));

/** The chosen reading of a verse (or readings, for a verse read in pieces): where each is cut, their order, the silence around them, and listening to them */
export function ClipEditor({ verseKey, selection, verseText }: { verseKey: string; selection: Selection; verseText: string }) {
  const { project, matchById, editSelection, unselect, unjoin, arrange } = useApp();
  const player = usePlayer();
  const parts = partsOf(selection);
  const [active, setActive] = useState(0);
  const [expanded, setExpanded] = useState(false);
  const index = Math.min(active, parts.length - 1);
  const part = parts[index];
  const source = project?.sources.find((s) => s.id === part.sourceId);
  const match = matchById.get(part.matchId);
  const [clipStart, clipEnd] = clipOf(part);
  const audioPath = source && project ? audioPathOf(project, source) : undefined;
  if (!project || !source) return null;

  const set = (change: Partial<Selection>) => editSelection(verseKey, change);
  const move = (i: number, by: number) => {
    const order = [...parts];
    [order[i], order[i + by]] = [order[i + by], order[i]];
    arrange(verseKey, order);
    if (i === index) setActive(i + by);
    else if (i + by === index) setActive(i);
  };
  const remove = (i: number) => {
    unjoin(verseKey, parts[i].matchId);
    if (i < index) setActive(index - 1);
  };
  const [book, chapter, verse] = verseKey.split(".").map(Number);
  // How far into the verse's audio this part begins, so the waveform follows it as the verse plays
  const before = parts.slice(0, index).reduce((sum, p) => sum + playedSeconds(p), 0);
  const joined = parts.length > 1;
  const played = playedSeconds(part);
  const cutCount = part.cuts?.length ?? 0;

  const rawId = `sel:${verseKey}:${part.matchId}`;
  const aroundId = `around:${verseKey}:${part.matchId}`;
  const exportId = `export:${verseKey}`;
  const now = sourceTime(player, rawId) ?? sourceTime(player, aroundId) ?? sourceTime(player, exportId);
  const icon = (id: string, Idle: typeof Play) =>
    player.loading === id ? <Loader2 className="animate-spin" /> : player.id === id ? <Square /> : <Idle />;

  const listen = (
    <div className="flex flex-wrap items-center gap-1.5">
      <Button
        size="sm"
        variant="outline"
        onClick={() =>
          play(
            rawId,
            () =>
              api.renderClip({
                audioPath: audioPath!,
                start: clipStart,
                end: clipEnd,
                processed: selection.gainDb !== 0,
                gainDb: selection.gainDb,
                cuts: part.cuts,
              }),
            clipStart,
            0,
            0,
            skipsOf(part),
          )
        }
      >
        {icon(rawId, Play)} {joined ? `Part ${index + 1}` : "Clip"}
      </Button>
      <Button
        size="sm"
        variant="ghost"
        title="Two seconds either side, to hear the cuts"
        onClick={() => {
          const s = Math.max(0, clipStart - 2);
          void play(aroundId, () => api.renderClip({ audioPath: audioPath!, start: s, end: clipEnd + 2, cuts: part.cuts }), s, 0, 0, skipsOf(part));
        }}
      >
        {icon(aroundId, Rewind)} Around cuts
      </Button>
      <Button
        size="sm"
        variant="ghost"
        title={`${joined ? "Every part, one after another, with" : "With"} its volume, leveling, fades, and silence, as the export will be`}
        onClick={() =>
          play(exportId, async () => api.renderVerse(await savedProjectId(), book, chapter, verse), clipStart, selection.padBefore + before, 0, skipsOf(part))
        }
      >
        {icon(exportId, Volume2)} As exported
      </Button>
    </div>
  );
  const quality = match && (
    <span
      className={cn("shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium", kindClass[match.kind])}
      title={match.accuracy > 0 ? `${Math.round(match.accuracy * 100)}% accurate` : undefined}
    >
      {kindLabel[match.kind]} · {Math.round(match.score * 100)}%
    </span>
  );
  const where = (
    <>
      {joined && `Part ${index + 1}: `}
      {fileName(audioPath ?? "")} · {formatTime(clipStart, true)}–{formatTime(clipEnd, true)} ({played.toFixed(1)}s
      {cutCount > 0 && `, ${cutCount} cut out`})
    </>
  );

  return (
    <div className="space-y-3 rounded-lg border border-emerald-500/60 bg-background p-3">
      <div className="flex items-center gap-2">
        <span className="size-2.5 rounded-full" style={{ background: sourceColor(project, source.id) }} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">
            {joined ? `Chosen: ${parts.length} readings, joined` : `Chosen: ${sourceTitle(project, source)}`}
          </div>
          <div className="truncate text-[11px] text-muted-foreground" title={audioPath}>
            {where}
          </div>
        </div>
        {quality}
        <Button size="icon-sm" variant="ghost" title="Cut it in a larger window" onClick={() => setExpanded(true)}>
          <Maximize2 />
        </Button>
        <Button size="icon-sm" variant="ghost" title="Unchoose" onClick={() => unselect(verseKey)}>
          <Trash2 />
        </Button>
      </div>

      {joined && (
        <div className="space-y-1">
          {verseText && <JoinedText text={verseText} heard={parts.map((p) => matchById.get(p.matchId)?.verseWordsHeard)} />}
          <div className="text-[11px] text-muted-foreground">Played one after another, in this order. Click one to cut it.</div>
          {parts.map((p, i) => {
            const s = project.sources.find((x) => x.id === p.sourceId);
            const [a, b] = clipOf(p);
            const m = matchById.get(p.matchId);
            return (
              <div
                key={p.matchId}
                onClick={() => setActive(i)}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded-md border px-2 py-1 text-xs hover:bg-muted/60",
                  i === index && "border-emerald-500 bg-emerald-500/5",
                )}
              >
                <span
                  className="flex size-4 shrink-0 items-center justify-center rounded-sm text-[10px] font-semibold text-white tabular-nums"
                  style={{ background: partColor(i) }}
                >
                  {i + 1}
                </span>
                <span className="size-2 shrink-0 rounded-full" style={{ background: sourceColor(project, p.sourceId) }} />
                <span className="min-w-0 flex-1 truncate">
                  {sourceTitle(project, s)} · {formatTime(a, true)}–{formatTime(b, true)}{" "}
                  <span className="text-muted-foreground">({playedSeconds(p).toFixed(1)}s)</span>
                </span>
                {m && <span className={cn("shrink-0 rounded px-1 text-[10px] font-medium", kindClass[m.kind])}>{kindLabel[m.kind]} · {Math.round(m.score * 100)}%</span>}
                <span className="flex shrink-0" onClick={(e) => e.stopPropagation()}>
                  <Button size="icon-xs" variant="ghost" title="Play earlier" disabled={i === 0} onClick={() => move(i, -1)}>
                    <ChevronUp />
                  </Button>
                  <Button size="icon-xs" variant="ghost" title="Play later" disabled={i === parts.length - 1} onClick={() => move(i, 1)}>
                    <ChevronDown />
                  </Button>
                  <Button size="icon-xs" variant="ghost" title="Take this reading out" onClick={() => remove(i)}>
                    <X />
                  </Button>
                </span>
              </div>
            );
          })}
        </div>
      )}

      {!expanded && <TrimArea key={part.matchId} verseKey={verseKey} part={part} index={index} height={120} startMargin={3} now={now} />}
      <Dialog open={expanded} onOpenChange={setExpanded}>
        <DialogContent className="w-[95vw] max-w-[95vw] sm:max-w-[95vw]">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              Cutting {chapter}:{verse}
              {joined && `, part ${index + 1}`} · {sourceTitle(project, source)} {quality}
            </DialogTitle>
            <DialogDescription>{where}</DialogDescription>
          </DialogHeader>
          {verseText && <p className="text-sm leading-relaxed">{verseText}</p>}
          <TrimArea key={part.matchId} verseKey={verseKey} part={part} index={index} height={260} startMargin={8} now={now} />
          {listen}
        </DialogContent>
      </Dialog>

      <div className="flex flex-wrap items-end gap-x-5 gap-y-2">
        <Seconds id="padb" label="Silence before" value={selection.padBefore} onChange={(v) => set({ padBefore: v })} />
        <Seconds id="pada" label="Silence after" value={selection.padAfter} onChange={(v) => set({ padAfter: v })} />
        <div className="flex items-center gap-2 pb-1">
          <Switch id="trim" checked={selection.needsTrim} onCheckedChange={(v) => set({ needsTrim: v })} />
          <Label htmlFor="trim" className="flex items-center gap-1 text-xs font-normal">
            <Scissors className="size-3.5 text-amber-500" /> Still needs trimming
          </Label>
        </div>
      </div>

      <div className="space-y-1">
        <div className="flex items-center justify-between text-[11px] text-muted-foreground">
          <span>Volume</span>
          <span className="flex items-center gap-1">
            <span className="font-medium text-foreground tabular-nums">
              {selection.gainDb > 0 ? "+" : ""}
              {selection.gainDb.toFixed(1)} dB
            </span>
            {selection.gainDb !== 0 && (
              <Button size="icon-xs" variant="ghost" title="Reset volume" onClick={() => set({ gainDb: 0 })}>
                <RotateCcw />
              </Button>
            )}
          </span>
        </div>
        <Slider min={-12} max={12} step={0.5} value={[selection.gainDb]} onValueChange={([v]) => set({ gainDb: v })} />
        <p className="text-[11px] text-muted-foreground">
          {project.settings.leveling === "each"
            ? "Applied after the reading is leveled to the target."
            : project.settings.leveling === "export"
              ? "Kept relative to the other readings when the whole export is leveled."
              : "Leveling is off, so this is the only volume change."}
        </p>
      </div>

      {listen}
    </div>
  );
}
