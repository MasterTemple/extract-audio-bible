import type { BookOutline, FileMetadata, Match, MatchKind, Part, Passage, Project, Run, Selection, Source } from "./api";

export const verseKey = (book: number, chapter: number, verse: number) => `${book}.${chapter}.${verse}`;

export function parseKey(key: string): [number, number, number] {
  const [b, c, v] = key.split(".").map(Number);
  return [b, c, v];
}

/** Whether a reading was added by hand (from what was said) */
export const isAdded = (m: { id: string }) => m.id.startsWith("added:");

/** Whether a reading is still the project's: one added by hand is, until it's taken out (or its adding undone) */
export const readingKept = (project: Project, m: Match) => !isAdded(m) || (project.addedReadings ?? []).some((a) => a.id === m.id);

/** Where a part is cut, with its trims, as the library cuts it */
export function clipOf(part: Part): [number, number] {
  const start = Math.max(0, part.start + part.trimStart);
  return [start, Math.max(start + 0.05, part.end + part.trimEnd)];
}

/**
 * What's played of a part, as the library plays it: its clip without its cuts, in order (cuts
 * outside it ignored, overlapping ones as one, pieces under 20 ms left out; all of it if nothing's left)
 */
export function spansOf(part: Part): [number, number][] {
  const [start, end] = clipOf(part);
  const cuts = (part.cuts ?? [])
    .map((c): [number, number] => [Math.max(start, Math.min(c.start, c.end)), Math.min(end, Math.max(c.start, c.end))])
    .filter(([a, b]) => b > a)
    .sort((x, y) => x[0] - y[0]);
  const spans: [number, number][] = [];
  let at = start;
  for (const [a, b] of cuts) {
    if (a > at) spans.push([at, a]);
    at = Math.max(at, b);
  }
  spans.push([at, end]);
  const heard = spans.filter(([a, b]) => b - a >= 0.02);
  return heard.length > 0 ? heard : [[start, end]];
}

/** The stretches between spans (what's left out), as the player skips them */
export const gapsOf = (spans: [number, number][]): [number, number][] => spans.slice(1).map(([from], i) => [spans[i][1], from]);

/** How long a part plays, in seconds: its clip without its cuts */
export const playedSeconds = (part: Part) => spansOf(part).reduce((sum, [a, b]) => sum + b - a, 0);

const KIND_RANK: Record<MatchKind, number> = { exact: 2, close: 1, partial: 0 };

/**
 * How good a verse's chosen reading is: its kind and score, or for readings joined, the worst of
 * them (with the lowest score); null until its readings are read
 */
export function chosenQuality(sel: Selection, matchById: Map<string, Match>): { kind: MatchKind; score: number; joined: number } | null {
  const parts = [sel.matchId, ...(sel.joined ?? []).map((p) => p.matchId)].map((id) => matchById.get(id));
  if (parts.some((m) => !m)) return null;
  const found = parts as Match[];
  const kind = found.reduce((worst, m) => (KIND_RANK[m.kind] < KIND_RANK[worst] ? m.kind : worst), found[0].kind);
  return { kind, score: Math.min(...found.map((m) => m.score)), joined: found.length };
}

/** Whether one kind of reading is better than another */
export const betterKind = (a: MatchKind, b: MatchKind) => KIND_RANK[a] > KIND_RANK[b];

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

/** The digits of a short id, as the short-uuid npm package (and Let's Church) write them: Flickr's base 58 */
const BASE58 = "123456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ";
const SHORT_ID_LENGTH = 22;

/** A UUID's 128 bits, with or without its dashes */
function uuidBits(id: string): bigint | null {
  return /^([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}|[0-9a-f]{32})$/i.test(id) ? BigInt(`0x${id.replace(/-/g, "")}`) : null;
}

/** A short id's 128 bits */
function shortIdBits(id: string): bigint | null {
  if (id.length !== SHORT_ID_LENGTH) return null;
  let n = 0n;
  for (const c of id) {
    const digit = BASE58.indexOf(c);
    if (digit < 0) return null;
    n = n * 58n + BigInt(digit);
  }
  return n < 1n << 128n ? n : null;
}

/** An id as a UUID (lowercase), as the library makes it: itself if it is one, or a short id's; null if it's neither */
export function uuidOf(id: string): string | null {
  const bits = uuidBits(id) ?? shortIdBits(id);
  if (bits === null) return null;
  const hex = bits.toString(16).padStart(32, "0");
  return [hex.slice(0, 8), hex.slice(8, 12), hex.slice(12, 16), hex.slice(16, 20), hex.slice(20)].join("-");
}

/** An id as a short id (a UUID in 22 base-58 digits, as short-uuid writes it): itself if it is one, or a UUID's; null if it's neither */
export function shortIdOf(id: string): string | null {
  let bits = shortIdBits(id) ?? uuidBits(id);
  if (bits === null) return null;
  let out = "";
  for (let i = 0; i < SHORT_ID_LENGTH; i++, bits /= 58n) out = BASE58[Number(bits % 58n)] + out;
  return out;
}

/**
Where a sermon's audio is: its own file, or the project's audio URL filled in with its ids (downloaded when it's needed):
`{id}` its file id, `{uuid}` and `{shortId}` that as a UUID or a short id, `{downloadId}` the id given to download it by
(each the file id when it can't be had)
*/
export function audioPathOf(project: Project, source: Source): string {
  if (source.audioPath) return source.audioPath;
  const template = project.settings.audioUrl?.trim() ?? "";
  if (!template) return "";
  const id = fileIdOf(source);
  const ids: Record<string, string> = {
    id,
    uuid: uuidOf(id) ?? id,
    shortId: shortIdOf(id) ?? id,
    downloadId: project.files?.[id]?.downloadId?.trim() || id,
  };
  return template.replace(/\{(id|uuid|shortId|downloadId)\}/g, (_, key: string) => urlEncode(ids[key]));
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
export function versesNeeded(outline: BookOutline, chapter: number, project?: Project | null): number[] {
  const count = outline.chapters[chapter - 1] ?? 0;
  const empty = new Set(outline.empty[chapter - 1] ?? []);
  return Array.from({ length: count }, (_, i) => i + 1).filter((v) => !empty.has(v) && inPassage(project, outline.id, chapter, v));
}

/** Whether a verse is in the project's passage (any verse is, when it has none, or for another book) */
export function inPassage(project: Project | null | undefined, book: number, chapter: number, verse: number): boolean {
  const p = project?.passage;
  if (!p || project?.book !== book) return true;
  const at = chapter * 1000 + verse;
  return at >= p.startChapter * 1000 + p.startVerse && at <= p.endChapter * 1000 + p.endVerse;
}

/** Whether the project's passage has any of a chapter's verses (any chapter does, when it has none) */
export function passageHasChapter(project: Project | null | undefined, book: number, chapter: number): boolean {
  const p = project?.passage;
  return !p || project?.book !== book || (chapter >= p.startChapter && chapter <= p.endChapter);
}

/** A passage's verses: "2:14-17", "7:24-8:2", or "3:16" */
export function passageLabel(p: Passage): string {
  if (p.startChapter !== p.endChapter) return `${p.startChapter}:${p.startVerse}-${p.endChapter}:${p.endVerse}`;
  return p.startVerse !== p.endVerse ? `${p.startChapter}:${p.startVerse}-${p.endVerse}` : `${p.startChapter}:${p.startVerse}`;
}

/** What a project is of: "2 Corinthians 2:14-17", "1 Thessalonians", or null for the whole Bible */
export function projectReference(project: Project, outline: BookOutline[]): string | null {
  if (project.book === null) return null;
  const book = outline.find((b) => b.id === project.book)?.name ?? "Book";
  return project.passage ? `${book} ${passageLabel(project.passage)}` : book;
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
  for (const v of versesNeeded(outline, chapter, project)) {
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
