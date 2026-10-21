import { useEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { AudioLines, CloudDownload, FileJson, FilePlus, FileText, FolderPlus, Loader2, ScanSearch, Tags, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, errorMessage, type Progress as ProgressEvent } from "@/lib/api";
import { useApp } from "@/lib/store";
import { ScopeEditor, TranslationPicker } from "./ScopeEditor";
import { audioPathOf, fileIdOf, fileMetadataOf, fileName, formatTime, sourceColor, valuesOf } from "@/lib/refs";

export function SourcesPanel() {
  const { project, update, setProject, resetMatches, summary, flush, setTab, outline, go, setFileMetadata, openTranscript, bibles } =
    useApp();
  const [bookChanged, setBookChanged] = useState(false);
  // Files the last add left out: audio without a transcript, and JSON that isn't one
  const [leftOut, setLeftOut] = useState<{ unpaired: string[]; skipped: string[] }>({ unpaired: [], skipped: [] });
  const [progress, setProgress] = useState<ProgressEvent | null>(null);

  useEffect(() => {
    const off = listen<ProgressEvent>("analyze-progress", (e) => setProgress(e.payload));
    return () => void off.then((f) => f());
  }, []);

  // Only the sermons on screen are drawn (there can be tens of thousands)
  const table = useRef<HTMLDivElement>(null);
  const sources = project?.sources;
  const rows = useVirtualizer({
    count: sources?.length ?? 0,
    getScrollElement: () => table.current,
    estimateSize: () => 58,
    getItemKey: (i) => sources?.[i]?.id ?? i,
    overscan: 12,
  });
  // How many chosen verses use each sermon, and the names given, worked out once for them all
  const selections = project?.selections;
  const used = useMemo(() => {
    const counts = new Map<string, number>();
    for (const s of Object.values(selections ?? {})) counts.set(s.sourceId, (counts.get(s.sourceId) ?? 0) + 1);
    return counts;
  }, [selections]);
  const files = project?.files;
  const [speakerValues, seriesValues] = useMemo(() => {
    const p = useApp.getState().project;
    return p ? [valuesOf(p, "speaker"), valuesOf(p, "series")] : [[], []];
  }, [sources, files]);

  // Searched as soon as it's shown, when it was just made from another project's sermons
  const analyzeNow = useRef<() => Promise<void>>(async () => undefined);
  const searchOnOpen = useApp((s) => s.searchOnOpen);
  useEffect(() => {
    if (!searchOnOpen) return;
    useApp.setState({ searchOnOpen: false });
    void analyzeNow.current();
  }, [searchOnOpen]);

  if (!project) return null;

  const add = async (directory: boolean) => {
    const picked = await open({
      multiple: true,
      directory,
      filters: directory
        ? undefined
        : [{ name: "Sermon audio and WhisperX JSON", extensions: ["mp3", "m4a", "wav", "flac", "ogg", "json", "gz"] }],
    });
    if (!picked) return;
    await flush();
    try {
      const result = await api.addSources(project.id, Array.isArray(picked) ? picked : [picked]);
      setProject(result.project, `add ${result.added} sermon${result.added === 1 ? "" : "s"}`);
      setLeftOut({ unpaired: result.unpaired, skipped: result.skipped });
      toast.success(`Added ${result.added} sermon${result.added === 1 ? "" : "s"}`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const importInfo = async () => {
    const path = await open({ filters: [{ name: "Titles and passages (JSON)", extensions: ["json"] }] });
    if (typeof path !== "string") return;
    try {
      const given = await api.readFileMetadata(path);
      setFileMetadata(given);
      const ids = new Set(project.sources.map(fileIdOf));
      const matched = Object.keys(given).filter((id) => ids.has(id)).length;
      toast.success(`Read ${Object.keys(given).length} sermons' information (${matched} in this project)`);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const analyze = (analyzeNow.current = async () => {
    await flush();
    setProgress({ done: 0, total: project.sources.length, message: "Starting" });
    try {
      const result = await api.analyze(useApp.getState().project!.id);
      setProject(result.project, "search the sermons");
      await resetMatches(result.summary);
      for (const e of result.errors) toast.error(e);
      setBookChanged(false);
      toast.success(`Found ${result.summary.readings.toLocaleString()} readings`, {
        action: { label: "View verses", onClick: () => setTab("verses") },
      });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setProgress(null);
    }
  });

  const remove = (id: string) => {
    const used = Object.values(project.selections).filter((s) => s.sourceId === id).length;
    if (used > 0 && !confirm(`${used} chosen verses use this sermon and will be unchosen. Remove it?`)) return;
    update((p) => ({
      ...p,
      sources: p.sources.filter((s) => s.id !== id),
      selections: Object.fromEntries(Object.entries(p.selections).filter(([, s]) => s.sourceId !== id)),
    }), "remove a sermon");
    // Its readings are left out once the project is saved
  };

  const stale = project.sources.some((s) => s.wordCount === 0);

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto max-w-6xl space-y-4 p-6">
        <Card>
          <CardHeader className="flex flex-row items-start justify-between gap-4">
            <div className="space-y-1.5">
              <CardTitle>Sermons</CardTitle>
              <CardDescription>
                Each sermon is its audio and the transcript made from it (WhisperX or Let's Church JSON), paired by file
                name (<code className="text-xs">sermon.mp3</code> + <code className="text-xs">sermon.json</code>). Add
                files, or a folder to pair everything in it. A transcript alone is a sermon too: it can be searched, and
                its audio downloaded from where you say below. Titles and passages are kept by each sermon's id (its
                transcript's, once searched), with their speakers and series, and can be read from JSON:{" "}
                <code className="text-xs">{'{"<id>": {"title": …, "passage": …, "speaker": …, "series": …}}'}</code>.
              </CardDescription>
            </div>
            <div className="flex shrink-0 gap-2">
              <Button variant="outline" size="sm" onClick={importInfo} title="Read titles and passages by sermon id from a JSON file">
                <Tags /> Import titles
              </Button>
              <Button variant="outline" size="sm" onClick={() => add(false)}>
                <FilePlus /> Add files
              </Button>
              <Button variant="outline" size="sm" onClick={() => add(true)}>
                <FolderPlus /> Add folder
              </Button>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center gap-3">
              <div className="w-40 shrink-0 text-sm">Download audio from</div>
              <Input
                className="max-w-xl font-mono text-xs"
                placeholder="https://example.com/audio/{id}.mp3"
                value={project.settings.audioUrl ?? ""}
                onChange={(e) => update((p) => ({ ...p, settings: { ...p.settings, audioUrl: e.target.value } }), "change where audio downloads from", "audioUrl")}
              />
              <span className="text-xs text-muted-foreground">
                For sermons with only a transcript: <code>{"{id}"}</code> is the sermon's id. It's downloaded when first played.
              </span>
            </div>
            {(
              [
                [leftOut.unpaired, "audio file had no transcript", "audio files had no transcript"],
                [leftOut.skipped, "JSON file isn't a transcript", "JSON files aren't transcripts"],
              ] as const
            ).map(
              ([files, one, many]) =>
                files.length > 0 && (
                  <Alert key={one}>
                    <AlertTitle>
                      {files.length} {files.length === 1 ? one : many}
                    </AlertTitle>
                    <AlertDescription>
                      <div className="max-h-24 overflow-auto text-xs">
                        {files.map((u) => (
                          <div key={u} className="truncate" title={u}>
                            {fileName(u)}
                          </div>
                        ))}
                      </div>
                    </AlertDescription>
                  </Alert>
                ),
            )}
            {project.sources.length === 0 ? (
              <div className="rounded-lg border border-dashed p-10 text-center text-sm text-muted-foreground">
                No sermons yet.
              </div>
            ) : (
              <div ref={table} className="max-h-[70vh] overflow-auto rounded-md border">
                <Table className="table-fixed">
                  <colgroup>
                    <col className="w-[22%]" />
                    <col className="w-[13%]" />
                    <col className="w-[12%]" />
                    <col className="w-[10%]" />
                    <col className="w-[17%]" />
                    <col className="w-[6%]" />
                    <col className="w-[6%]" />
                    <col className="w-[6%]" />
                    <col className="w-[4%]" />
                    <col className="w-[6rem]" />
                  </colgroup>
                  <TableHeader className="sticky top-0 z-10 bg-card">
                    <TableRow>
                      <TableHead>Sermon</TableHead>
                      <TableHead>Passage</TableHead>
                      <TableHead>Speaker</TableHead>
                      <TableHead>Series</TableHead>
                      <TableHead>Files</TableHead>
                      <TableHead className="text-right">Length</TableHead>
                      <TableHead className="text-right">Words</TableHead>
                      <TableHead className="text-right">Readings</TableHead>
                      <TableHead className="text-right">Used</TableHead>
                      <TableHead />
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {rows.getVirtualItems().length > 0 && (
                      <tr aria-hidden style={{ height: rows.getVirtualItems()[0].start }} />
                    )}
                    {rows.getVirtualItems().map((item) => {
                      const s = project.sources[item.index];
                      const fileId = fileIdOf(s);
                      const meta = fileMetadataOf(project, s);
                      return (
                        <TableRow key={item.key} data-index={item.index} ref={rows.measureElement}>
                          <TableCell>
                            <div className="flex items-center gap-2 font-medium">
                              <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, s.id) }} />
                              <input
                                className="w-full min-w-0 bg-transparent outline-none placeholder:text-foreground/70 focus:underline"
                                value={meta.title ?? ""}
                                placeholder={s.label}
                                title="Its title (its file's name if none is given)"
                                onChange={(e) => setFileMetadata({ [fileId]: { title: e.target.value } })}
                              />
                            </div>
                            <div className="ml-4.5 truncate text-[11px] text-muted-foreground" title="The id its title and passage are kept by">
                              {fileId}
                            </div>
                          </TableCell>
                          <TableCell>
                            <input
                              className="w-full min-w-0 bg-transparent text-sm outline-none placeholder:text-muted-foreground/60 focus:underline"
                              value={meta.passage ?? ""}
                              placeholder="Add passage"
                              onChange={(e) => setFileMetadata({ [fileId]: { passage: e.target.value } })}
                            />
                          </TableCell>
                          <TableCell>
                            <input
                              className="w-full min-w-0 bg-transparent text-sm outline-none placeholder:text-muted-foreground/60 focus:underline"
                              value={meta.speaker ?? ""}
                              placeholder="Add speaker"
                              list="sermon-speakers"
                              onChange={(e) => setFileMetadata({ [fileId]: { speaker: e.target.value } })}
                            />
                          </TableCell>
                          <TableCell>
                            <input
                              className="w-full min-w-0 bg-transparent text-sm outline-none placeholder:text-muted-foreground/60 focus:underline"
                              value={meta.series ?? ""}
                              placeholder="Add series"
                              list="sermon-series"
                              onChange={(e) => setFileMetadata({ [fileId]: { series: e.target.value } })}
                            />
                          </TableCell>
                          <TableCell className="overflow-hidden text-xs text-muted-foreground">
                            {s.audioPath ? (
                              <div className="flex items-center gap-1 truncate" title={s.audioPath}>
                                <AudioLines className="size-3 shrink-0" /> {fileName(s.audioPath)}
                              </div>
                            ) : audioPathOf(project, s) ? (
                              <div className="flex items-center gap-1 truncate" title={`Downloaded when needed from ${audioPathOf(project, s)}`}>
                                <CloudDownload className="size-3 shrink-0" /> {audioPathOf(project, s)}
                              </div>
                            ) : (
                              <div className="flex items-center gap-1 text-amber-600">
                                <AudioLines className="size-3 shrink-0" /> No audio: set where to download it from
                              </div>
                            )}
                            <div className="flex items-center gap-1 truncate" title={s.transcriptPath}>
                              <FileJson className="size-3 shrink-0" /> {fileName(s.transcriptPath)}
                            </div>
                          </TableCell>
                          <TableCell className="text-right tabular-nums">{s.duration ? formatTime(s.duration) : "–"}</TableCell>
                          <TableCell className="text-right tabular-nums">{s.wordCount ? s.wordCount.toLocaleString() : "–"}</TableCell>
                          <TableCell className="text-right tabular-nums">{s.wordCount ? s.matchCount : "–"}</TableCell>
                          <TableCell className="text-right tabular-nums">{used.get(s.id) ?? 0}</TableCell>
                          <TableCell className="text-right whitespace-nowrap">
                            <Button variant="ghost" size="icon-sm" title="Read its transcript" onClick={() => openTranscript(s.id)}>
                              <FileText />
                            </Button>
                            <Button variant="ghost" size="icon-sm" title="Remove" onClick={() => remove(s.id)}>
                              <Trash2 />
                            </Button>
                          </TableCell>
                        </TableRow>
                      );
                    })}
                    {rows.getVirtualItems().length > 0 && (
                      <tr
                        aria-hidden
                        style={{ height: rows.getTotalSize() - rows.getVirtualItems()[rows.getVirtualItems().length - 1].end }}
                      />
                    )}
                  </TableBody>
                </Table>
              </div>
            )}
            {/* Names already given, suggested as each is typed */}
            <datalist id="sermon-speakers">
              {speakerValues.map((v) => (
                <option key={v} value={v} />
              ))}
            </datalist>
            <datalist id="sermon-series">
              {seriesValues.map((v) => (
                <option key={v} value={v} />
              ))}
            </datalist>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Find readings</CardTitle>
            <CardDescription>
              {project.kind === "readings"
                ? "Searches every sermon for every verse of the books chosen, in each translation chosen: exact readings, close ones (a word changed, added, or skipped), and partial ones."
                : "Searches every sermon for every verse of the book: exact readings, close ones (a word changed, added, or skipped), and partial ones. Your chosen verses are kept when you search again."}
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            {project.kind === "readings" ? (
              <>
                <div className="flex items-start gap-4">
                  <div className="w-40 shrink-0 pt-0.5 text-sm">Translations</div>
                  <div className="flex-1">
                    <TranslationPicker
                      bibles={bibles}
                      value={project.translations?.length ? project.translations : [project.bibleId]}
                      onChange={(translations) => {
                        // The first is the one whose names are shown
                        update((p) => ({ ...p, translations, bibleId: translations.includes(p.bibleId) ? p.bibleId : translations[0] }), "change the translations");
                        setBookChanged(true);
                      }}
                    />
                  </div>
                </div>
                <div className="flex items-start gap-4">
                  <div className="w-40 shrink-0 pt-0.5 text-sm">Books</div>
                  <div className="flex-1">
                    <ScopeEditor
                      scope={project.scope ?? {}}
                      books={outline}
                      onChange={(scope) => {
                        update((p) => ({ ...p, scope }), "change the books searched");
                        setBookChanged(true);
                      }}
                    />
                  </div>
                </div>
                {bookChanged && <p className="text-sm text-amber-600">Search again to find readings with these.</p>}
              </>
            ) : (
            <div className="flex items-center gap-4">
              <div className="w-40 shrink-0 text-sm">Book</div>
              <Select
                value={project.book === null ? "all" : String(project.book)}
                onValueChange={(v) => {
                  const book = v === "all" ? null : Number(v);
                  update((p) => ({ ...p, book }), "change the book");
                  setBookChanged(true);
                  if (book !== null) go(book, 1);
                  setTab("sources");
                }}
              >
                <SelectTrigger className="w-64">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {outline.map((b) => (
                    <SelectItem key={b.id} value={String(b.id)}>
                      {b.name}
                    </SelectItem>
                  ))}
                  <SelectItem value="all">The whole Bible</SelectItem>
                </SelectContent>
              </Select>
              {bookChanged && <span className="text-sm text-amber-600">Search again to find readings in this book.</span>}
            </div>
            )}
            <div className="flex items-center gap-4">
              <div className="w-40 shrink-0 text-sm">
                Minimum accuracy <span className="font-medium tabular-nums">{Math.round(project.settings.minAccuracy * 100)}%</span>
              </div>
              <Slider
                className="max-w-sm"
                min={0.5}
                max={1}
                step={0.05}
                value={[project.settings.minAccuracy]}
                onValueChange={([v]) => update((p) => ({ ...p, settings: { ...p.settings, minAccuracy: v } }), "change the minimum accuracy", "minAccuracy")}
              />
              <span className="text-xs text-muted-foreground">Lower finds readings with more words changed, skipped, or misheard.</span>
            </div>
            <div className="flex items-center gap-3">
              <Button disabled={project.sources.length === 0 || progress !== null} onClick={analyze}>
                {progress ? <Loader2 className="animate-spin" /> : <ScanSearch />}
                {summary.analyzedAt ? "Search again" : "Search sermons"}
              </Button>
              {stale && !progress && summary.analyzedAt > 0 && (
                <span className="text-sm text-amber-600">Some sermons haven't been searched yet.</span>
              )}
              {summary.analyzedAt > 0 && !progress && (
                <span className="text-sm text-muted-foreground">
                  Last searched {new Date(summary.analyzedAt).toLocaleString()} ·{" "}
                  {summary.readings.toLocaleString()} readings, {summary.runs.toLocaleString()} passages
                  read straight through
                </span>
              )}
            </div>
            {progress && (
              <div className="space-y-1.5">
                <Progress value={progress.total ? (progress.done / progress.total) * 100 : 0} />
                <div className="text-xs text-muted-foreground">
                  {progress.message} ({progress.done}/{progress.total})
                </div>
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
