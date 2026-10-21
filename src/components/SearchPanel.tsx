import { useEffect, useMemo, useRef, useState } from "react";
import { FileText, Loader2, Play, Search, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, errorMessage, type Said, type SearchProgress } from "@/lib/api";
import { useApp } from "@/lib/store";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, formatTime, sermonMatches, sourceColor, sourceOf, sourceTitle, valuesOf } from "@/lib/refs";
import { AddReading } from "./AddReading";
import { MultiSelect } from "./MultiSelect";

/** The most places shown */
const LIMIT = 500;

/** What was last searched for, and among whose sermons, kept while other tabs are open */
let lastQuery = "";
let lastSpeakers: string[] = [];
let lastSeries: string[] = [];

/** Searches for this when the Search tab is next shown, among every sermon */
export function searchFor(query: string) {
  [lastQuery, lastSpeakers, lastSeries] = [query, [], []];
}

/** Searching every sermon's transcript for what was said */
export function SearchPanel() {
  const project = useApp((s) => s.project);
  const openTranscript = useApp((s) => s.openTranscript);
  const player = usePlayer();
  const [query, setQuery] = useState(lastQuery);
  const [speakers, setSpeakers] = useState(lastSpeakers);
  const [series, setSeries] = useState(lastSeries);
  // What's been found so far: `searched` of `total` sermons, and whether that's all
  const [result, setResult] = useState<{
    query: string;
    found: Said[];
    more: boolean;
    searched: number;
    total: number;
    done: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => input.current?.focus(), []);

  // Searched a moment after typing stops
  useEffect(() => {
    [lastQuery, lastSpeakers, lastSeries] = [query, speakers, series];
    const p = useApp.getState().project;
    if (!p || !query.trim()) {
      setResult(null);
      return;
    }
    // Only the sermons of the speakers and series chosen (all, with none chosen)
    const among = speakers.length || series.length ? p.sources.filter((s) => sermonMatches(p, s, speakers, series)).map((s) => s.id) : null;
    let live = true;
    const timer = setTimeout(() => {
      setBusy(true);
      // Shown as they're found, a batch of sermons at a time, in the sermons' order
      let found: Said[] = [];
      let errors: string[] = [];
      const onProgress = (progress: SearchProgress) => {
        if (!live) return;
        found = progress.found.length ? [...found, ...progress.found] : found;
        errors = progress.errors.length ? [...errors, ...progress.errors] : errors;
        setResult({ query, found, more: progress.more, searched: progress.searched, total: progress.total, done: progress.done });
        setError(errors.length ? errors.join("\n") : null);
      };
      api.searchTranscripts(p.id, query, among, LIMIT, { onProgress }).then(
        (r) => {
          if (!live) return;
          setResult({ query, found: r.found, more: r.more, searched: 0, total: 0, done: true });
          setError(r.errors.length ? r.errors.join("\n") : null);
          setBusy(false);
        },
        (e) => {
          if (!live) return;
          setError(errorMessage(e));
          setBusy(false);
        },
      );
    }, 200);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, speakers, series]);

  // Places by sermon, in the sermons' order
  const groups = useMemo(() => {
    const bySource = new Map<string, Said[]>();
    for (const s of result?.found ?? []) {
      const list = bySource.get(s.sourceId);
      if (list) list.push(s);
      else bySource.set(s.sourceId, [s]);
    }
    return [...bySource.entries()];
  }, [result]);

  // The speakers and series to choose among (from every sermon, so worked out once for them)
  const sermons = project?.sources;
  const files = project?.files;
  const [speakerValues, seriesValues] = useMemo(() => {
    const p = useApp.getState().project;
    return p ? [valuesOf(p, "speaker"), valuesOf(p, "series")] : [[], []];
  }, [sermons, files]);

  if (!project) return null;

  const listen = (s: Said) => {
    const source = sourceOf(project, s.sourceId);
    if (!source) return;
    const start = Math.max(0, s.start - 2);
    void play(`said:${s.sourceId}:${s.wordStart}`, () => api.renderClip({ audioPath: audioPathOf(project, source), start, end: s.end + 4 }), start);
  };

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-4xl space-y-4 p-6">
        <div className="relative">
          <Search className="absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={input}
            className="h-11 pl-9 text-base"
            placeholder="Search what was said in every sermon"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
          {busy && <Loader2 className="absolute top-1/2 right-3 size-4 -translate-y-1/2 animate-spin text-muted-foreground" />}
        </div>
        {(speakerValues.length > 0 || seriesValues.length > 0) && (
          <div className="grid gap-2 sm:grid-cols-2">
            {speakerValues.length > 0 && (
              <MultiSelect
                options={speakerValues.map((v) => ({ value: v, label: v }))}
                value={speakers}
                onChange={setSpeakers}
                placeholder="Every speaker"
                searchPlaceholder="Search speakers…"
              />
            )}
            {seriesValues.length > 0 && (
              <MultiSelect
                options={seriesValues.map((v) => ({ value: v, label: v }))}
                value={series}
                onChange={setSeries}
                placeholder="Every series"
                searchPlaceholder="Search series…"
              />
            )}
          </div>
        )}
        <p className="text-xs text-muted-foreground">
          Words are found in order, whatever their case, accents, or punctuation, with numbers spelled out (“70 times 7” finds
          “seventy times seven”); the last word can be the start of one.
        </p>
        {error && <p className="text-sm whitespace-pre-line text-destructive">{error}</p>}

        {result && !result.done && (
          <div className="text-sm text-muted-foreground">
            Searching… {result.searched.toLocaleString()} of {result.total.toLocaleString()} sermons ·{" "}
            {result.found.length.toLocaleString()} place{result.found.length === 1 ? "" : "s"} so far
          </div>
        )}
        {result?.done && (
          <div className="text-sm text-muted-foreground">
            {result.found.length === 0
              ? `“${result.query}” wasn't found.`
              : `${result.more ? `The first ${result.found.length}` : result.found.length} place${result.found.length === 1 ? "" : "s"} in ${groups.length} sermon${groups.length === 1 ? "" : "s"}${result.more ? ": add words to narrow it" : ""}`}
          </div>
        )}

        {groups.map(([sourceId, found]) => {
          const source = sourceOf(project, sourceId);
          return (
            <section key={sourceId} className="space-y-1 rounded-lg border bg-background p-3">
              <div className="flex items-center gap-2 pb-1">
                <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, sourceId) }} />
                <span className="min-w-0 truncate text-sm font-medium">{sourceTitle(project, source)}</span>
                <span className="text-xs text-muted-foreground">{found.length}</span>
                <Button
                  size="xs"
                  variant="ghost"
                  className="ml-auto"
                  onClick={() => openTranscript(sourceId, found[0].start, result?.query ?? "")}
                >
                  <FileText /> Transcript
                </Button>
              </div>
              {found.map((s) => {
                const id = `said:${s.sourceId}:${s.wordStart}`;
                return (
                  <div key={s.wordStart} className="group flex items-start gap-2 rounded-md px-1 py-1 hover:bg-muted/60">
                    <Button size="icon-xs" variant="ghost" title="Listen" onClick={() => listen(s)}>
                      {player.loading === id ? <Loader2 className="animate-spin" /> : player.id === id ? <Square /> : <Play />}
                    </Button>
                    <button
                      className="w-14 shrink-0 pt-0.5 text-left text-xs text-muted-foreground tabular-nums"
                      title="Open the transcript here"
                      onClick={() => openTranscript(sourceId, s.start, result?.query ?? "")}
                    >
                      {formatTime(s.start)}
                    </button>
                    <button
                      className="min-w-0 flex-1 text-left text-[13px] leading-relaxed"
                      title="Open the transcript here"
                      onClick={() => openTranscript(sourceId, s.start, result?.query ?? "")}
                    >
                      <span className="text-muted-foreground">{s.before ? `…${s.before} ` : ""}</span>
                      <mark className="rounded-sm bg-yellow-200 px-0.5 text-foreground dark:bg-yellow-700/60">{s.text}</mark>
                      <span className="text-muted-foreground">{s.after ? ` ${s.after}…` : ""}</span>
                    </button>
                    {project.kind !== "readings" && <AddReading said={s} />}
                  </div>
                );
              })}
            </section>
          );
        })}
      </div>
    </div>
  );
}
