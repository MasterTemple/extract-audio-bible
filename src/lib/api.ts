import { Channel, invoke } from "@tauri-apps/api/core";

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
  /** Who preached it */
  speaker?: string | null;
  /** The series it's part of */
  series?: string | null;
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
  /** Who preached it, and its series, if known */
  speaker: string | null;
  series: string | null;
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

/** Verses of a project's book from one to another (2:14 to 2:17, or 7:24 to 8:2) */
export interface Passage {
  startChapter: number;
  startVerse: number;
  endChapter: number;
  endVerse: number;
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
  /** Only these verses of the book (exported as one file, and its verses as an album of their own) */
  passage?: Passage | null;
  createdAt: number;
  updatedAt: number;
  sources: Source[];
  selections: Record<string, Selection>;
  settings: Settings;
  metadata: Metadata;
  /** What's known about each sermon, by its file id, kept apart from the sermons */
  files?: Record<string, FileMetadata>;
}

/** What a search of transcripts has found since it last said, as it goes */
export interface SearchProgress {
  /** Places found since, in the sermons' order (after those given before) */
  found: Said[];
  /** Sermons whose transcripts couldn't be read since, with why */
  errors: string[];
  /** How many sermons have been searched, of how many */
  searched: number;
  total: number;
  /** Whether it's over: every sermon searched, the limit reached, or a newer search begun */
  done: boolean;
  /** Whether there were more than the limit */
  more: boolean;
}

/** Entries of a map changed: those set (added or changed), and those taken out */
export interface Keyed<T> {
  set: Record<string, T>;
  removed: string[];
}

/** What's changed in a project since it was saved: each part given replaces its own, and chosen verses and sermon information change entry by entry */
export type ProjectChanges = Partial<Omit<Project, "id" | "createdAt" | "updatedAt" | "selections" | "files">> & {
  selections?: Keyed<Selection>;
  files?: Keyed<FileMetadata>;
};

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
  /** The folder it's kept in, when it was opened from one rather than made here */
  folder: string | null;
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
  /** The words said, as the transcript has them (empty from searches before it was kept) */
  said?: string;
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

/** How many readings a project has, and of which verses (each chapter's are read as it's shown) */
export interface Summary {
  /** When the sermons were last searched (0 if never) */
  analyzedAt: number;
  readings: number;
  runs: number;
  /** Each verse that has readings: [book, chapter, verse, readings] */
  verses: [number, number, number, number][];
}

/** Which readings to list: each given part narrows them */
/** A way to finish a reference being typed: what a menu shows, the whole text it becomes, and its book */
export interface ReferenceCompletion {
  label: string;
  text: string;
  book: number;
}

export interface RowFilter {
  /** Only readings of these kinds (none: any) */
  kinds?: MatchKind[];
  /** The translation each fits best, by abbreviation */
  translation?: string | null;
  speakers?: string[];
  series?: string[];
  /** Words its reference has ("Romans 8") */
  reference?: string;
}

/** Which of the readings filtered to list: a sermon's, a chapter's, or all */
export type RowGroup = { by: "sermon"; id: string } | { by: "chapter"; book: number; chapter: number } | { by: "all" };

export interface SermonGroup {
  id: string;
  title: string;
  speaker: string | null;
  series: string | null;
  readings: number;
  verses: number;
}

export interface ChapterGroup {
  chapter: number;
  readings: number;
  verses: number;
}

export interface BookGroup {
  book: number;
  name: string;
  readings: number;
  verses: number;
  chapters: ChapterGroup[];
}

/** The readings filtered, counted, and grouped (each group's rows are read when it's opened) */
export interface Overview {
  /** All of them, unfiltered */
  readings: number;
  verses: number;
  books: number;
  sermons: number;
  /** How many each translation fits best, most first (unfiltered) */
  translations: [string, number][];
  speakers: string[];
  series: string[];
  /** How many the filter keeps */
  shown: number;
  bySermon: SermonGroup[];
  byBook: BookGroup[];
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
    // `warnings`: what was left out (files not read, books passed over), a sentence each
    invoke<BibleSummary & { warnings: string[] }>("import_bible", { paths, name, abbreviation, language }),
  deleteBible: (id: string) => invoke<void>("delete_bible", { id }),
  /** Renames a translation, wherever it's kept and shown */
  renameBible: (id: string, name: string, abbreviation: string, language: string | null) =>
    invoke<BibleSummary>("rename_bible", { id, name, abbreviation, language }),
  bibleOutline: (id: string) => invoke<BookOutline[]>("bible_outline", { id }),
  bookText: (bibleId: string, book: number) => invoke<string[][]>("book_text", { bibleId, book }),

  listProjects: () => invoke<ProjectSummary[]>("list_projects"),
  /** An audio Bible of a book, some of its verses (`passage`, like "2:14-17"), or the whole Bible */
  createProject: (name: string, bibleId: string, book: number | null, passage: string | null = null) =>
    invoke<Project>("create_project", { name, bibleId, book, passage }),
  /** An audio Bible from some of another project's sermons (their files, what's known of them, and where their audio is) */
  createProjectFrom: (projectId: string, sourceIds: string[], name: string, bibleId: string, book: number | null, passage: string | null) =>
    invoke<Project>("create_project_from", { projectId, sourceIds, name, bibleId, book, passage }),
  createReadingsProject: (name: string, bibleIds: string[], scope: BookScope) =>
    invoke<Project>("create_readings_project", { name, bibleIds, scope }),
  genres: () => invoke<Genre[]>("genres"),
  /** The books a scope searches, or null for every book; an error if it leaves nothing or names something unknown */
  scopeBooks: (scope: BookScope) => invoke<number[] | null>("scope_books", { scope }),
  /** The readings a filter keeps, counted and grouped by sermon and by book and chapter */
  /** Ways to finish a reference being typed (`2 Co` → `2 Corinthians`), best first */
  completeReference: (text: string, limit: number) => invoke<ReferenceCompletion[]>("complete_reference", { text, limit }),
  readingsOverview: (projectId: string, filter: RowFilter) => invoke<Overview>("readings_overview", { projectId, filter }),
  /** The readings a filter keeps of one group (a sermon's, a chapter's), as rows */
  readingsRows: (projectId: string, filter: RowFilter, group: RowGroup) =>
    invoke<ReadingRow[]>("readings_rows", { projectId, filter, group }),
  /** Every reading a filter keeps written to `path` (CSV, or JSON for a .json path); how many */
  exportReadings: (projectId: string, filter: RowFilter, path: string) =>
    invoke<number>("export_readings", { projectId, filter, path }),
  getProject: (id: string) => invoke<Project>("get_project", { id }),
  /** Saves what's changed since the project was last saved; when it was changed */
  saveChanges: (projectId: string, changes: ProjectChanges) => invoke<number>("save_changes", { projectId, changes }),
  /** Deletes a project made here; one opened from a folder is only closed, its folder kept */
  deleteProject: (id: string) => invoke<void>("delete_project", { id }),
  /** Opens a project kept in a folder of its own (as the command line makes them), where it is */
  openProjectFolder: (path: string) => invoke<Project>("open_project_folder", { path }),
  addSources: (projectId: string, paths: string[]) =>
    invoke<{ project: Project; added: number; paired: number; unpaired: string[]; skipped: string[] }>("add_sources", { projectId, paths }),
  /** Searches the project as it was last saved */
  analyze: (projectId: string) =>
    invoke<{ project: Project; summary: Summary; errors: string[] }>("analyze", { projectId }),
  /** How many readings there are, and of which verses (made from the readings kept as JSON before, the first time) */
  matchSummary: (projectId: string) => invoke<Summary>("match_summary", { projectId }),
  /** A chapter's readings (best first in each verse), with their runs and those runs' other readings */
  chapterMatches: (projectId: string, book: number, chapter: number) =>
    invoke<MatchSet>("chapter_matches", { projectId, book, chapter }),
  /** A sermon's readings as said, with its runs */
  sermonMatches: (projectId: string, sourceId: string) => invoke<MatchSet>("sermon_matches", { projectId, sourceId }),
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
  // These read the project as it was last saved: save it first (the store's `saved`)
  renderChapter: (projectId: string, book: number, chapter: number) =>
    invoke<ArrayBuffer>("render_chapter", { projectId, book, chapter }),
  /** A verse as it will be exported: all its readings, one after another, with its silence */
  /** Titles and passages by file id, from a JSON file (`{"<file id>": {"title": …, "passage": …}}`) */
  /** A text file (a table of sermons' titles to import), as text */
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  /** A whole transcript, in paragraphs (where each begins, by word) */
  transcript: (transcriptPath: string) =>
    invoke<{ id: string | null; title: string | null; words: Word[]; paragraphs: number[] }>("transcript", { transcriptPath }),
  /** Where a phrase was said in the project's sermons (or some of them), up to `limit` places; its last word may be the start of one */
  searchTranscripts: (
    projectId: string,
    phrase: string,
    sourceIds: string[] | null,
    limit: number,
    {
      prefix = true,
      key = "search",
      onProgress,
    }: {
      prefix?: boolean;
      /** A newer search with the same key stops this one (the Search tab's and a transcript's don't stop each other) */
      key?: string;
      /** Given what's found as it's found, a batch of sermons at a time, in the sermons' order */
      onProgress?: (progress: SearchProgress) => void;
    } = {},
  ) => {
    const channel = new Channel<SearchProgress>();
    if (onProgress) channel.onmessage = onProgress;
    return invoke<{ found: Said[]; more: boolean; errors: string[] }>("search_transcripts", {
      projectId,
      phrase,
      prefix,
      sourceIds,
      limit,
      search: key,
      onProgress: channel,
    });
  },
  renderVerse: (projectId: string, book: number, chapter: number, verse: number) =>
    invoke<ArrayBuffer>("render_verse", { projectId, book, chapter, verse }),
  /** When each chosen verse is heard in `renderChapter`'s audio */
  chapterTimeline: (projectId: string, book: number, chapter: number) =>
    invoke<VerseTime[]>("chapter_timeline", { projectId, book, chapter }),
  chaptersToExport: (projectId: string) => invoke<ChapterRef[]>("chapters_to_export", { projectId }),
  exportChapters: (projectId: string, chapters: ChapterRef[] | null) =>
    invoke<ExportedFile[]>("export_chapters", { projectId, chapters }),
  /** Project folders to run the benchmark on (`EAB_BENCH`), none normally */
  benchFolders: () => invoke<string[]>("bench_folders"),
  /** Only the benchmark's steps with these words in their names (`EAB_BENCH_STEPS`); all, if none */
  benchSteps: () => invoke<string[]>("bench_steps"),
  /** A line of the benchmark's progress, printed as it goes */
  benchLog: (line: string) => invoke<void>("bench_log", { line }),
  /** The benchmark's report, printed, and the app closed */
  benchDone: (report: string) => invoke<void>("bench_done", { report }),
};

/** How a command fails: its message, shown as is, and what kind of failure it is */
export interface AppError {
  kind: "notFound" | "io" | "invalid" | "audio" | "network" | "app";
  message: string;
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === "object" && e !== null && typeof (e as AppError).message === "string" && "kind" in e;
}

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (isAppError(e)) return e.message;
  return JSON.stringify(e);
}
