import { invoke } from "@tauri-apps/api/core";

export interface BibleSummary {
  id: string;
  name: string;
  abbreviation: string;
  language: string | null;
  format: string;
  bookCount: number;
  verseCount: number;
}

export interface BookOutline {
  id: number;
  name: string;
  osis: string;
  /** Verses per chapter */
  chapters: number[];
  /** Per chapter, verses with no text of their own (bridged into the verse before) */
  empty: number[][];
}

export interface Source {
  id: string;
  label: string;
  audioPath: string;
  transcriptPath: string;
  duration: number | null;
  wordCount: number;
  matchCount: number;
  /** Its id for metadata: the one its transcript gives once searched, or its file's name (empty in older projects) */
  fileId?: string;
}

/** Where a phrase was said in a sermon */
export interface Said {
  sourceId: string;
  /** Its words, by index into the transcript (inclusive) */
  wordStart: number;
  wordEnd: number;
  start: number;
  end: number;
  before: string;
  text: string;
  after: string;
}

/** What's known about a sermon, by its file id */
export interface FileMetadata {
  title?: string | null;
  /** The passage it preaches ("1 Thessalonians 5:12-15") */
  passage?: string | null;
}

export interface Selection {
  matchId: string;
  sourceId: string;
  start: number;
  end: number;
  trimStart: number;
  trimEnd: number;
  padBefore: number;
  padAfter: number;
  /** Volume change for this verse, in dB */
  gainDb: number;
  needsTrim: boolean;
  note: string;
  /** More readings of the verse, played after this one in order (a verse read in pieces) */
  joined?: Part[];
}

/** One reading used for part of a verse, and how it's cut */
export interface Part {
  matchId: string;
  sourceId: string;
  start: number;
  end: number;
  trimStart: number;
  trimEnd: number;
}

export interface Settings {
  /** How faithfully a verse's words must be read for a reading to count (0–1) */
  minAccuracy: number;
  gapBetweenVerses: number;
  chapterLeadIn: number;
  chapterTail: number;
  /** none: only manual volume; each: every reading to the target; export: one change for the whole export */
  leveling: Leveling;
  targetDb: number;
  outputGainDb: number;
  fadeMs: number;
  /** Where to download a sermon's audio when it has no file, with `{id}` for its file id; empty for none */
  audioUrl: string;
}

export type Leveling = "none" | "each" | "export";
export type ExportMode = "chapter" | "verse" | "both";

export interface Metadata {
  titleTemplate: string;
  albumTemplate: string;
  filenameTemplate: string;
  verseTitleTemplate: string;
  verseFilenameTemplate: string;
  exportMode: ExportMode;
  artist: string;
  albumArtist: string;
  narrator: string;
  genre: string;
  year: string;
  comment: string;
  copyright: string;
  publisher: string;
  language: string;
  coverPath: string | null;
  bitrate: number;
  sampleRate: number;
  outputDir: string | null;
  onlyComplete: boolean;
  folderPerBook: boolean;
  trackNumbering: "chapter" | "sequence";
}

export type ProjectKind = "audioBible" | "readings";
export type Testament = "old" | "new";

/** Which books to search: included testaments limit it, included genres and books add up within them, exclusions always win */
export interface BookScope {
  testaments?: Testament[];
  genres?: string[];
  books?: string[];
  excludeTestaments?: Testament[];
  excludeGenres?: string[];
  excludeBooks?: string[];
}

/** A group of books (topos's genres: Pentateuch, Gospels…) */
export interface Genre {
  name: string;
  books: number[];
}

/** A verse read in a sermon, for a table or an export */
export interface ReadingRow {
  /** The reading's id, and its sermon's in the project */
  id: string;
  sourceId: string;
  /** The sermon's title, and its file id */
  sermon: string;
  sermonId: string;
  reference: string;
  book: number;
  chapter: number;
  verse: number;
  translation: string;
  alsoIn: string[];
  /** Other verses worded the same ("Psalm 14:1"), when the sermon doesn't say which */
  alsoAt: string[];
  start: number;
  end: number;
  kind: MatchKind;
  accuracy: number;
  coverage: number;
  said: string;
}

export interface Project {
  id: string;
  name: string;
  /** Building an audio Bible, or finding every verse read (missing in older projects: an audio Bible) */
  kind?: ProjectKind;
  bibleId: string;
  /** For finding every verse read: the Bibles searched, by id */
  translations?: string[];
  /** For finding every verse read: the books searched */
  scope?: BookScope;
  /** The book being built; null for the whole Bible */
  book: number | null;
  createdAt: number;
  updatedAt: number;
  sources: Source[];
  selections: Record<string, Selection>;
  settings: Settings;
  metadata: Metadata;
  /** What's known about each sermon, by its file id, kept apart from the sermons */
  files?: Record<string, FileMetadata>;
}

export interface ProjectSummary {
  id: string;
  name: string;
  kind: ProjectKind;
  /** The Bibles searched, by id */
  translations: string[];
  bibleId: string;
  book: number | null;
  updatedAt: number;
  sourceCount: number;
  selectionCount: number;
}

export type MatchKind = "exact" | "close" | "partial";

/** Where a verse is heard in a chapter's audio, in seconds */
export interface VerseTime {
  verse: number;
  start: number;
  end: number;
}

export interface Match {
  id: string;
  sourceId: string;
  book: number;
  chapter: number;
  verse: number;
  wordStart: number;
  wordEnd: number;
  start: number;
  end: number;
  score: number;
  /** How faithfully the verse's words were said, from the first read to the last (0 from searches before it was kept) */
  accuracy: number;
  coverage: number;
  precision: number;
  kind: MatchKind;
  verseWordsHeard: boolean[];
  extraWords: number[];
  runId: string | null;
  /** The translation it fits best, by its Bible's id (empty from searches before it was kept) */
  translation?: string;
  /** Other translations it was read from as faithfully, by their Bibles' ids */
  alsoIn?: string[];
  /** Other verses worded the same (`book.chapter.verse`), when the sermon doesn't say which */
  alsoAt?: string[];
}

export interface Run {
  id: string;
  sourceId: string;
  book: number;
  startChapter: number;
  startVerse: number;
  endChapter: number;
  endVerse: number;
  matchIds: string[];
  start: number;
  end: number;
}

export interface MatchSet {
  analyzedAt: number;
  matches: Match[];
  runs: Run[];
}

export interface Word {
  text: string;
  start: number;
  end: number;
  score: number | null;
  speaker: string | null;
  timed: boolean;
}

export interface ChapterRef {
  book: number;
  chapter: number;
}

export interface ExportedFile {
  book: number;
  chapter: number;
  verse: number | null;
  path: string;
  duration: number;
  missing: number[];
}

export interface Progress {
  done: number;
  total: number;
  message?: string;
  current?: string;
}

export const api = {
  listBibles: () => invoke<BibleSummary[]>("list_bibles"),
  importBible: (paths: string[], name: string, abbreviation: string, language: string | null) =>
    invoke<BibleSummary>("import_bible", { paths, name, abbreviation, language }),
  deleteBible: (id: string) => invoke<void>("delete_bible", { id }),
  bibleOutline: (id: string) => invoke<BookOutline[]>("bible_outline", { id }),
  bookText: (bibleId: string, book: number) => invoke<string[][]>("book_text", { bibleId, book }),

  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  createProject: (name: string, bibleId: string, book: number | null) =>
    invoke<Project>("create_project", { name, bibleId, book }),
  createReadingsProject: (name: string, bibleIds: string[], scope: BookScope) =>
    invoke<Project>("create_readings_project", { name, bibleIds, scope }),
  genres: () => invoke<Genre[]>("genres"),
  /** The books a scope searches, or null for every book; an error if it leaves nothing or names something unknown */
  scopeBooks: (scope: BookScope) => invoke<number[] | null>("scope_books", { scope }),
  /** Every reading found, as rows, and written to `path` (CSV, or JSON for a .json path) if given */
  readingRows: (project: Project, path: string | null) => invoke<ReadingRow[]>("reading_rows", { project, path }),
  getProject: (id: string) => invoke<Project>("get_project", { id }),
  saveProject: (project: Project) => invoke<number>("save_project", { project }),
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  addSources: (projectId: string, paths: string[]) =>
    invoke<{ project: Project; added: number; unpaired: string[] }>("add_sources", { projectId, paths }),
  analyze: (project: Project) =>
    invoke<{ project: Project; matches: MatchSet; errors: string[] }>("analyze", { project }),
  getMatches: (projectId: string) => invoke<MatchSet>("get_matches", { projectId }),
  transcriptWords: (transcriptPath: string, from: number, to: number) =>
    invoke<{ from: number; words: Word[] }>("transcript_words", { transcriptPath, from, to }),
  audioPeaks: (audioPath: string, start: number, end: number) =>
    invoke<{ start: number; perSecond: number; peaks: number[]; duration: number }>("audio_peaks", {
      audioPath,
      start,
      end,
    }),
  renderClip: (request: {
    audioPath: string;
    start: number;
    end: number;
    padBefore?: number;
    padAfter?: number;
    processed?: boolean;
    targetDb?: number | null;
    gainDb?: number;
  }) => invoke<ArrayBuffer>("render_clip", { request }),
  renderChapter: (project: Project, book: number, chapter: number) =>
    invoke<ArrayBuffer>("render_chapter", { project, book, chapter }),
  /** A verse as it will be exported: all its readings, one after another, with its silence */
  /** Titles and passages by file id, from a JSON file (`{"<file id>": {"title": …, "passage": …}}`) */
  readFileMetadata: (path: string) => invoke<Record<string, FileMetadata>>("read_file_metadata", { path }),
  /** A whole transcript, in paragraphs (where each begins, by word) */
  transcript: (transcriptPath: string) =>
    invoke<{ id: string | null; title: string | null; words: Word[]; paragraphs: number[] }>("transcript", { transcriptPath }),
  /** Where a phrase was said in the project's sermons (or one), up to `limit` places; its last word may be the start of one */
  searchTranscripts: (project: Project, phrase: string, sourceId: string | null, limit: number, prefix = true) =>
    invoke<{ found: Said[]; more: boolean; errors: string[] }>("search_transcripts", { project, phrase, prefix, sourceId, limit }),
  renderVerse: (project: Project, book: number, chapter: number, verse: number) =>
    invoke<ArrayBuffer>("render_verse", { project, book, chapter, verse }),
  /** When each chosen verse is heard in `renderChapter`'s audio */
  chapterTimeline: (project: Project, book: number, chapter: number) =>
    invoke<VerseTime[]>("chapter_timeline", { project, book, chapter }),
  chaptersToExport: (project: Project) => invoke<ChapterRef[]>("chapters_to_export", { project }),
  exportChapters: (project: Project, chapters: ChapterRef[] | null) =>
    invoke<ExportedFile[]>("export_chapters", { project, chapters }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
