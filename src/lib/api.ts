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
  needsTrim: boolean;
  note: string;
}

export interface Settings {
  minScore: number;
  gapBetweenVerses: number;
  chapterLeadIn: number;
  chapterTail: number;
  normalize: boolean;
  targetDb: number;
  fadeMs: number;
}

export interface Metadata {
  titleTemplate: string;
  albumTemplate: string;
  filenameTemplate: string;
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

export interface Project {
  id: string;
  name: string;
  bibleId: string;
  createdAt: number;
  updatedAt: number;
  sources: Source[];
  selections: Record<string, Selection>;
  settings: Settings;
  metadata: Metadata;
}

export interface ProjectSummary {
  id: string;
  name: string;
  bibleId: string;
  updatedAt: number;
  sourceCount: number;
  selectionCount: number;
}

export type MatchKind = "exact" | "close" | "partial";

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
  coverage: number;
  precision: number;
  kind: MatchKind;
  verseWordsHeard: boolean[];
  extraWords: number[];
  runId: string | null;
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
  createProject: (name: string, bibleId: string) => invoke<Project>("create_project", { name, bibleId }),
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
  }) => invoke<ArrayBuffer>("render_clip", { request }),
  renderChapter: (project: Project, book: number, chapter: number) =>
    invoke<ArrayBuffer>("render_chapter", { project, book, chapter }),
  chaptersToExport: (project: Project) => invoke<ChapterRef[]>("chapters_to_export", { project }),
  exportChapters: (project: Project, chapters: ChapterRef[] | null) =>
    invoke<ExportedFile[]>("export_chapters", { project, chapters }),
};

export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}
