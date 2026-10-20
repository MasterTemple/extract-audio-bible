import type { BookOutline, FileMetadata, Match, Project, Run, Source } from "./api";

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
  for (let dot = name.lastIndexOf("."); dot > 0 && SERMON_FILES.has(name.slice(dot + 1).toLowerCase()); dot = name.lastIndexOf("."))
    name = name.slice(0, dot);
  return name;
}

/** What's known about a sermon */
export function fileMetadataOf(project: Project, source: Source): FileMetadata {
  return project.files?.[fileIdOf(source)] ?? {};
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
  matchesByVerse: Map<string, Match[]>,
): ChapterStatus {
  const status: ChapterStatus = { total: 0, selected: 0, withMatches: 0, needsTrim: 0, matches: 0 };
  for (const v of versesNeeded(outline, chapter)) {
    const key = verseKey(outline.id, chapter, v);
    status.total++;
    const sel = project.selections[key];
    if (sel) status.selected++;
    if (sel?.needsTrim) status.needsTrim++;
    const n = matchesByVerse.get(key)?.length ?? 0;
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

export function sourceColor(project: Project, sourceId: string): string {
  const i = project.sources.findIndex((s) => s.id === sourceId);
  return PALETTE[(i < 0 ? 0 : i) % PALETTE.length];
}

export const kindLabel = { exact: "Exact", close: "Close", partial: "Partial" } as const;
