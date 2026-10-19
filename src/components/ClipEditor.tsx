import { Loader2, Play, RotateCcw, Scissors, Square, Trash2, Volume2, Rewind } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { api, type Selection } from "@/lib/api";
import { useApp } from "@/lib/store";
import { usePeaks, useWords } from "@/lib/hooks";
import { play, sourceTime, usePlayer } from "@/lib/player";
import { fileName, formatTime, sourceColor } from "@/lib/refs";
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

/** The chosen reading of a verse: where it's cut, the silence around it, and listening to it */
export function ClipEditor({ verseKey, selection }: { verseKey: string; selection: Selection }) {
  const { project, matchById, editSelection, unselect } = useApp();
  const player = usePlayer();
  const source = project?.sources.find((s) => s.id === selection.sourceId);
  const match = matchById.get(selection.matchId);
  const clipStart = Math.max(0, selection.start + selection.trimStart);
  const clipEnd = Math.max(clipStart + 0.05, selection.end + selection.trimEnd);
  // The window moves in whole seconds, so dragging a handle doesn't refetch the waveform
  const winStart = Math.max(0, Math.floor(Math.min(selection.start, clipStart) - 3));
  const winEnd = Math.ceil(Math.max(selection.end, clipEnd) + 3);
  const { data: peaks, error } = usePeaks(source?.audioPath, winStart, winEnd);
  const words = useWords(source?.transcriptPath, (match?.wordStart ?? 0) - 25, (match?.wordEnd ?? 0) + 25);
  if (!project || !source) return null;

  const set = (change: Partial<Selection>) => editSelection(verseKey, change);
  const setEdge = (edge: "start" | "end", time: number) => {
    if (edge === "start") set({ trimStart: Math.min(time, clipEnd - 0.1) - selection.start });
    else set({ trimEnd: Math.max(time, clipStart + 0.1) - selection.end });
  };

  const rawId = `sel:${verseKey}`;
  const aroundId = `around:${verseKey}`;
  const exportId = `export:${verseKey}`;
  const now = sourceTime(player, rawId) ?? sourceTime(player, aroundId) ?? sourceTime(player, exportId);
  const icon = (id: string, Idle: typeof Play) =>
    player.loading === id ? <Loader2 className="animate-spin" /> : player.id === id ? <Square /> : <Idle />;

  return (
    <div className="space-y-3 rounded-lg border border-emerald-500/60 bg-background p-3">
      <div className="flex items-center gap-2">
        <span className="size-2.5 rounded-full" style={{ background: sourceColor(project, source.id) }} />
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">Chosen: {source.label}</div>
          <div className="truncate text-[11px] text-muted-foreground" title={source.audioPath}>
            {fileName(source.audioPath)} · {formatTime(clipStart, true)}–{formatTime(clipEnd, true)} ({(clipEnd - clipStart).toFixed(1)}s)
          </div>
        </div>
        <Button size="icon-sm" variant="ghost" title="Unchoose" onClick={() => unselect(verseKey)}>
          <Trash2 />
        </Button>
      </div>

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
          onWord={(w, e) => (e.shiftKey ? setEdge("end", w.end + 0.08) : setEdge("start", w.start - 0.08))}
        />
      ) : (
        <div className="flex h-[116px] items-center justify-center gap-2 rounded-md border text-xs text-muted-foreground">
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
        <Nudge label="Start" value={selection.trimStart} onChange={(v) => set({ trimStart: v })} />
        <Nudge label="End" value={selection.trimEnd} onChange={(v) => set({ trimEnd: v })} />
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
          {icon(rawId, Play)} Clip
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
          title="With its volume, leveling, fades, and silence, as the export will be (leveling of the whole export is applied when exporting)"
          onClick={() =>
            play(
              exportId,
              () =>
                api.renderClip({
                  audioPath: source.audioPath,
                  start: clipStart,
                  end: clipEnd,
                  padBefore: selection.padBefore,
                  padAfter: selection.padAfter,
                  processed: true,
                  targetDb: project.settings.leveling === "each" ? project.settings.targetDb : null,
                  gainDb: selection.gainDb + project.settings.outputGainDb,
                }),
              clipStart,
              selection.padBefore,
            )
          }
        >
          {icon(exportId, Volume2)} As exported
        </Button>
        {(selection.trimStart !== 0 || selection.trimEnd !== 0) && (
          <Button size="sm" variant="ghost" className="ml-auto" onClick={() => set({ trimStart: 0, trimEnd: 0 })}>
            <RotateCcw /> Reset cuts
          </Button>
        )}
      </div>
    </div>
  );
}
