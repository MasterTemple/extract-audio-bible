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
import { api, errorMessage, type MatchKind, type ReadingRow } from "@/lib/api";
import { useApp } from "@/lib/store";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, formatTime, kindLabel, sourceColor } from "@/lib/refs";
import { kindClass } from "./MatchCard";

/** Past this many sermons, or verses, the list starts folded (by sermon, or by book) */
const FOLD_SERMONS = 15;
const FOLD_VERSES = 150;

/** What's folded in each project's views, kept while other tabs are open */
const folded = new Map<string, Set<string>>();

/** A level of the list that folds: its name, and how much is in it */
function Fold({
  open,
  onToggle,
  depth,
  label,
  rows,
  children,
}: {
  open: boolean;
  onToggle: () => void;
  depth: number;
  label: React.ReactNode;
  rows: ReadingRow[];
  children?: React.ReactNode;
}) {
  const verses = new Set(rows.map((r) => `${r.book}.${r.chapter}.${r.verse}`)).size;
  return (
    <div className="flex items-center gap-1.5" style={{ paddingLeft: depth * 16 }}>
      <button className="flex min-w-0 items-center gap-1.5 rounded px-1 py-1 text-left hover:bg-muted" onClick={onToggle}>
        <ChevronRight className={cn("size-4 shrink-0 text-muted-foreground transition-transform", open && "rotate-90")} />
        <span className={cn("truncate", depth === 0 ? "text-sm font-semibold" : "text-sm font-medium")}>{label}</span>
        <span className="shrink-0 text-xs text-muted-foreground tabular-nums">
          {rows.length.toLocaleString()} reading{rows.length === 1 ? "" : "s"}
          {verses !== rows.length && ` · ${verses.toLocaleString()} verse${verses === 1 ? "" : "s"}`}
        </span>
      </button>
      {children}
    </div>
  );
}

const testamentOf = (book: number) => (book <= 39 ? "Old Testament" : book <= 66 ? "New Testament" : "Other books");
const bookOf = (r: ReadingRow) => r.reference.replace(/ \d+:\d+$/, "");

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
  // What's folded: sermons (`s:id`), testaments (`t:name`), books (`b:id`), chapters (`c:book.chapter`)
  const [fold, setFold] = useState<Set<string> | null>(() => (project ? folded.get(project.id) ?? null : null));
  const changeFold = (next: Set<string>) => {
    setFold(next);
    if (project) folded.set(project.id, next);
  };

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

  // Testament, then book, then chapter, then each verse's readings
  const tree = useMemo(() => {
    const testaments: { name: string; rows: ReadingRow[]; books: { id: number; name: string; rows: ReadingRow[]; chapters: { chapter: number; rows: ReadingRow[]; verses: ReadingRow[][] }[] }[] }[] = [];
    for (const verse of byVerse) {
      const r = verse[0];
      const tName = testamentOf(r.book);
      let t = testaments[testaments.length - 1];
      if (t?.name !== tName) testaments.push((t = { name: tName, rows: [], books: [] }));
      let b = t.books[t.books.length - 1];
      if (b?.id !== r.book) t.books.push((b = { id: r.book, name: bookOf(r), rows: [], chapters: [] }));
      let c = b.chapters[b.chapters.length - 1];
      if (c?.chapter !== r.chapter) b.chapters.push((c = { chapter: r.chapter, rows: [], verses: [] }));
      c.verses.push(verse);
      for (const list of [t.rows, b.rows, c.rows]) list.push(...verse);
    }
    return testaments;
  }, [byVerse]);

  // Until something is folded or unfolded, a long list starts folded
  const shut = useMemo(() => {
    if (fold) return fold;
    const start = new Set<string>();
    if (bySermon.length > FOLD_SERMONS) for (const [id] of bySermon) start.add(`s:${id}`);
    if (byVerse.length > FOLD_VERSES) for (const t of tree) for (const b of t.books) start.add(`b:${b.id}`);
    return start;
  }, [fold, bySermon, byVerse, tree]);
  // A reference typed in shows everything it finds
  const searching = filter.trim() !== "";
  const isOpen = (key: string) => searching || !shut.has(key);
  const toggle = (key: string) => {
    const next = new Set(shut);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    changeFold(next);
  };
  const foldAll = () => {
    const next = new Set(shut);
    if (view === "sermon") for (const [id] of bySermon) next.add(`s:${id}`);
    else
      for (const t of tree) {
        next.add(`t:${t.name}`);
        for (const b of t.books) {
          next.add(`b:${b.id}`);
          for (const c of b.chapters) next.add(`c:${b.id}.${c.chapter}`);
        }
      }
    changeFold(next);
  };
  const unfoldAll = () => {
    const prefixes = view === "sermon" ? ["s:"] : ["t:", "b:", "c:"];
    changeFold(new Set([...shut].filter((k) => !prefixes.some((p) => k.startsWith(p)))));
  };

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

  const verses = new Set((rows ?? []).map((r) => `${r.book}.${r.chapter}.${r.verse}`)).size;
  const books = new Set((rows ?? []).map((r) => r.book)).size;
  const sermons = new Set((rows ?? []).map((r) => r.sourceId)).size;

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
          {view === "sermon"
            ? bySermon.map(([sourceId, list]) => {
                const open = isOpen(`s:${sourceId}`);
                return (
                  <section key={sourceId} className={cn("rounded-lg border bg-background px-2 py-1", open && "pb-2")}>
                    <Fold
                      open={open}
                      onToggle={() => toggle(`s:${sourceId}`)}
                      depth={0}
                      rows={list}
                      label={
                        <span className="flex items-center gap-2">
                          <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, sourceId) }} />
                          {list[0].sermon}
                        </span>
                      }
                    >
                      <Button size="xs" variant="ghost" className="ml-auto" onClick={() => openTranscript(sourceId)}>
                        <FileText /> Transcript
                      </Button>
                    </Fold>
                    {open && <div className="space-y-0.5 pt-1">{list.map(sermonRow)}</div>}
                  </section>
                );
              })
            : tree.map((t) => {
                const tOpen = isOpen(`t:${t.name}`);
                return (
                  <section key={t.name} className="rounded-lg border bg-background px-2 py-1">
                    <Fold open={tOpen} onToggle={() => toggle(`t:${t.name}`)} depth={0} rows={t.rows} label={t.name} />
                    {tOpen &&
                      t.books.map((b) => {
                        const bOpen = isOpen(`b:${b.id}`);
                        return (
                          <div key={b.id}>
                            <Fold open={bOpen} onToggle={() => toggle(`b:${b.id}`)} depth={1} rows={b.rows} label={b.name} />
                            {bOpen &&
                              b.chapters.map((c) => {
                                const cOpen = isOpen(`c:${b.id}.${c.chapter}`);
                                return (
                                  <div key={c.chapter}>
                                    <Fold
                                      open={cOpen}
                                      onToggle={() => toggle(`c:${b.id}.${c.chapter}`)}
                                      depth={2}
                                      rows={c.rows}
                                      label={`${b.name} ${c.chapter}`}
                                    />
                                    {cOpen && <div className="space-y-1 pt-0.5 pb-1.5 pl-16">{c.verses.map(verseRow)}</div>}
                                  </div>
                                );
                              })}
                          </div>
                        );
                      })}
                  </section>
                );
              })}
        </div>
      </div>
    </div>
  );
}
