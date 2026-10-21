import { useEffect, useMemo, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { ChevronRight, ChevronsDownUp, ChevronsUpDown, Download, FileText, Loader2, Play, Search, Square } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuContent, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { api, errorMessage, type MatchKind, type Overview, type Project, type ReadingRow, type RowFilter, type RowGroup } from "@/lib/api";
import { useApp } from "@/lib/store";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, formatTime, kindLabel, sourceColor } from "@/lib/refs";
import { kindClass } from "./MatchCard";
import { MultiSelect } from "./MultiSelect";

/** Past this many sermons, or readings, the list starts folded (by sermon, or by book and chapter) */
const FOLD_SERMONS = 15;
const FOLD_READINGS = 500;
/** A reference typed in opens everything it finds, up to this many readings */
const OPEN_FOUND_UP_TO = 2000;
/** Sermons listed at first, and more each time more are asked for */
const SERMON_PAGE = 100;

/** What's folded in each project's views, kept while other tabs are open */
const folded = new Map<string, Set<string>>();

/** Each group's rows as read, by what they were read for (the project, its search, the filter, the group) */
const rowCache = new Map<string, ReadingRow[]>();

const testamentOf = (book: number) => (book <= 39 ? "Old Testament" : book <= 66 ? "New Testament" : "Other books");

/** A level of the list that folds: its name, and how much is in it */
function Fold({
  open,
  onToggle,
  depth,
  label,
  readings,
  verses,
  children,
}: {
  open: boolean;
  onToggle: () => void;
  depth: number;
  label: React.ReactNode;
  readings: number;
  verses: number;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-1.5" style={{ paddingLeft: depth * 16 }}>
      <button className="flex min-w-0 items-center gap-1.5 rounded px-1 py-1 text-left hover:bg-muted" onClick={onToggle}>
        <ChevronRight className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-90")} />
        <span className={cn("truncate", depth === 0 ? "text-sm font-semibold" : "text-sm font-medium")}>{label}</span>
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
          {readings.toLocaleString()} reading{readings === 1 ? "" : "s"}
          {verses !== readings && ` · ${verses.toLocaleString()} verse${verses === 1 ? "" : "s"}`}
        </span>
      </button>
      {children}
    </div>
  );
}

/** A group's readings, read when it's opened, given to `children` once they're here */
function GroupRows({
  project,
  filter,
  group,
  readFor,
  children,
}: {
  project: Project;
  filter: RowFilter;
  group: RowGroup;
  /** What the readings were read for (changes when they're searched again or filtered) */
  readFor: string;
  children: (rows: ReadingRow[]) => React.ReactNode;
}) {
  const key = `${readFor}|${JSON.stringify(group)}`;
  const [rows, setRows] = useState<{ key: string; rows: ReadingRow[] } | null>(() => {
    const cached = rowCache.get(key);
    return cached ? { key, rows: cached } : null;
  });
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    const cached = rowCache.get(key);
    if (cached) {
      setRows({ key, rows: cached });
      return;
    }
    let live = true;
    api.readingsRows(project.id, filter, group).then(
      (found) => {
        rowCache.set(key, found);
        if (live) setRows({ key, rows: found });
      },
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
    // `key` covers the project, the filter, and the group
  }, [key]);
  if (error) return <p className="px-2 py-1 text-sm text-destructive">{error}</p>;
  if (rows?.key !== key)
    return (
      <div className="flex items-center gap-2 px-8 py-1.5 text-xs text-muted-foreground">
        <Loader2 className="size-3.5 animate-spin" /> Reading…
      </div>
    );
  return <>{children(rows.rows)}</>;
}

/** Every verse read in the project's sermons, by sermon or by verse */
export function ReadingsPanel() {
  const { project, summary, openTranscript, setTab } = useApp();
  const player = usePlayer();
  const [overview, setOverview] = useState<Overview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"sermon" | "verse">("sermon");
  const [filterText, setFilterText] = useState("");
  const [reference, setReference] = useState("");
  const [kind, setKind] = useState<MatchKind | "all">("all");
  const [translation, setTranslation] = useState("all");
  const [speakers, setSpeakers] = useState<string[]>([]);
  const [series, setSeries] = useState<string[]>([]);
  const [sermonsShown, setSermonsShown] = useState(SERMON_PAGE);
  // What's folded: sermons (`s:id`), testaments (`t:name`), books (`b:id`), chapters (`c:book.chapter`)
  const [fold, setFold] = useState<Set<string> | null>(() => (project ? folded.get(project.id) ?? null : null));
  const changeFold = (next: Set<string>) => {
    setFold(next);
    if (project) folded.set(project.id, next);
  };

  // A reference is looked for once typing pauses
  useEffect(() => {
    const t = setTimeout(() => setReference(filterText.trim()), 250);
    return () => clearTimeout(t);
  }, [filterText]);

  const filter: RowFilter = useMemo(
    () => ({
      kind: kind === "all" ? null : kind,
      translation: translation === "all" ? null : translation,
      speakers,
      series,
      reference,
    }),
    [kind, translation, speakers, series, reference],
  );
  const projectId = project?.id;
  const readFor = `${projectId}|${summary.analyzedAt}|${summary.readings}|${JSON.stringify(filter)}`;

  // The readings counted and grouped; each group's are read when it's opened
  useEffect(() => {
    if (!projectId || summary.analyzedAt === 0) return;
    let live = true;
    setError(null);
    rowCache.clear();
    api.readingsOverview(projectId, filter).then(
      (o) => {
        if (!live) return;
        setOverview(o);
        setSermonsShown(SERMON_PAGE);
      },
      (e) => live && setError(errorMessage(e)),
    );
    return () => {
      live = false;
    };
    // `readFor` covers the project, its search, and the filter
  }, [readFor]);

  // Testament, then book, then chapter (each chapter's verses read when it's opened)
  const tree = useMemo(() => {
    const testaments: { name: string; readings: number; verses: number; books: Overview["byBook"] }[] = [];
    for (const b of overview?.byBook ?? []) {
      const name = testamentOf(b.book);
      let t = testaments[testaments.length - 1];
      if (t?.name !== name) testaments.push((t = { name, readings: 0, verses: 0, books: [] }));
      t.books.push(b);
      t.readings += b.readings;
      t.verses += b.verses;
    }
    return testaments;
  }, [overview]);

  // Until something is folded or unfolded, a long list starts folded
  const shut = useMemo(() => {
    if (fold) return fold;
    const start = new Set<string>();
    if (!overview) return start;
    if (overview.bySermon.length > FOLD_SERMONS) for (const g of overview.bySermon) start.add(`s:${g.id}`);
    if (overview.shown > FOLD_READINGS)
      for (const b of overview.byBook) {
        start.add(`b:${b.book}`);
        for (const c of b.chapters) start.add(`c:${b.book}.${c.chapter}`);
      }
    return start;
  }, [fold, overview]);
  // A reference typed in shows everything it finds, when that isn't too much to read
  const searching = reference !== "" && (overview?.shown ?? 0) <= OPEN_FOUND_UP_TO;
  const isOpen = (key: string) => searching || !shut.has(key);
  const toggle = (key: string) => {
    const next = new Set(shut);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    changeFold(next);
  };
  const foldAll = () => {
    const next = new Set(shut);
    if (view === "sermon") for (const g of overview?.bySermon ?? []) next.add(`s:${g.id}`);
    else
      for (const t of tree) {
        next.add(`t:${t.name}`);
        for (const b of t.books) {
          next.add(`b:${b.book}`);
          for (const c of b.chapters) next.add(`c:${b.book}.${c.chapter}`);
        }
      }
    changeFold(next);
  };
  const unfoldAll = () => {
    const prefixes = view === "sermon" ? ["s:"] : ["t:", "b:", "c:"];
    changeFold(new Set([...shut].filter((k) => !prefixes.some((p) => k.startsWith(p)))));
  };

  if (!project) return null;

  if (summary.analyzedAt === 0 || (overview && overview.readings === 0 && !error)) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 text-sm text-muted-foreground">
        {project.sources.length === 0 ? "Add sermons, then search them." : summary.analyzedAt === 0 ? "Search the sermons to find the verses read." : "No verses were found in these sermons."}
        <Button variant="outline" onClick={() => setTab("sources")}>
          Go to Sermons
        </Button>
      </div>
    );
  }

  const filtered = overview !== null && overview.shown !== overview.readings;
  const exportAs = async (format: "csv" | "json", only: RowFilter) => {
    const path = await save({
      defaultPath: `${project.name}.${format}`,
      filters: [{ name: format === "csv" ? "CSV" : "JSON", extensions: [format] }],
    });
    if (!path) return;
    try {
      const written = await api.exportReadings(project.id, only, path);
      toast.success(`Exported ${written.toLocaleString()} readings`);
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

  /** Other verses a reading could as well be, shown after its reference */
  const alsoAt = (r: ReadingRow) =>
    r.alsoAt.length > 0 && (
      <span className="font-normal text-muted-foreground" title="Worded the same; the sermon doesn't say which">
        {" "}
        or {r.alsoAt.join(", ")}
      </span>
    );

  const sermonRow = (r: ReadingRow) => (
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
        <span className="text-sm font-medium">
          {r.reference}
          {alsoAt(r)}
        </span>
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
  );

  const verseRow = (list: ReadingRow[]) => {
    const first = list[0];
    const fits = [...new Set(list.flatMap((r) => [r.translation, ...r.alsoIn]))];
    return (
      <div key={first.id} className="grid grid-cols-[10rem_1fr] gap-3 rounded-md border bg-background p-2">
        <div>
          <div className="text-sm font-medium">
            {first.reference}
            {alsoAt(first)}
          </div>
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
  };

  /** A chapter's readings, by verse */
  const byVerse = (rows: ReadingRow[]) => {
    const verses: ReadingRow[][] = [];
    for (const r of rows) {
      const last = verses[verses.length - 1];
      if (last && last[0].verse === r.verse) last.push(r);
      else verses.push([r]);
    }
    return verses.map(verseRow);
  };

  const sermons = overview?.bySermon ?? [];

  return (
    <div className="flex h-full flex-col">
      <div className="space-y-3 border-b px-6 py-3">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-sm">
          {overview ? (
            <span>
              <span className="font-semibold">{overview.readings.toLocaleString()}</span> readings of{" "}
              <span className="font-semibold">{overview.verses.toLocaleString()}</span> verses in {overview.books} books, in{" "}
              {overview.sermons.toLocaleString()} sermons
            </span>
          ) : (
            <span className="flex items-center gap-2 text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Counting the readings…
            </span>
          )}
          {overview && overview.translations.length > 1 && (
            <span className="text-xs text-muted-foreground">
              {overview.translations.map(([t, n]) => `${t} ${n.toLocaleString()}`).join(" · ")} (the translation each fits best)
            </span>
          )}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button size="sm" variant="outline" className="ml-auto" disabled={!overview}>
                <Download /> Export
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => exportAs("csv", {})}>CSV (a spreadsheet)</DropdownMenuItem>
              <DropdownMenuItem onClick={() => exportAs("json", {})}>JSON</DropdownMenuItem>
              {filtered && (
                <>
                  <DropdownMenuItem onClick={() => exportAs("csv", filter)}>CSV of what's shown</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => exportAs("json", filter)}>JSON of what's shown</DropdownMenuItem>
                </>
              )}
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
            <Input className="h-8 pl-8 text-sm" placeholder="Filter by reference (Romans 8)" value={filterText} onChange={(e) => setFilterText(e.target.value)} />
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
          {overview && overview.translations.length > 1 && (
            <Select value={translation} onValueChange={setTranslation}>
              <SelectTrigger size="sm" className="w-40">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="all">Every translation</SelectItem>
                {overview.translations.map(([t]) => (
                  <SelectItem key={t} value={t}>
                    {t}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          )}
          {overview && overview.speakers.length > 0 && (
            <MultiSelect
              className="w-56"
              options={overview.speakers.map((v) => ({ value: v, label: v }))}
              value={speakers}
              onChange={setSpeakers}
              placeholder="Every speaker"
              searchPlaceholder="Search speakers…"
            />
          )}
          {overview && overview.series.length > 0 && (
            <MultiSelect
              className="w-56"
              options={overview.series.map((v) => ({ value: v, label: v }))}
              value={series}
              onChange={setSeries}
              placeholder="Every series"
              searchPlaceholder="Search series…"
            />
          )}
          {filtered && <span className="text-xs text-muted-foreground">{overview.shown.toLocaleString()} shown</span>}
          <div className="ml-auto flex items-center gap-1">
            {searching && <span className="text-xs text-muted-foreground">Showing everything found</span>}
            <Button size="sm" variant="ghost" disabled={searching} onClick={unfoldAll} title="Open every sermon, or every testament, book, and chapter">
              <ChevronsUpDown /> Expand all
            </Button>
            <Button size="sm" variant="ghost" disabled={searching} onClick={foldAll} title="Fold every sermon, or every testament, book, and chapter">
              <ChevronsDownUp /> Collapse all
            </Button>
          </div>
        </div>
        {error && <p className="text-sm text-destructive">{error}</p>}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-5xl space-y-1 p-6">
          {view === "sermon" ? (
            <>
              {sermons.slice(0, sermonsShown).map((g) => {
                const open = isOpen(`s:${g.id}`);
                return (
                  <section key={g.id} className={cn("rounded-lg border bg-background px-2 py-1", open && "pb-2")}>
                    <Fold
                      open={open}
                      onToggle={() => toggle(`s:${g.id}`)}
                      depth={0}
                      readings={g.readings}
                      verses={g.verses}
                      label={
                        <span className="flex items-center gap-2">
                          <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, g.id) }} />
                          {g.title}
                          {(g.speaker || g.series) && (
                            <span className="text-xs font-normal text-muted-foreground">{[g.speaker, g.series].filter(Boolean).join(" · ")}</span>
                          )}
                        </span>
                      }
                    >
                      <Button size="xs" variant="ghost" className="ml-auto" onClick={() => openTranscript(g.id)}>
                        <FileText /> Transcript
                      </Button>
                    </Fold>
                    {open && (
                      <GroupRows project={project} filter={filter} group={{ by: "sermon", id: g.id }} readFor={readFor}>
                        {(rows) => <div className="space-y-0.5 pt-1">{rows.map(sermonRow)}</div>}
                      </GroupRows>
                    )}
                  </section>
                );
              })}
              {sermons.length > sermonsShown && (
                <Button variant="outline" className="w-full" onClick={() => setSermonsShown((n) => n + SERMON_PAGE * 2)}>
                  Show {Math.min(SERMON_PAGE * 2, sermons.length - sermonsShown).toLocaleString()} more sermons (
                  {(sermons.length - sermonsShown).toLocaleString()} not shown)
                </Button>
              )}
            </>
          ) : (
            tree.map((t) => {
              const tOpen = isOpen(`t:${t.name}`);
              return (
                <section key={t.name} className="rounded-lg border bg-background px-2 py-1">
                  <Fold open={tOpen} onToggle={() => toggle(`t:${t.name}`)} depth={0} readings={t.readings} verses={t.verses} label={t.name} />
                  {tOpen &&
                    t.books.map((b) => {
                      const bOpen = isOpen(`b:${b.book}`);
                      return (
                        <div key={b.book}>
                          <Fold open={bOpen} onToggle={() => toggle(`b:${b.book}`)} depth={1} readings={b.readings} verses={b.verses} label={b.name} />
                          {bOpen &&
                            b.chapters.map((c) => {
                              const cOpen = isOpen(`c:${b.book}.${c.chapter}`);
                              return (
                                <div key={c.chapter}>
                                  <Fold
                                    open={cOpen}
                                    onToggle={() => toggle(`c:${b.book}.${c.chapter}`)}
                                    depth={2}
                                    readings={c.readings}
                                    verses={c.verses}
                                    label={`${b.name} ${c.chapter}`}
                                  />
                                  {cOpen && (
                                    <GroupRows
                                      project={project}
                                      filter={filter}
                                      group={{ by: "chapter", book: b.book, chapter: c.chapter }}
                                      readFor={readFor}
                                    >
                                      {(rows) => <div className="space-y-1 pt-0.5 pb-1.5 pl-16">{byVerse(rows)}</div>}
                                    </GroupRows>
                                  )}
                                </div>
                              );
                            })}
                        </div>
                      );
                    })}
                </section>
              );
            })
          )}
        </div>
      </div>
    </div>
  );
}
