import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { Download, FileText, Loader2, Play, Search, Square } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { api, errorMessage, type MatchKind, type ReadingRow } from "@/lib/api";
import { useApp } from "@/lib/store";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, formatTime, kindLabel, sourceColor } from "@/lib/refs";
import { kindClass } from "./MatchCard";

/** How many groups are shown before "Show more": sermons (each with many readings), or verses */
const PAGE = { sermon: 40, verse: 200 };

/** Every verse read in the project's sermons, by sermon or by verse */
export function ReadingsPanel() {
  const { project, matchSet, openTranscript, setTab } = useApp();
  const player = usePlayer();
  const [rows, setRows] = useState<ReadingRow[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"sermon" | "verse">("sermon");
  const [filter, setFilter] = useState("");
  const [kind, setKind] = useState<MatchKind | "all">("all");
  const [translation, setTranslation] = useState("all");
  const [shown, setShown] = useState(PAGE.sermon);

  // The rows (with the words said) come from the saved readings and the transcripts
  const sources = project?.sources;
  const files = project?.files;
  useEffect(() => {
    const p = useApp.getState().project;
    if (!p) return;
    let live = true;
    setError(null);
    api.readingRows(p, null).then(
      (r) => live && setRows(r),
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
  }, [matchSet, sources, files]);

  const kept = useMemo(() => {
    const words = filter.toLowerCase().split(/\s+/).filter(Boolean);
    return (rows ?? []).filter(
      (r) =>
        (kind === "all" || r.kind === kind) &&
        (translation === "all" || r.translation === translation) &&
        words.every((w) => r.reference.toLowerCase().includes(w)),
    );
  }, [rows, filter, kind, translation]);

  const bySermon = useMemo(() => {
    const groups = new Map<string, ReadingRow[]>();
    for (const r of kept) {
      const list = groups.get(r.sourceId);
      if (list) list.push(r);
      else groups.set(r.sourceId, [r]);
    }
    return [...groups.entries()];
  }, [kept]);

  const byVerse = useMemo(() => {
    const groups = new Map<string, ReadingRow[]>();
    for (const r of kept) {
      const key = `${r.book}.${r.chapter}.${r.verse}`;
      const list = groups.get(key);
      if (list) list.push(r);
      else groups.set(key, [r]);
    }
    return [...groups.values()].sort((a, b) => a[0].book - b[0].book || a[0].chapter - b[0].chapter || a[0].verse - b[0].verse);
  }, [kept]);

  const translations = useMemo(() => {
    const counts = new Map<string, number>();
    for (const r of rows ?? []) counts.set(r.translation, (counts.get(r.translation) ?? 0) + 1);
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [rows]);

  useEffect(() => setShown(PAGE[view]), [view, filter, kind, translation]);

  if (!project) return null;

  if (matchSet.analyzedAt === 0 || (rows && rows.length === 0 && !error)) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
        {project.sources.length === 0 ? "Add sermons, then search them." : matchSet.analyzedAt === 0 ? "Search the sermons to find the verses read." : "No verses were found in these sermons."}
        <Button variant="outline" onClick={() => setTab("sources")}>
          Go to Sermons
        </Button>
      </div>
    );
  }

  const exportAs = async (format: "csv" | "json") => {
    const path = await save({
      defaultPath: `${project.name}.${format}`,
      filters: [{ name: format === "csv" ? "CSV" : "JSON", extensions: [format] }],
    });
    if (!path) return;
    try {
      const written = await api.readingRows(project, path);
      toast.success(`Exported ${written.length.toLocaleString()} readings`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const listen = (r: ReadingRow) => {
    const source = project.sources.find((s) => s.id === r.sourceId);
    const audioPath = source ? audioPathOf(project, source) : "";
    if (!audioPath) {
      toast.info("This sermon has no audio");
      return;
    }
    void play(`row:${r.id}`, () => api.renderClip({ audioPath, start: r.start, end: r.end }), r.start);
  };
  const playIcon = (r: ReadingRow) =>
    player.loading === `row:${r.id}` ? <Loader2 className="animate-spin" /> : player.id === `row:${r.id}` ? <Square /> : <Play />;

  const verses = new Set((rows ?? []).map((r) => `${r.book}.${r.chapter}.${r.verse}`)).size;
  const books = new Set((rows ?? []).map((r) => r.book)).size;
  const sermons = new Set((rows ?? []).map((r) => r.sourceId)).size;
  const groups = view === "sermon" ? bySermon.length : byVerse.length;

  return (
    <div className="flex h-full flex-col">
      <div className="space-y-3 border-b px-6 py-3">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
          {rows ? (
            <span>
              <span className="font-semibold">{rows.length.toLocaleString()}</span> readings of{" "}
              <span className="font-semibold">{verses.toLocaleString()}</span> verses in {books} books, in {sermons} sermons
            </span>
          ) : (
            <span className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Reading the transcripts…
            </span>
          )}
          {translations.length > 1 && (
            <span className="text-xs text-muted-foreground">
              {translations.map(([t, n]) => `${t} ${n.toLocaleString()}`).join(" · ")} (the translation each fits best)
            </span>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" variant="outline" className="ml-auto" disabled={!rows}>
                <Download /> Export
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => exportAs("csv")}>CSV (a spreadsheet)</DropdownMenuItem>
              <DropdownMenuItem onClick={() => exportAs("json")}>JSON</DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <ToggleGroup type="single" variant="outline" size="sm" value={view} onValueChange={(v) => v && setView(v as "sermon" | "verse")}>
            <ToggleGroupItem value="sermon">By sermon</ToggleGroupItem>
            <ToggleGroupItem value="verse">By verse</ToggleGroupItem>
          </ToggleGroup>
          <div className="relative w-64">
            <Search className="absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input className="h-8 pl-8 text-sm" placeholder="Filter by reference (Romans 8)" value={filter} onChange={(e) => setFilter(e.target.value)} />
          </div>
          <Select value={kind} onValueChange={(v) => setKind(v as MatchKind | "all")}>
            <SelectTrigger size="sm" className="w-36">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="all">Every kind</SelectItem>
              <SelectItem value="exact">Exact</SelectItem>
              <SelectItem value="close">Close</SelectItem>
              <SelectItem value="partial">Partial</SelectItem>
            </SelectContent>
          </Select>
          {translations.length > 1 && (
            <Select value={translation} onValueChange={setTranslation}>
              <SelectTrigger size="sm" className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">Every translation</SelectItem>
                {translations.map(([t]) => (
                  <SelectItem key={t} value={t}>
                    {t}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          {rows && kept.length !== rows.length && (
            <span className="text-xs text-muted-foreground">{kept.length.toLocaleString()} shown</span>
          )}
        </div>
        {error && <p className="text-sm text-destructive">{error}</p>}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-5xl space-y-3 p-6">
          {view === "sermon"
            ? bySermon.slice(0, shown).map(([sourceId, list]) => (
                <section key={sourceId} className="rounded-lg border bg-background p-3">
                  <div className="flex items-center gap-2 pb-2">
                    <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, sourceId) }} />
                    <span className="min-w-0 truncate text-sm font-medium">{list[0].sermon}</span>
                    <span className="text-xs text-muted-foreground">{list.length}</span>
                    <Button size="xs" variant="ghost" className="ml-auto" onClick={() => openTranscript(sourceId)}>
                      <FileText /> Transcript
                    </Button>
                  </div>
                  <div className="space-y-0.5">
                    {list.map((r) => (
                      <div key={r.id} className="grid grid-cols-[1.5rem_3.5rem_9rem_1fr] items-start gap-2 rounded px-1 py-0.5 hover:bg-muted/60">
                        <Button size="icon-xs" variant="ghost" title="Listen" onClick={() => listen(r)}>
                          {playIcon(r)}
                        </Button>
                        <button
                          className="pt-0.5 text-left text-xs text-muted-foreground tabular-nums hover:text-foreground"
                          title="Open the transcript here"
                          onClick={() => openTranscript(r.sourceId, r.start)}
                        >
                          {formatTime(r.start)}
                        </button>
                        <div className="flex min-w-0 flex-col gap-0.5">
                          <span className="truncate text-sm font-medium">{r.reference}</span>
                          <span className="flex items-center gap-1 text-[11px] text-muted-foreground">
                            <span className={cn("rounded px-1 font-medium", kindClass[r.kind])} title={`${Math.round(r.accuracy * 100)}% accurate`}>
                              {kindLabel[r.kind]} {Math.round(r.coverage * 100)}%
                            </span>
                            <span title={r.alsoIn.length ? `Also fits ${r.alsoIn.join(", ")}` : undefined}>
                              {r.translation}
                              {r.alsoIn.length > 0 && ` +${r.alsoIn.length}`}
                            </span>
                          </span>
                        </div>
                        <p className="line-clamp-2 text-[13px] leading-relaxed text-muted-foreground">{r.said}</p>
                      </div>
                    ))}
                  </div>
                </section>
              ))
            : byVerse.slice(0, shown).map((list) => {
                const first = list[0];
                const fits = [...new Set(list.flatMap((r) => [r.translation, ...r.alsoIn]))];
                return (
                  <div key={first.id} className="grid grid-cols-[10rem_1fr] gap-3 rounded-lg border bg-background p-3">
                    <div>
                      <div className="text-sm font-medium">{first.reference}</div>
                      <div className="text-[11px] text-muted-foreground">
                        {list.length} reading{list.length === 1 ? "" : "s"} · {fits.join(", ")}
                      </div>
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                      {list.map((r) => (
                        <span key={r.id} className="flex items-center rounded-full border text-xs">
                          <Button size="icon-xs" variant="ghost" className="rounded-full" title="Listen" onClick={() => listen(r)}>
                            {playIcon(r)}
                          </Button>
                          <button
                            className="max-w-72 truncate py-0.5 pr-2.5 hover:underline"
                            title={`${r.said}\n\nOpen the transcript here`}
                            onClick={() => openTranscript(r.sourceId, r.start)}
                          >
                            <span className={cn("mr-1 rounded px-1 text-[10px] font-medium", kindClass[r.kind])}>{kindLabel[r.kind]}</span>
                            {r.sermon} <span className="text-muted-foreground tabular-nums">{formatTime(r.start)}</span>
                          </button>
                        </span>
                      ))}
                    </div>
                  </div>
                );
              })}
          {groups > shown && (
            <div className="text-center">
              <Button variant="outline" onClick={() => setShown(shown + PAGE[view])}>
                Show more ({(groups - shown).toLocaleString()} left)
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
