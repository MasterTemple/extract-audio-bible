import { create } from "zustand";
import { toast } from "sonner";
import {
  api,
  errorMessage,
  type BibleSummary,
  type BookOutline,
  type FileMetadata,
  type Match,
  type Keyed,
  type MatchSet,
  type Project,
  type ProjectChanges,
  type Summary,
  type ProjectSummary,
  type Part,
  type Run,
  type Selection,
} from "./api";
import { verseKey } from "./refs";

export type Tab = "verses" | "readings" | "sources" | "search" | "export" | "bibles";

/** The readings of the chapters loaded so far (each chapter's are read when it's shown) */
interface Indexed {
  matchesByVerse: Map<string, Match[]>;
  matchById: Map<string, Match>;
  runById: Map<string, Run>;
}

interface AppState extends Indexed {
  bibles: BibleSummary[];
  projects: ProjectSummary[];
  project: Project | null;
  outline: BookOutline[];
  /** How many readings there are, and of which verses */
  summary: Summary;
  /** Each verse's count of readings, by verse key */
  verseCounts: Map<string, number>;
  /** The chapters whose readings are loaded (`book.chapter`) */
  loadedChapters: Set<string>;
  /** Book id → chapters of verse text */
  bookText: Record<number, string[][]>;
  tab: Tab;
  book: number | null;
  chapter: number | null;
  verse: number | null;
  saving: boolean;
  /** The transcript open, the time to show in it, and what to find in it */
  reading: { sourceId: string; at: number | null; query: string } | null;
  /** Steps to undo, the last most recent, and steps undone to redo */
  past: Step[];
  future: Step[];

  setTab: (tab: Tab) => void;
  refreshLists: () => Promise<void>;
  /** Opens a project; with `search`, on its Sermons tab, searching them at once */
  openProject: (id: string, options?: { search?: boolean }) => Promise<void>;
  /** Whether the Sermons tab is to search the sermons as soon as it's shown */
  searchOnOpen: boolean;
  closeProject: () => void;
  /** Sets the project as the backend changed it (adding or searching sermons), as a step to undo */
  setProject: (project: Project, label: string) => void;
  /**
   * Changes the project and saves it shortly after, as a step to undo called `label`. Changes
   * with the same `merge` key a moment apart (dragging, typing) are one step
   */
  update: (recipe: (p: Project) => Project, label: string, merge?: string) => void;
  undo: () => void;
  redo: () => void;
  /** Reads the readings again (after a search, or when sermons were added or taken out), from `summary` if given */
  resetMatches: (summary?: Summary) => Promise<void>;
  /** Reads a chapter's readings, if they aren't yet */
  loadChapter: (book: number, chapter: number) => Promise<void>;
  loadBook: (book: number) => Promise<string[][] | undefined>;
  go: (book: number, chapter: number, verse?: number | null) => void;
  setVerse: (verse: number | null) => void;
  /** Opens a sermon's transcript, at a time in it if given, finding a phrase in it if given; null closes it */
  openTranscript: (sourceId: string | null, at?: number | null, query?: string) => void;
  select: (match: Match) => void;
  selectMany: (matches: Match[]) => void;
  unselect: (key: string) => void;
  editSelection: (key: string, change: Partial<Selection>) => void;
  /** Changes what's known about sermons by their file ids, field by field (an empty field is dropped) */
  setFileMetadata: (given: Record<string, FileMetadata>) => void;
  /** Adds a reading to its verse's choice, played with it (a verse read in pieces), in the verse's order */
  join: (match: Match) => void;
  /** Takes a reading out of its verse's choice (the only one unchooses the verse) */
  unjoin: (key: string, matchId: string) => void;
  /** Plays a verse's readings in this order */
  arrange: (key: string, parts: Part[]) => void;
  /** Changes how one of a verse's readings is cut */
  editPart: (key: string, index: number, change: Partial<Part>) => void;
  flush: () => Promise<void>;
}

const EMPTY_SUMMARY: Summary = { analyzedAt: 0, readings: 0, runs: 0, verses: [] };

function emptyIndex(): Indexed {
  return { matchesByVerse: new Map(), matchById: new Map(), runById: new Map() };
}

/** The readings loaded, with a chapter's added (a run's readings of other chapters come with it) */
function withChapter(had: Indexed, set: MatchSet): Indexed {
  const matchesByVerse = new Map(had.matchesByVerse);
  const matchById = new Map(had.matchById);
  const touched = new Set<string>();
  for (const m of set.matches) {
    if (matchById.has(m.id)) continue;
    const key = verseKey(m.book, m.chapter, m.verse);
    matchesByVerse.set(key, [...(matchesByVerse.get(key) ?? []), m]);
    matchById.set(m.id, m);
    touched.add(key);
  }
  for (const key of touched) matchesByVerse.get(key)!.sort((a, b) => b.score - a.score);
  const runById = new Map(had.runById);
  for (const r of set.runs) runById.set(r.id, r);
  return { matchesByVerse, matchById, runById };
}

function countsOf(summary: Summary): Map<string, number> {
  return new Map(summary.verses.map(([b, c, v, n]) => [verseKey(b, c, v), n]));
}

/** The entries of a map that changed (by reference: what's unchanged is the same object), or null if none did */
function keyed<T>(from: Record<string, T> | undefined, to: Record<string, T> | undefined): Keyed<T> | null {
  if (from === to) return null;
  const set: Record<string, T> = {};
  const removed: string[] = [];
  for (const [key, value] of Object.entries(to ?? {})) if (from?.[key] !== value) set[key] = value;
  for (const key of Object.keys(from ?? {})) if (!to || !(key in to)) removed.push(key);
  return Object.keys(set).length > 0 || removed.length > 0 ? { set, removed } : null;
}

/**
 * What changed from one version of a project to the next, or null if nothing did. Changes copy only
 * what they change, so a part that's the same object is unchanged
 */
function changesOf(from: Project, to: Project): ProjectChanges | null {
  const changes: Record<string, unknown> = {};
  for (const key of Object.keys(to) as (keyof Project)[]) {
    if (key === "id" || key === "createdAt" || key === "updatedAt" || key === "selections" || key === "files") continue;
    if (to[key] !== from[key]) changes[key] = to[key] ?? null;
  }
  const selections = keyed(from.selections, to.selections);
  if (selections) changes.selections = selections;
  const files = keyed(from.files, to.files);
  if (files) changes.files = files;
  return Object.keys(changes).length > 0 ? (changes as ProjectChanges) : null;
}

/** The project's sermons, as a key to tell when they change */
const sermonsKey = (p: Project | null) => (p ? p.sources.map((s) => s.id).join() : "");

export function selectionFor(m: Match, previous?: Selection): Selection {
  return {
    matchId: m.id,
    sourceId: m.sourceId,
    start: m.start,
    end: m.end,
    trimStart: 0,
    trimEnd: 0,
    padBefore: previous?.padBefore ?? 0,
    padAfter: previous?.padAfter ?? 0,
    gainDb: previous?.gainDb ?? 0,
    needsTrim: false,
    note: previous?.note ?? "",
  };
}

/** Every reading of a verse's choice, in the order they're played */
export function partsOf(sel: Selection): Part[] {
  const { matchId, sourceId, start, end, trimStart, trimEnd } = sel;
  return [{ matchId, sourceId, start, end, trimStart, trimEnd }, ...(sel.joined ?? [])];
}

/** The choice playing these readings in this order (the first becomes its own) */
export function withParts(sel: Selection, parts: Part[]): Selection {
  const [first, ...joined] = parts;
  if (!first) return sel;
  return { ...sel, ...first, joined };
}

/** Where in its verse a reading begins: the first of the verse's words heard */
function begins(m: Match): number {
  const i = m.verseWordsHeard.indexOf(true);
  return i < 0 ? Infinity : i;
}

/** A project as it was before a change, to go back to */
export interface Step {
  project: Project;
  /** What the change was ("choose a reading") */
  label: string;
}

/** The most steps kept to undo */
const HISTORY = 200;
/** Changes with the same merge key less than this apart (ms) are one step */
const MERGE_WITHIN = 1000;

const LAST_PROJECT = "cab:lastProject";
let saveTimer: ReturnType<typeof setTimeout> | null = null;
/** The project as the backend has it (last read or saved), to send only what's changed since */
let saved: Project | null = null;
/** The save under way: saves are made one after another, in order */
let saving: Promise<void> = Promise.resolve();
/** Counts what's been opened or searched, so readings read for an earlier one are dropped */
let generation = 0;
/** Chapters being read, by `book.chapter` */
const loading = new Map<string, Promise<void>>();
/** The sermons the readings loaded were read for */
let readFor = "";
/** The last change recorded, to merge with */
let lastChange: { merge: string; at: number } | null = null;

export const useApp = create<AppState>((set, get) => {
  /** Sends what's changed since the last save (after any save under way): the project as it is now */
  const save = () => {
    saveTimer = null;
    const project = get().project;
    saving = saving.then(async () => {
      if (!project || saved?.id !== project.id) return;
      const changes = changesOf(saved, project);
      if (!changes) return;
      set({ saving: true });
      try {
        await api.saveChanges(project.id, changes);
        saved = project;
        // Saving keeps what's read of the readings to the project's sermons
        if (sermonsKey(project) !== readFor && get().project === project) void get().resetMatches();
      } catch (e) {
        // What wasn't saved is sent with the next save
        toast.error(`Couldn't save: ${errorMessage(e)}`);
      } finally {
        set({ saving: false });
      }
    });
    return saving;
  };

  const scheduleSave = () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 400);
  };

  /** Keeps the project as it is now, to undo the change about to be made */
  const record = (label: string, merge?: string) => {
    const { project, past } = get();
    if (!project) return;
    const now = Date.now();
    const merging = merge !== undefined && lastChange?.merge === merge && now - lastChange.at < MERGE_WITHIN;
    lastChange = merge === undefined ? null : { merge, at: now };
    if (merging) return;
    set({ past: [...past, { project, label }].slice(-HISTORY), future: [] });
  };

  /** Goes to a step, keeping where it came from on `to`'s side */
  const travel = (from: "past" | "future") => {
    const state = get();
    const steps = state[from];
    const step = steps[steps.length - 1];
    if (!step || !state.project) return;
    const here: Step = { project: state.project, label: step.label };
    const other = from === "past" ? "future" : "past";
    lastChange = null;
    set({
      [from]: steps.slice(0, -1),
      [other]: [...state[other], here],
      project: step.project,
    });
    scheduleSave();
    toast(`${from === "past" ? "Undid" : "Redid"}: ${step.label}`);
  };

  return {
    bibles: [],
    projects: [],
    project: null,
    outline: [],
    summary: EMPTY_SUMMARY,
    verseCounts: new Map(),
    loadedChapters: new Set(),
    ...emptyIndex(),
    bookText: {},
    tab: "verses",
    book: null,
    chapter: null,
    verse: null,
    saving: false,
    reading: null,
    past: [],
    future: [],

    setTab: (tab) => set({ tab }),

    refreshLists: async () => {
      const [bibles, projects] = await Promise.all([api.listBibles(), api.listProjects()]);
      set({ bibles, projects });
    },

    searchOnOpen: false,

    openProject: async (id, options) => {
      await get().flush();
      const project = await api.getProject(id);
      saved = project;
      // Readings kept as JSON before are moved into the readings database the first time, which takes a while
      const slow = setTimeout(() => toast.loading("Reading the project's readings…", { id: "summary" }), 800);
      let outline: BookOutline[];
      let summary: Summary;
      try {
        [outline, summary] = await Promise.all([api.bibleOutline(project.bibleId), api.matchSummary(id)]);
      } finally {
        clearTimeout(slow);
        toast.dismiss("summary");
      }
      try {
        localStorage.setItem(LAST_PROJECT, id);
      } catch {
        // Remembering the last project is only a convenience
      }
      const first =
        outline.find((b) => b.id === project.book) ??
        outline.find((b) => summary.verses.some(([book]) => book === b.id)) ??
        outline[0];
      set({
        project,
        outline,
        summary,
        verseCounts: countsOf(summary),
        loadedChapters: new Set(),
        ...emptyIndex(),
        bookText: {},
        book: first?.id ?? null,
        // A passage opens at its first chapter
        chapter: first ? (project.passage && first.id === project.book ? project.passage.startChapter : 1) : null,
        verse: null,
        tab: project.sources.length === 0 || options?.search ? "sources" : project.kind === "readings" ? "readings" : "verses",
        searchOnOpen: !!options?.search,
        past: [],
        future: [],
      });
      lastChange = null;
      generation++;
      loading.clear();
      readFor = sermonsKey(project);
      if (first) {
        void get().loadBook(first.id);
        void get().loadChapter(first.id, get().chapter ?? 1);
      }
    },

    closeProject: () => {
      // Saved before it's closed (with what it's saved against)
      void get().flush().then(() => {
        if (!get().project) saved = null;
      });
      try {
        localStorage.removeItem(LAST_PROJECT);
      } catch {
        // ignore
      }
      generation++;
      loading.clear();
      set({
        project: null,
        outline: [],
        summary: EMPTY_SUMMARY,
        verseCounts: new Map(),
        loadedChapters: new Set(),
        ...emptyIndex(),
        bookText: {},
        book: null,
        chapter: null,
        verse: null,
        past: [],
        future: [],
      });
      void get().refreshLists();
    },

    setProject: (project, label) => {
      record(label);
      // As the backend changed and saved it
      saved = project;
      set({ project });
    },

    update: (recipe, label, merge) => {
      const project = get().project;
      if (!project) return;
      const changed = recipe(project);
      if (changed === project) return;
      record(label, merge);
      set({ project: changed });
      scheduleSave();
    },

    undo: () => travel("past"),
    redo: () => travel("future"),

    resetMatches: async (given) => {
      const project = get().project;
      if (!project) return;
      const at = ++generation;
      loading.clear();
      readFor = sermonsKey(project);
      try {
        const summary = given ?? (await api.matchSummary(project.id));
        if (at !== generation) return;
        set({ summary, verseCounts: countsOf(summary), loadedChapters: new Set(), ...emptyIndex() });
        const { book, chapter } = get();
        if (book && chapter) await get().loadChapter(book, chapter);
      } catch (e) {
        toast.error(`Couldn't read the readings: ${errorMessage(e)}`);
      }
    },

    loadChapter: (book, chapter) => {
      const key = `${book}.${chapter}`;
      const project = get().project;
      if (!project || get().loadedChapters.has(key)) return Promise.resolve();
      const pending = loading.get(key);
      if (pending) return pending;
      const at = generation;
      const read = api.chapterMatches(project.id, book, chapter).then(
        (found) => {
          loading.delete(key);
          if (at !== generation) return;
          set((s) => ({ ...withChapter(s, found), loadedChapters: new Set(s.loadedChapters).add(key) }));
        },
        (e) => {
          loading.delete(key);
          toast.error(`Couldn't read ${key}'s readings: ${errorMessage(e)}`);
        },
      );
      loading.set(key, read);
      return read;
    },

    loadBook: async (book) => {
      const have = get().bookText[book];
      if (have) return have;
      const project = get().project;
      if (!project) return;
      const text = await api.bookText(project.bibleId, book);
      set((s) => ({ bookText: { ...s.bookText, [book]: text } }));
      return text;
    },

    go: (book, chapter, verse = null) => {
      set({ book, chapter, verse, tab: "verses" });
      void get().loadBook(book);
      void get().loadChapter(book, chapter);
    },

    setVerse: (verse) => set({ verse }),

    openTranscript: (sourceId, at = null, query = "") => set({ reading: sourceId ? { sourceId, at, query } : null }),

    select: (m) => get().selectMany([m]),

    selectMany: (matches) =>
      get().update((p) => {
        const selections = { ...p.selections };
        for (const m of matches) {
          const key = verseKey(m.book, m.chapter, m.verse);
          selections[key] = selectionFor(m, selections[key]);
        }
        return { ...p, selections };
      }, matches.length === 1 ? "choose a reading" : `choose ${matches.length} readings`),

    unselect: (key) =>
      get().update((p) => {
        const selections = { ...p.selections };
        delete selections[key];
        return { ...p, selections };
      }, "unchoose a verse"),

    editSelection: (key, change) =>
      get().update((p) => {
        const current = p.selections[key];
        if (!current) return p;
        return { ...p, selections: { ...p.selections, [key]: { ...current, ...change } } };
      }, "change a verse", `verse:${key}:${Object.keys(change).join()}`),

    setFileMetadata: (given) =>
      get().update((p) => {
        const files = { ...p.files };
        for (const [id, change] of Object.entries(given)) {
          const merged: FileMetadata = { ...files[id], ...change };
          for (const key of ["title", "passage", "speaker", "series"] as const) if (!merged[key]?.trim()) delete merged[key];
          if (Object.keys(merged).length === 0) delete files[id];
          else files[id] = merged;
        }
        return { ...p, files };
      }, "change a sermon's title or passage", `file:${Object.keys(given).join()}`),

    join: (m) =>
      get().update((p) => {
        const key = verseKey(m.book, m.chapter, m.verse);
        const current = p.selections[key];
        if (!current) return { ...p, selections: { ...p.selections, [key]: selectionFor(m) } };
        const parts = partsOf(current);
        if (parts.some((x) => x.matchId === m.id)) return p;
        const { matchById } = get();
        const later = parts.findIndex((x) => {
          const other = matchById.get(x.matchId);
          return other !== undefined && begins(other) > begins(m);
        });
        const part: Part = { matchId: m.id, sourceId: m.sourceId, start: m.start, end: m.end, trimStart: 0, trimEnd: 0 };
        parts.splice(later < 0 ? parts.length : later, 0, part);
        return { ...p, selections: { ...p.selections, [key]: withParts(current, parts) } };
      }, "join a reading"),

    unjoin: (key, matchId) =>
      get().update((p) => {
        const current = p.selections[key];
        if (!current) return p;
        const parts = partsOf(current).filter((x) => x.matchId !== matchId);
        const selections = { ...p.selections };
        if (parts.length === 0) delete selections[key];
        else selections[key] = withParts(current, parts);
        return { ...p, selections };
      }, "take out a reading"),

    arrange: (key, parts) =>
      get().update((p) => {
        const current = p.selections[key];
        if (!current || parts.length === 0) return p;
        return { ...p, selections: { ...p.selections, [key]: withParts(current, parts) } };
      }, "reorder a verse's readings"),

    editPart: (key, index, change) =>
      get().update((p) => {
        const current = p.selections[key];
        if (!current) return p;
        const parts = partsOf(current);
        if (!parts[index]) return p;
        parts[index] = { ...parts[index], ...change };
        return { ...p, selections: { ...p.selections, [key]: withParts(current, parts) } };
      }, "cut a reading", `part:${key}:${index}`),

    flush: async () => {
      if (saveTimer) {
        clearTimeout(saveTimer);
        void save();
      }
      await saving;
    },
  };
});

/** The open project's id, once what's changed in it is saved: for what reads the project as the backend has it */
export async function savedProjectId(): Promise<string> {
  await useApp.getState().flush();
  const project = useApp.getState().project;
  if (!project) throw new Error("No project is open");
  return project.id;
}

export function lastProjectId(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT);
  } catch {
    return null;
  }
}
