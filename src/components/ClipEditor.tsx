import { useState } from "react";
import { ChevronDown, ChevronUp, Loader2, Play, RotateCcw, Scissors, Square, Trash2, Volume2, Rewind, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { api, type Part, type Selection } from "@/lib/api";
import { partsOf, useApp } from "@/lib/store";
import { usePeaks, useWords } from "@/lib/hooks";
import { play, sourceTime, usePlayer } from "@/lib/player";
import { fileName, formatTime, sourceColor } from "@/lib/refs";
import { cn } from "@/lib/utils";
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

/** Where a part is cut, with its trims */
function cuts(part: Part): [number, number] {
  const start = Math.max(0, part.start + part.trimStart);
  return [start, Math.max(start + 0.05, part.end + part.trimEnd)];
}

/** The chosen reading of a verse (or readings, for a verse read in pieces): where each is cut, their order, the silence around them, and listening to them */
export function ClipEditor({ verseKey, selection, verseText }: { verseKey: string; selection: Selection; verseText: string }) {
  const { project, matchById, editSelection, editPart, unselect, unjoin, arrange } = useApp();
  const player = usePlayer();
  const parts = partsOf(selection);
  const [active, setActive] = useState(0);
  const index = Math.min(active, parts.length - 1);
  const part = parts[index];
  const source = project?.sources.find((s) => s.id === part.sourceId);
  const match = matchById.get(part.matchId);
  const [clipStart, clipEnd] = cuts(part);
  // The window is around the reading's own cut points, in whole seconds, growing only when the
  // clip comes near its edge; while a handle is dragged it stays put, so the waveform doesn't
  // move under the pointer
  const [held, setHeld] = useState<[number, number] | null>(null);
  const [winStart, winEnd] = held ?? [
    Math.max(0, Math.floor(Math.min(part.start - 3, clipStart - 1))),
    Math.ceil(Math.max(part.end + 3, clipEnd + 1)),
  ];
  const { data: peaks, error } = usePeaks(source?.audioPath, winStart, winEnd);
  const words = useWords(source?.transcriptPath, (match?.wordStart ?? 0) - 25, (match?.wordEnd ?? 0) + 25);
  if (!project || !source) return null;

  const set = (change: Partial<Selection>) => editSelection(verseKey, change);
  const setPart = (change: Partial<Part>) => editPart(verseKey, index, change);
  const setEdge = (edge: "start" | "end", time: number) => {
    if (edge === "start") setPart({ trimStart: Math.min(time, clipEnd - 0.1) - part.start });
    else setPart({ trimEnd: Math.max(time, clipStart + 0.1) - part.end });
  };
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
  const before = parts.slice(0, index).reduce((sum, p) => sum + (cuts(p)[1] - cuts(p)[0]), 0);
  const joined = parts.length > 1;

  const rawId = `sel:${verseKey}:${part.matchId}`;
  const aroundId = `around:${verseKey}:${part.matchId}`;
  const exportId = `export:${verseKey}`;
  const now = sourceTime(player, rawId) ?? sourceTime(player, aroundId) ?? sourceTime(player, exportId);
  const icon = (id: string, Idle: typeof Play) =>
    player.loading === id ? <Loader2 className="animate-spin" /> : player.id === id ? <Square /> : <Idle />;

  return (
    <div className="space-y-3 rounded-lg border border-emerald-500/60 bg-background p-3">
      <div className="flex items-center gap-2">
        <span className="size-2.5 rounded-full" style={{ background: sourceColor(project, source.id) }} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">
            {joined ? `Chosen: ${parts.length} readings, joined` : `Chosen: ${source.label}`}
          </div>
          <div className="truncate text-[11px] text-muted-foreground" title={source.audioPath}>
            {joined && `Part ${index + 1}: `}
            {fileName(source.audioPath)} · {formatTime(clipStart, true)}–{formatTime(clipEnd, true)} ({(clipEnd - clipStart).toFixed(1)}s)
          </div>
        </div>
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
            const [a, b] = cuts(p);
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
                  {s?.label ?? "Missing sermon"} · {formatTime(a, true)}–{formatTime(b, true)}{" "}
                  <span className="text-muted-foreground">({(b - a).toFixed(1)}s)</span>
                </span>
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

      {peaks ? (
        <Waveform
          peaks={peaks.peaks}
          peaksStart={peaks.start}
          perSecond={peaks.perSecond}
          start={winStart}
          end={winEnd}
          clipStart={clipStart}
          clipEnd={clipEnd}
          now={now}
          words={words?.words ?? []}
          onChange={setEdge}
          onDrag={(edge) => setHeld(edge ? [winStart, winEnd] : null)}
          onWord={(w, e) => (e.shiftKey ? setEdge("end", w.end + 0.08) : setEdge("start", w.start - 0.08))}
        />
      ) : (
        <div className="flex h-[184px] items-center justify-center gap-2 rounded-md border text-xs text-muted-foreground">
          {error ? (
            error
          ) : (
            <>
              <Loader2 className="size-4 animate-spin" /> Reading the sermon's audio (once per file)…
            </>
          )}
        </div>
      )}
      <p className="text-[11px] text-muted-foreground">
        Drag the green handles, or click a word to start just before it (shift-click to end just after it).
      </p>

      <div className="flex flex-wrap gap-x-5 gap-y-2">
        <Nudge label="Start" value={part.trimStart} onChange={(v) => setPart({ trimStart: v })} />
        <Nudge label="End" value={part.trimEnd} onChange={(v) => setPart({ trimEnd: v })} />
      </div>
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

      <div className="flex flex-wrap items-center gap-1.5">
        <Button
          size="sm"
          variant="outline"
          onClick={() =>
            play(
              rawId,
              () =>
                api.renderClip({
                  audioPath: source.audioPath,
                  start: clipStart,
                  end: clipEnd,
                  processed: selection.gainDb !== 0,
                  gainDb: selection.gainDb,
                }),
              clipStart,
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
            void play(aroundId, () => api.renderClip({ audioPath: source.audioPath, start: s, end: clipEnd + 2 }), s);
          }}
        >
          {icon(aroundId, Rewind)} Around cuts
        </Button>
        <Button
          size="sm"
          variant="ghost"
          title={`${joined ? "Every part, one after another, with" : "With"} its volume, leveling, fades, and silence, as the export will be`}
          onClick={() => play(exportId, () => api.renderVerse(project, book, chapter, verse), clipStart, selection.padBefore + before)}
        >
          {icon(exportId, Volume2)} As exported
        </Button>
        {(part.trimStart !== 0 || part.trimEnd !== 0) && (
          <Button size="sm" variant="ghost" className="ml-auto" onClick={() => setPart({ trimStart: 0, trimEnd: 0 })}>
            <RotateCcw /> Reset cuts
          </Button>
        )}
      </div>
    </div>
  );
}
