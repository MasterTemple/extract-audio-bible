import type { BookOutline, FileMetadata, Project, Run, Source } from "./api";

export const verseKey = (book: number, chapter: number, verse: number) => `${book}.${chapter}.${verse}`;

export function parseKey(key: string): [number, number, number] {
  const [b, c, v] = key.split(".").map(Number);
  return [b, c, v];
}

export function formatTime(seconds: number, precise = false): string {
  if (!Number.isFinite(seconds)) return "–";
  const sign = seconds < 0 ? "-" : "";
  const s = Math.abs(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const secText = precise ? sec.toFixed(2).padStart(5, "0") : Math.floor(sec).toString().padStart(2, "0");
  return h > 0 ? `${sign}${h}:${m.toString().padStart(2, "0")}:${secText}` : `${sign}${m}:${secText}`;
}

export function bookName(outline: BookOutline[], book: number): string {
  return outline.find((b) => b.id === book)?.name ?? `Book ${book}`;
}

export function runLabel(outline: BookOutline[], run: Run): string {
  const name = bookName(outline, run.book);
  if (run.startChapter === run.endChapter) return `${name} ${run.startChapter}:${run.startVerse}–${run.endVerse}`;
  return `${name} ${run.startChapter}:${run.startVerse}–${run.endChapter}:${run.endVerse}`;
}

export function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

const SERMON_FILES = new Set(["json", "mp3", "m4a", "wav", "flac", "ogg", "aac", "mp4"]);

/** A sermon's id for metadata: the one its transcript gives, or its file's name without its extensions */
export function fileIdOf(source: Source): string {
  if (source.fileId) return source.fileId;
  let name = fileName(source.transcriptPath);
  if (name.toLowerCase().endsWith(".json.gz")) name = name.slice(0, -".gz".length);
  for (let dot = name.lastIndexOf("."); dot > 0 && SERMON_FILES.has(name.slice(dot + 1).toLowerCase()); dot = name.lastIndexOf("."))
    name = name.slice(0, dot);
  return name;
}

/** Text made safe in a URL, as the library does: letters, digits, and -._~ kept, the rest percent-encoded */
function urlEncode(text: string): string {
  return Array.from(new TextEncoder().encode(text), (b) =>
    /[A-Za-z0-9\-._~]/.test(String.fromCharCode(b)) ? String.fromCharCode(b) : `%${b.toString(16).toUpperCase().padStart(2, "0")}`,
  ).join("");
}

/** Where a sermon's audio is: its own file, or the project's audio URL filled in with its file id (downloaded when it's needed) */
export function audioPathOf(project: Project, source: Source): string {
  if (source.audioPath) return source.audioPath;
  const template = project.settings.audioUrl?.trim() ?? "";
  return template ? template.split("{id}").join(urlEncode(fileIdOf(source))) : "";
}

/** What's known about a sermon */
export function fileMetadataOf(project: Project, source: Source): FileMetadata {
  return project.files?.[fileIdOf(source)] ?? {};
}

/** The speakers (or series) the project's sermons have, in order, each once */
export function valuesOf(project: Project, field: "speaker" | "series"): string[] {
  const values = project.sources.map((s) => fileMetadataOf(project, s)[field]?.trim()).filter((v): v is string => !!v);
  return [...new Set(values)].sort((a, b) => a.localeCompare(b));
}

/** Whether a sermon is by one of `speakers` and in one of `series` (an empty list allows any) */
export function sermonMatches(project: Project, source: Source, speakers: string[], series: string[]): boolean {
  const meta = fileMetadataOf(project, source);
  const among = (value: string | null | undefined, wanted: string[]) => wanted.length === 0 || (!!value && wanted.includes(value.trim()));
  return among(meta.speaker, speakers) && among(meta.series, series);
}

/** A sermon's title, or its file's name when it has none */
export function sourceTitle(project: Project, source: Source | undefined): string {
  if (!source) return "Missing sermon";
  return fileMetadataOf(project, source).title || source.label;
}

/** Verses of a chapter that need audio (bridged verses have no text of their own) */
export function versesNeeded(outline: BookOutline, chapter: number): number[] {
  const count = outline.chapters[chapter - 1] ?? 0;
  const empty = new Set(outline.empty[chapter - 1] ?? []);
  return Array.from({ length: count }, (_, i) => i + 1).filter((v) => !empty.has(v));
}

export interface ChapterStatus {
  total: number;
  selected: number;
  withMatches: number;
  needsTrim: number;
  matches: number;
}

export function chapterStatus(
  project: Project,
  outline: BookOutline,
  chapter: number,
  /** Each verse's count of readings, by verse key */
  verseCounts: Map<string, number>,
): ChapterStatus {
  const status: ChapterStatus = { total: 0, selected: 0, withMatches: 0, needsTrim: 0, matches: 0 };
  for (const v of versesNeeded(outline, chapter)) {
    const key = verseKey(outline.id, chapter, v);
    status.total++;
    const sel = project.selections[key];
    if (sel) status.selected++;
    if (sel?.needsTrim) status.needsTrim++;
    const n = verseCounts.get(key) ?? 0;
    if (n > 0) status.withMatches++;
    status.matches += n;
  }
  return status;
}

export type Tone = "done" | "partial" | "available" | "none" | "trim";

export function chapterTone(s: ChapterStatus): Tone {
  if (s.total > 0 && s.selected === s.total) return s.needsTrim > 0 ? "trim" : "done";
  if (s.selected > 0) return "partial";
  if (s.withMatches > 0) return "available";
  return "none";
}

/** A color per sermon, the same wherever it's shown */
const PALETTE = [
  "#2563eb",
  "#db2777",
  "#16a34a",
  "#ea580c",
  "#7c3aed",
  "#0891b2",
  "#ca8a04",
  "#dc2626",
  "#4f46e5",
  "#059669",
];

/** Each sermon list's sermons by id, with where each is: made once for each list (they're replaced, not changed) */
const sermonIndexes = new WeakMap<Source[], Map<string, [number, Source]>>();

function sermonIndex(project: Project): Map<string, [number, Source]> {
  let index = sermonIndexes.get(project.sources);
  if (!index) {
    index = new Map(project.sources.map((s, i) => [s.id, [i, s]]));
    sermonIndexes.set(project.sources, index);
  }
  return index;
}

/** A sermon of the project, by its id */
export function sourceOf(project: Project, sourceId: string): Source | undefined {
  return sermonIndex(project).get(sourceId)?.[1];
}

export function sourceColor(project: Project, sourceId: string): string {
  const i = sermonIndex(project).get(sourceId)?.[0] ?? 0;
  return PALETTE[i % PALETTE.length];
}

export const kindLabel = { exact: "Exact", close: "Close", partial: "Partial" } as const;
