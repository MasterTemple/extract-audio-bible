import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { Download, FolderOpen, ImageIcon, Loader2, X } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { api, errorMessage, type BookOutline, type ExportedFile, type Metadata, type Progress as ProgressEvent, type Settings } from "@/lib/api";
import { useApp } from "@/lib/store";
import { cn } from "@/lib/utils";
import { bookName, fileName, formatTime, projectReference, verseKey, versesNeeded } from "@/lib/refs";

const PLACEHOLDERS =
  "{passage} {book} {osis} {bookNumber} {bookNumber2} {chapter} {chapter2} {chapter3} {chapters} {verse} {verse2} {verse3} {verses} {translation} {abbreviation}";

/** Where a preview is filled in from: a chapter of the project, and a verse of it */
interface Sample {
  book: BookOutline;
  chapter: number;
  verse: number;
  translation: string;
  abbreviation: string;
  /** The project's passage ("2 Corinthians 2:14-17"), if it has one */
  passage: string | null;
}

/** A template filled in as the export fills it (no verse for a chapter's file) */
function fill(template: string, s: Sample, verse: number | null) {
  const pad = (n: number, width: number) => String(n).padStart(width, "0");
  const values: Record<string, string> = {
    book: s.book.name,
    osis: s.book.osis,
    bookNumber: String(s.book.id),
    bookNumber2: pad(s.book.id, 2),
    chapter: String(s.chapter),
    chapter2: pad(s.chapter, 2),
    chapter3: pad(s.chapter, 3),
    chapters: String(s.book.chapters.length),
    verse: verse === null ? "" : String(verse),
    verse2: verse === null ? "" : pad(verse, 2),
    verse3: verse === null ? "" : pad(verse, 3),
    verses: String(s.book.chapters[s.chapter - 1] ?? 0),
    translation: s.translation,
    abbreviation: s.abbreviation,
    passage: s.passage ?? `${s.book.name} ${s.chapter}`,
  };
  return template.replace(/\{(\w+)\}/g, (all, key: string) => values[key] ?? all);
}

/** A name made safe for a file, as the export makes it */
function safeName(name: string) {
  return name
    .replace(/[/\\:*?"<>|\p{Cc}]/gu, "_")
    .trim()
    .replace(/^\.+|\.+$/g, "");
}

/** What a field will be: its value, in the file shown */
function Preview({ children }: { children: React.ReactNode }) {
  return (
    <p className="truncate rounded bg-muted/60 px-2 py-1 text-[11px] text-muted-foreground">
      → <span className="font-medium text-foreground">{children}</span>
    </p>
  );
}

const pad2 = (n: number) => String(n).padStart(2, "0");

function Field({ label, children, hint }: { label: string; children: React.ReactNode; hint?: string }) {
  return (
    <div className="space-y-1.5">
      <Label className="text-xs">{label}</Label>
      {children}
      {hint && <p className="text-[11px] text-muted-foreground">{hint}</p>}
    </div>
  );
}

export function ExportPanel() {
  const { project, outline, update, bibles, flush, go } = useApp();
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  const [results, setResults] = useState<ExportedFile[]>([]);
  // Books left out of the export (of a project of more than one), and whether to make every file again
  const [leftOut, setLeftOut] = useState<Set<number>>(new Set());
  const [again, setAgain] = useState(false);

  useEffect(() => {
    const off = listen<ProgressEvent>("export-progress", (e) => setProgress(e.payload));
    return () => void off.then((f) => f());
  }, []);

  const chapters = useMemo(() => {
    if (!project) return [];
    const out: { book: number; chapter: number; chosen: number; total: number }[] = [];
    for (const b of outline.filter((x) => project.book === null || x.id === project.book)) {
      for (let c = 1; c <= b.chapters.length; c++) {
        const needed = versesNeeded(b, c, project);
        const chosen = needed.filter((v) => project.selections[verseKey(b.id, c, v)]).length;
        if (chosen === 0) continue;
        if (project.metadata.onlyComplete && chosen < needed.length) continue;
        out.push({ book: b.id, chapter: c, chosen, total: needed.length });
      }
    }
    return out;
  }, [project, outline]);
  // The books with chapters to export, and those chapters of the books chosen
  const books = useMemo(() => [...new Set(chapters.map((c) => c.book))], [chapters]);
  const included = chapters.filter((c) => !leftOut.has(c.book));

  if (!project) return null;
  const meta = project.metadata;
  const settings = project.settings;
  const bible = bibles.find((b) => b.id === project.bibleId);
  const setMeta = (change: Partial<Metadata>) =>
    update((p) => ({ ...p, metadata: { ...p.metadata, ...change } }), "change the export's tags or files", `metadata:${Object.keys(change).join()}`);
  const setSettings = (change: Partial<Settings>) =>
    update((p) => ({ ...p, settings: { ...p.settings, ...change } }), "change the export's sound", `settings:${Object.keys(change).join()}`);
  // Previews are of the first chapter to be exported (or the book's first), and its first chosen verse
  const first = chapters[0] ?? (project.book !== null ? { book: project.book, chapter: 1 } : null);
  const sampleBook = outline.find((b) => b.id === (first?.book ?? outline[0]?.id));
  const sampleChapter = first?.chapter ?? 1;
  const sample: Sample | null = sampleBook
    ? {
        book: sampleBook,
        chapter: sampleChapter,
        verse:
          versesNeeded(sampleBook, sampleChapter, project).find((v) => project.selections[verseKey(sampleBook.id, sampleChapter, v)]) ?? 1,
        translation: bible?.name ?? "",
        abbreviation: bible?.abbreviation ?? "",
        passage: project.passage ? projectReference(project, outline) : null,
      }
    : null;
  /** How a field previews: a tag of a chapter's file (or a verse's), or a file's path */
  type Shows = "tag" | "verseTag" | "file" | "verseFile" | "none";
  const preview = (key: keyof Metadata, shows: Shows) => {
    if (!sample || shows === "none") return null;
    const value = (meta[key] as string) ?? "";
    if (!value.trim()) return <Preview>{shows === "tag" || shows === "verseTag" ? "Not written" : "(no name)"}</Preview>;
    if (key === "year" && !/^\s*-?\d+\s*$/.test(value)) return <Preview>Not written: a year is a number</Preview>;
    const filled = fill(value, sample, shows === "verseTag" || shows === "verseFile" ? sample.verse : null);
    if (shows === "tag" || shows === "verseTag") return <Preview>{filled}</Preview>;
    const bookDir = meta.folderPerBook ? `${safeName(`${pad2(sample.book.id)} ${sample.book.name}`)}/` : "";
    const dir = shows === "verseFile" ? `${bookDir}${safeName(`${sample.book.name} ${sample.chapter}`)}/` : bookDir;
    return <Preview>{`${dir}${safeName(filled)}.mp3`}</Preview>;
  };
  const text = (key: keyof Metadata, label: string, placeholder = "", hint?: string, shows: Shows = "tag") => (
    <Field label={label} hint={hint}>
      <Input value={(meta[key] as string) ?? ""} placeholder={placeholder} onChange={(e) => setMeta({ [key]: e.target.value })} />
      {preview(key, shows)}
    </Field>
  );
  const seconds = (key: keyof Settings, label: string, hint?: string) => (
    <Field label={label} hint={hint}>
      <div className="flex items-center gap-1.5">
        <Input
          type="number"
          min={0}
          step={0.1}
          className="w-24"
          value={settings[key] as number}
          onChange={(e) => setSettings({ [key]: Math.max(0, Number(e.target.value) || 0) })}
        />
        <span className="text-xs text-muted-foreground">seconds</span>
      </div>
    </Field>
  );

  const pickDir = async () => {
    const dir = await open({ directory: true });
    if (typeof dir === "string") setMeta({ outputDir: dir });
  };
  const pickCover = async () => {
    const file = await open({ filters: [{ name: "Image", extensions: ["jpg", "jpeg", "png"] }] });
    if (typeof file === "string") setMeta({ coverPath: file });
  };

  const run = async () => {
    await flush();
    setResults([]);
    setProgress({ done: 0, total: included.length, current: "" });
    try {
      const only = leftOut.size > 0 ? included.map(({ book, chapter }) => ({ book, chapter })) : null;
      const files = await api.exportChapters(useApp.getState().project!.id, only, again);
      setResults(files);
      const made = files.filter((f) => !f.unchanged).length;
      const same = files.length - made;
      toast.success(
        `Made ${made} file${made === 1 ? "" : "s"}${same > 0 ? `; ${same} already there as ${same === 1 ? "it" : "they"} would be made, left as ${same === 1 ? "it was" : "they were"}` : ""}`,
      );
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setProgress(null);
    }
  };

  const chapterFiles = meta.exportMode !== "verse";
  const verseFiles = meta.exportMode !== "chapter";
  const verseCount = included.reduce((n, c) => n + c.chosen, 0);

  return (
    <div className="h-full overflow-auto">
      <div className="mx-auto grid max-w-6xl gap-4 p-6 lg:grid-cols-[1fr_22rem]">
        <div className="space-y-4">
          <Card>
            <CardHeader>
              <CardTitle>Tags</CardTitle>
              <CardDescription>
                Written into every MP3 (ID3v2.4). Any field can use <code className="text-[11px]">{PLACEHOLDERS}</code>
                {sample && (
                  <>
                    {" "}
                    Previews show {sample.book.name} {sample.chapter}
                    {verseFiles && `, and verse ${sample.verse} for verse files`}.
                  </>
                )}
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-2">
              {text("titleTemplate", "Title", "{book} {chapter}")}
              {text("albumTemplate", "Album", "{translation} – {book}")}
              {text("artist", "Artist", "Grace Bible Church")}
              {text("albumArtist", "Album artist")}
              {text("narrator", "Narrator / composer", "", "Audiobook players often show this as the narrator")}
              {text("genre", "Genre", "Audiobook")}
              {text("year", "Year", String(new Date().getFullYear()))}
              {text("language", "Language", "eng")}
              {text("publisher", "Publisher")}
              {text("copyright", "Copyright", "Scripture quotations from …")}
              <div className="sm:col-span-2">
                <Field label="Comment">
                  <Textarea value={meta.comment} onChange={(e) => setMeta({ comment: e.target.value })} rows={2} />
                  {preview("comment", "tag")}
                </Field>
              </div>
              <div className="sm:col-span-2">
                <Field label="Cover art">
                  <div className="flex items-center gap-2">
                    <Button variant="outline" size="sm" onClick={pickCover}>
                      <ImageIcon /> Choose image
                    </Button>
                    {meta.coverPath ? (
                      <>
                        <span className="truncate text-xs text-muted-foreground" title={meta.coverPath}>
                          {fileName(meta.coverPath)}
                        </span>
                        <Button size="icon-xs" variant="ghost" onClick={() => setMeta({ coverPath: null })}>
                          <X />
                        </Button>
                      </>
                    ) : (
                      <span className="text-xs text-muted-foreground">None</span>
                    )}
                  </div>
                </Field>
              </div>
              <Field label="Track number">
                <Select value={meta.trackNumbering} onValueChange={(v) => setMeta({ trackNumbering: v as Metadata["trackNumbering"] })}>
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="chapter">Chapter number (of the book's chapters)</SelectItem>
                    <SelectItem value="sequence">Order in this export</SelectItem>
                  </SelectContent>
                </Select>
                {sample && (
                  <Preview>
                    {meta.trackNumbering === "sequence"
                      ? `Track 1 of ${Math.max(1, chapters.length)}`
                      : `Track ${sample.chapter} of ${sample.book.chapters.length}`}
                    , disc {sample.book.id}
                    {verseFiles && ` (verse files: track ${sample.verse} of ${sample.book.chapters[sample.chapter - 1]}, disc ${sample.chapter})`}
                  </Preview>
                )}
              </Field>
              <p className="self-end pb-2 text-[11px] text-muted-foreground">The disc number is the book's number (Genesis is 1).</p>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Spacing and sound</CardTitle>
              <CardDescription>Silence for a single verse is set where you choose its reading.</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-3">
              {seconds("gapBetweenVerses", "Gap between every verse", "Verses read straight through stay joined when this is 0")}
              {seconds("chapterLeadIn", "Silence at the start")}
              {seconds("chapterTail", "Silence at the end")}
              <div className="sm:col-span-2">
                <Field
                  label="Leveling"
                  hint={
                    settings.leveling === "each"
                      ? "Every reading is brought to the target, so sermons recorded louder or softer sound alike."
                      : settings.leveling === "export"
                        ? "One change for everything exported, so it reaches the target overall while readings keep their levels relative to each other (and the volume you set for each verse)."
                        : "Only the volume you set for each verse, and the overall volume, are applied."
                  }
                >
                  <div className="flex items-center gap-2">
                    <Select value={settings.leveling} onValueChange={(v) => setSettings({ leveling: v as Settings["leveling"] })}>
                      <SelectTrigger className="w-64">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value="each">Level each reading</SelectItem>
                        <SelectItem value="export">Normalize the whole export</SelectItem>
                        <SelectItem value="none">Off (manual volume only)</SelectItem>
                      </SelectContent>
                    </Select>
                    <Input
                      type="number"
                      step={1}
                      className="w-20"
                      disabled={settings.leveling === "none"}
                      value={settings.targetDb}
                      onChange={(e) => setSettings({ targetDb: Math.min(-6, Number(e.target.value) || -20) })}
                    />
                    <span className="text-xs text-muted-foreground">dBFS target</span>
                  </div>
                </Field>
              </div>
              <Field label={`Overall volume: ${settings.outputGainDb > 0 ? "+" : ""}${settings.outputGainDb.toFixed(1)} dB`}>
                <Slider
                  className="mt-3"
                  min={-12}
                  max={12}
                  step={0.5}
                  value={[settings.outputGainDb]}
                  onValueChange={([v]) => setSettings({ outputGainDb: v })}
                />
              </Field>
              <Field label="Fade at cuts">
                <div className="flex items-center gap-1.5">
                  <Input
                    type="number"
                    min={0}
                    step={1}
                    className="w-24"
                    value={settings.fadeMs}
                    onChange={(e) => setSettings({ fadeMs: Math.max(0, Number(e.target.value) || 0) })}
                  />
                  <span className="text-xs text-muted-foreground">ms</span>
                </div>
              </Field>
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>Files</CardTitle>
              <CardDescription>Mono MP3s: one per chapter, one per verse, or both.</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4 sm:grid-cols-2">
              <div className="sm:col-span-2">
                <Field label="Folder">
                  <div className="flex items-center gap-2">
                    <Button variant="outline" size="sm" onClick={pickDir}>
                      <FolderOpen /> Choose folder
                    </Button>
                    <span className="truncate text-xs text-muted-foreground" title={meta.outputDir ?? ""}>
                      {meta.outputDir ?? "None chosen"}
                    </span>
                  </div>
                </Field>
              </div>
              <div className="sm:col-span-2">
                <Field
                  label="Files to write"
                  hint={
                    verseFiles
                      ? project.passage
                        ? "Verse files go in a folder for the passage, an album numbered in its order, each with only its own silence before and after."
                        : "Verse files go in a folder per chapter (John 3), each with only its own silence before and after."
                      : undefined
                  }
                >
                  <Select value={meta.exportMode} onValueChange={(v) => setMeta({ exportMode: v as Metadata["exportMode"] })}>
                    <SelectTrigger className="w-64">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value="chapter">{project.passage ? "One file for the passage" : "A file per chapter"}</SelectItem>
                      <SelectItem value="verse">A file per verse</SelectItem>
                      <SelectItem value="both">Both</SelectItem>
                    </SelectContent>
                  </Select>
                </Field>
              </div>
              {chapterFiles &&
                text("filenameTemplate", project.passage ? "Passage file name" : "Chapter file name", "{bookNumber2}_{osis}_{chapter3}", undefined, "file")}
              {verseFiles && (
                <>
                  {text(
                    "verseFilenameTemplate",
                    "Verse file name",
                    "{bookNumber2}_{osis}_{chapter3}_{verse3}",
                    undefined,
                    "verseFile",
                  )}
                  {text(
                    "verseTitleTemplate",
                    "Verse title",
                    "{book} {chapter}:{verse}",
                    "Verse files are numbered by verse, with the chapter as the disc",
                    "verseTag",
                  )}
                </>
              )}
              <div className="grid grid-cols-2 gap-3">
                <Field label="Bitrate">
                  <Select value={String(meta.bitrate)} onValueChange={(v) => setMeta({ bitrate: Number(v) })}>
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {[48, 64, 96, 128, 160, 192].map((b) => (
                        <SelectItem key={b} value={String(b)}>
                          {b} kbps
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
                <Field label="Sample rate">
                  <Select value={String(meta.sampleRate)} onValueChange={(v) => setMeta({ sampleRate: Number(v) })}>
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {[22050, 24000, 32000, 44100, 48000].map((r) => (
                        <SelectItem key={r} value={String(r)}>
                          {(r / 1000).toFixed(r % 1000 ? 2 : 0)} kHz
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </Field>
              </div>
              <div className="flex items-center gap-2">
                <Switch id="fpb" checked={meta.folderPerBook} onCheckedChange={(v) => setMeta({ folderPerBook: v })} />
                <Label htmlFor="fpb" className="text-sm font-normal">
                  A folder per book
                </Label>
              </div>
              <div className="flex items-center gap-2">
                <Switch id="oc" checked={meta.onlyComplete} onCheckedChange={(v) => setMeta({ onlyComplete: v })} />
                <Label htmlFor="oc" className="text-sm font-normal">
                  Only complete chapters
                </Label>
              </div>
            </CardContent>
          </Card>
        </div>

        <div className="space-y-4">
          <Card className="lg:sticky lg:top-0">
            <CardHeader>
              <CardTitle>Export</CardTitle>
              <CardDescription>
                {chapterFiles && `${included.length} chapter file${included.length === 1 ? "" : "s"}`}
                {chapterFiles && verseFiles && " and "}
                {verseFiles && `${verseCount} verse file${verseCount === 1 ? "" : "s"}`}
                {books.length > 1 && ` of ${books.length - leftOut.size} of ${books.length} books`}
                {!meta.onlyComplete && included.some((c) => c.chosen < c.total) && " (verses not chosen are left out)"}
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {books.length > 1 && (
                <div className="space-y-1">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium">Books</span>
                    <span className="flex gap-0.5">
                      <Button size="xs" variant="ghost" disabled={leftOut.size === 0} onClick={() => setLeftOut(new Set())}>
                        All
                      </Button>
                      <Button size="xs" variant="ghost" disabled={leftOut.size === books.length} onClick={() => setLeftOut(new Set(books))}>
                        None
                      </Button>
                    </span>
                  </div>
                  <div className="max-h-48 space-y-0.5 overflow-auto rounded-md border p-1">
                    {books.map((id) => {
                      const of = chapters.filter((c) => c.book === id);
                      const chosen = of.reduce((n, c) => n + c.chosen, 0);
                      const total = of.reduce((n, c) => n + c.total, 0);
                      return (
                        <label key={id} className="flex cursor-pointer items-center gap-2 rounded px-2 py-1 text-xs hover:bg-muted">
                          <Checkbox
                            checked={!leftOut.has(id)}
                            onCheckedChange={(on) =>
                              setLeftOut((s) => {
                                const next = new Set(s);
                                if (on) next.delete(id);
                                else next.add(id);
                                return next;
                              })
                            }
                          />
                          <span className="flex-1">{bookName(outline, id)}</span>
                          <span className="text-muted-foreground tabular-nums">
                            {of.length} ch · {chosen}/{total}
                          </span>
                        </label>
                      );
                    })}
                  </div>
                </div>
              )}
              <div className="max-h-80 space-y-0.5 overflow-auto rounded-md border p-1">
                {included.length === 0 && (
                  <p className="p-3 text-center text-xs text-muted-foreground">{chapters.length === 0 ? "Nothing chosen yet" : "No books chosen"}</p>
                )}
                {included.map((c) => (
                  <button
                    key={`${c.book}.${c.chapter}`}
                    onClick={() => go(c.book, c.chapter)}
                    className="flex w-full items-center justify-between rounded px-2 py-1 text-left text-xs hover:bg-muted"
                  >
                    <span>
                      {bookName(outline, c.book)} {c.chapter}
                    </span>
                    <span className={c.chosen === c.total ? "text-emerald-600" : "text-amber-600"}>
                      {c.chosen}/{c.total}
                    </span>
                  </button>
                ))}
              </div>
              <div className="flex items-start gap-2">
                <Switch id="again" checked={again} onCheckedChange={setAgain} />
                <Label htmlFor="again" className="flex-col items-start gap-0.5 text-sm font-normal">
                  Make every file again
                  <span className="text-[11px] text-muted-foreground">
                    {again
                      ? "Even files already in the folder as they'd be made."
                      : "Files already in the folder as they'd be made (the same readings, cuts, sound, and tags) are left as they are."}
                  </span>
                </Label>
              </div>
              <Button className="w-full" disabled={!meta.outputDir || included.length === 0 || progress !== null} onClick={run}>
                {progress ? <Loader2 className="animate-spin" /> : <Download />} Export MP3s
              </Button>
              {!meta.outputDir && <p className="text-xs text-muted-foreground">Choose a folder first.</p>}
              {progress && (
                <div className="space-y-1">
                  <Progress value={progress.total ? (progress.done / progress.total) * 100 : 0} />
                  <div className="text-xs text-muted-foreground">
                    {progress.current} ({progress.done}/{progress.total})
                  </div>
                </div>
              )}
              {results.length > 0 && (
                <div className="space-y-1">
                  <div className="flex items-center justify-between">
                    <span className="text-xs font-medium">
                      Written{" "}
                      {results.some((r) => r.unchanged) && (
                        <span className="font-normal text-muted-foreground">({results.filter((r) => r.unchanged).length} unchanged)</span>
                      )}
                    </span>
                    <Button size="xs" variant="ghost" onClick={() => revealItemInDir(results[0].path)}>
                      <FolderOpen /> Show
                    </Button>
                  </div>
                  <div className="max-h-60 overflow-auto rounded-md border p-1 text-xs">
                    {results.map((r) => (
                      <div key={r.path} className={cn("flex justify-between gap-2 px-1 py-0.5", r.unchanged && "text-muted-foreground")} title={r.path}>
                        <span className="truncate">
                          {fileName(r.path)}
                          {r.unchanged && " · unchanged"}
                        </span>
                        <span className="shrink-0 text-muted-foreground tabular-nums">
                          {formatTime(r.duration)}
                          {r.missing.length > 0 && <span className="text-amber-600"> · {r.missing.length} missing</span>}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
