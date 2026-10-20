import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { AudioLines, FileJson, FilePlus, FolderPlus, Loader2, ScanSearch, Tags, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { api, errorMessage, type Progress as ProgressEvent } from "@/lib/api";
import { useApp } from "@/lib/store";
import { fileIdOf, fileMetadataOf, fileName, formatTime, sourceColor } from "@/lib/refs";

export function SourcesPanel() {
  const { project, update, setProject, setMatches, matchSet, flush, setTab, outline, go, setFileMetadata } = useApp();
  const [bookChanged, setBookChanged] = useState(false);
  const [unpaired, setUnpaired] = useState<string[]>([]);
  const [progress, setProgress] = useState<ProgressEvent | null>(null);

  useEffect(() => {
    const off = listen<ProgressEvent>("analyze-progress", (e) => setProgress(e.payload));
    return () => void off.then((f) => f());
  }, []);

  if (!project) return null;

  const add = async (directory: boolean) => {
    const picked = await open({
      multiple: true,
      directory,
      filters: directory
        ? undefined
        : [{ name: "Sermon audio and WhisperX JSON", extensions: ["mp3", "m4a", "wav", "flac", "ogg", "json"] }],
    });
    if (!picked) return;
    await flush();
    try {
      const result = await api.addSources(project.id, Array.isArray(picked) ? picked : [picked]);
      setProject(result.project);
      setUnpaired(result.unpaired);
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

  const analyze = async () => {
    await flush();
    setProgress({ done: 0, total: project.sources.length, message: "Starting" });
    try {
      const result = await api.analyze(useApp.getState().project!);
      setProject(result.project);
      setMatches(result.matches);
      for (const e of result.errors) toast.error(e);
      setBookChanged(false);
      toast.success(`Found ${result.matches.matches.length.toLocaleString()} readings`, {
        action: { label: "View verses", onClick: () => setTab("verses") },
      });
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setProgress(null);
    }
  };

  const remove = (id: string) => {
    const used = Object.values(project.selections).filter((s) => s.sourceId === id).length;
    if (used > 0 && !confirm(`${used} chosen verses use this sermon and will be unchosen. Remove it?`)) return;
    update((p) => ({
      ...p,
      sources: p.sources.filter((s) => s.id !== id),
      selections: Object.fromEntries(Object.entries(p.selections).filter(([, s]) => s.sourceId !== id)),
    }));
    setMatches({
      ...matchSet,
      matches: matchSet.matches.filter((m) => m.sourceId !== id),
      runs: matchSet.runs.filter((r) => r.sourceId !== id),
    });
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
                Each sermon is its audio and the WhisperX JSON transcribed from it, paired by file name (
                <code className="text-xs">sermon.mp3</code> + <code className="text-xs">sermon.json</code>). Add
                files, or a folder to pair everything in it. Titles and passages are kept by each sermon's id (its
                transcript's, once searched), and can be read from JSON:{" "}
                <code className="text-xs">{'{"<id>": {"title": …, "passage": …}}'}</code>.
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
            {unpaired.length > 0 && (
              <Alert>
                <AlertTitle>{unpaired.length} files had no partner</AlertTitle>
                <AlertDescription>
                  <div className="max-h-24 overflow-auto text-xs">
                    {unpaired.map((u) => (
                      <div key={u} className="truncate" title={u}>
                        {fileName(u)}
                      </div>
                    ))}
                  </div>
                </AlertDescription>
              </Alert>
            )}
            {project.sources.length === 0 ? (
              <div className="rounded-lg border border-dashed p-10 text-center text-sm text-muted-foreground">
                No sermons yet.
              </div>
            ) : (
              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Sermon</TableHead>
                    <TableHead>Passage</TableHead>
                    <TableHead>Files</TableHead>
                    <TableHead className="text-right">Length</TableHead>
                    <TableHead className="text-right">Words</TableHead>
                    <TableHead className="text-right">Readings</TableHead>
                    <TableHead className="text-right">Used</TableHead>
                    <TableHead />
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {project.sources.map((s) => {
                    const used = Object.values(project.selections).filter((x) => x.sourceId === s.id).length;
                    const fileId = fileIdOf(s);
                    const meta = fileMetadataOf(project, s);
                    return (
                      <TableRow key={s.id}>
                        <TableCell>
                          <div className="flex items-center gap-2 font-medium">
                            <span className="size-2.5 shrink-0 rounded-full" style={{ background: sourceColor(project, s.id) }} />
                            <input
                              className="w-full min-w-40 bg-transparent outline-none placeholder:text-foreground/70 focus:underline"
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
                            className="w-full min-w-32 bg-transparent text-sm outline-none placeholder:text-muted-foreground/60 focus:underline"
                            value={meta.passage ?? ""}
                            placeholder="Add passage"
                            onChange={(e) => setFileMetadata({ [fileId]: { passage: e.target.value } })}
                          />
                        </TableCell>
                        <TableCell className="max-w-72 text-xs text-muted-foreground">
                          <div className="flex items-center gap-1 truncate" title={s.audioPath}>
                            <AudioLines className="size-3 shrink-0" /> {fileName(s.audioPath)}
                          </div>
                          <div className="flex items-center gap-1 truncate" title={s.transcriptPath}>
                            <FileJson className="size-3 shrink-0" /> {fileName(s.transcriptPath)}
                          </div>
                        </TableCell>
                        <TableCell className="text-right tabular-nums">{s.duration ? formatTime(s.duration) : "–"}</TableCell>
                        <TableCell className="text-right tabular-nums">{s.wordCount ? s.wordCount.toLocaleString() : "–"}</TableCell>
                        <TableCell className="text-right tabular-nums">{s.wordCount ? s.matchCount : "–"}</TableCell>
                        <TableCell className="text-right tabular-nums">{used}</TableCell>
                        <TableCell className="text-right">
                          <Button variant="ghost" size="icon-sm" onClick={() => remove(s.id)}>
                            <Trash2 />
                          </Button>
                        </TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Find readings</CardTitle>
            <CardDescription>
              Searches every sermon for every verse of the book: exact readings, close ones (a word changed, added, or skipped), and
              partial ones. Your chosen verses are kept when you search again.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center gap-4">
              <div className="w-40 shrink-0 text-sm">Book</div>
              <Select
                value={project.book === null ? "all" : String(project.book)}
                onValueChange={(v) => {
                  const book = v === "all" ? null : Number(v);
                  update((p) => ({ ...p, book }));
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
                onValueChange={([v]) => update((p) => ({ ...p, settings: { ...p.settings, minAccuracy: v } }))}
              />
              <span className="text-xs text-muted-foreground">Lower finds readings with more words changed, skipped, or misheard.</span>
            </div>
            <div className="flex items-center gap-3">
              <Button disabled={project.sources.length === 0 || progress !== null} onClick={analyze}>
                {progress ? <Loader2 className="animate-spin" /> : <ScanSearch />}
                {matchSet.analyzedAt ? "Search again" : "Search sermons"}
              </Button>
              {stale && !progress && matchSet.analyzedAt > 0 && (
                <span className="text-sm text-amber-600">Some sermons haven't been searched yet.</span>
              )}
              {matchSet.analyzedAt > 0 && !progress && (
                <span className="text-sm text-muted-foreground">
                  Last searched {new Date(matchSet.analyzedAt).toLocaleString()} ·{" "}
                  {matchSet.matches.length.toLocaleString()} readings, {matchSet.runs.length.toLocaleString()} passages
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
