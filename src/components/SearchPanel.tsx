import { useEffect, useMemo, useRef, useState } from "react";
import { FileText, Loader2, Play, Search, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { api, errorMessage, type Said } from "@/lib/api";
import { useApp } from "@/lib/store";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, formatTime, sermonMatches, sourceColor, sourceTitle, valuesOf } from "@/lib/refs";
import { MultiSelect } from "./MultiSelect";

/** The most places shown */
const LIMIT = 500;

/** What was last searched for, and among whose sermons, kept while other tabs are open */
let lastQuery = "";
let lastSpeakers: string[] = [];
let lastSeries: string[] = [];

/** Searching every sermon's transcript for what was said */
export function SearchPanel() {
  const { project, openTranscript } = useApp();
  const player = usePlayer();
  const [query, setQuery] = useState(lastQuery);
  const [speakers, setSpeakers] = useState(lastSpeakers);
  const [series, setSeries] = useState(lastSeries);
  const [result, setResult] = useState<{ query: string; found: Said[]; more: boolean } | null>(null);
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
      api.searchTranscripts(p, query, among, LIMIT).then(
        (r) => {
          if (!live) return;
          setResult({ query, found: r.found, more: r.more });
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

  if (!project) return null;

  const listen = (s: Said) => {
    const source = project.sources.find((x) => x.id === s.sourceId);
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
        {(valuesOf(project, "speaker").length > 0 || valuesOf(project, "series").length > 0) && (
          <div className="grid gap-2 sm:grid-cols-2">
            {valuesOf(project, "speaker").length > 0 && (
              <MultiSelect
                options={valuesOf(project, "speaker").map((v) => ({ value: v, label: v }))}
                value={speakers}
                onChange={setSpeakers}
                placeholder="Every speaker"
                searchPlaceholder="Search speakers…"
              />
            )}
            {valuesOf(project, "series").length > 0 && (
              <MultiSelect
                options={valuesOf(project, "series").map((v) => ({ value: v, label: v }))}
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

        {result && (
          <div className="text-sm text-muted-foreground">
            {result.found.length === 0
              ? `“${result.query}” wasn't found.`
              : `${result.more ? `The first ${result.found.length}` : result.found.length} place${result.found.length === 1 ? "" : "s"} in ${groups.length} sermon${groups.length === 1 ? "" : "s"}${result.more ? ": add words to narrow it" : ""}`}
          </div>
        )}

        {groups.map(([sourceId, found]) => {
          const source = project.sources.find((s) => s.id === sourceId);
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
