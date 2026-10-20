import { create } from "zustand";
import { toast } from "sonner";
import {
  api,
  errorMessage,
  type BibleSummary,
  type BookOutline,
  type FileMetadata,
  type Match,
  type MatchSet,
  type Project,
  type ProjectSummary,
  type Part,
  type Run,
  type Selection,
} from "./api";
import { verseKey } from "./refs";

export type Tab = "verses" | "readings" | "sources" | "search" | "export" | "bibles";

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
  matchSet: MatchSet;
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
  openProject: (id: string) => Promise<void>;
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
  setMatches: (set: MatchSet) => void;
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

function index(set: MatchSet): Indexed {
  const matchesByVerse = new Map<string, Match[]>();
  const matchById = new Map<string, Match>();
  for (const m of set.matches) {
    const key = verseKey(m.book, m.chapter, m.verse);
    const list = matchesByVerse.get(key);
    if (list) list.push(m);
    else matchesByVerse.set(key, [m]);
    matchById.set(m.id, m);
  }
  for (const list of matchesByVerse.values()) list.sort((a, b) => b.score - a.score);
  return { matchesByVerse, matchById, runById: new Map(set.runs.map((r) => [r.id, r])) };
}

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
  matchSet: MatchSet;
  /** What the change was ("choose a reading") */
  label: string;
}

/** The most steps kept to undo */
const HISTORY = 200;
/** Changes with the same merge key less than this apart (ms) are one step */
const MERGE_WITHIN = 1000;

const LAST_PROJECT = "cab:lastProject";
let saveTimer: ReturnType<typeof setTimeout> | null = null;
/** The last change recorded, to merge with */
let lastChange: { merge: string; at: number } | null = null;

export const useApp = create<AppState>((set, get) => {
  const save = async () => {
    saveTimer = null;
    const project = get().project;
    if (!project) return;
    set({ saving: true });
    try {
      await api.saveProject(project);
    } catch (e) {
      toast.error(`Couldn't save: ${errorMessage(e)}`);
    } finally {
      set({ saving: false });
    }
  };

  const scheduleSave = () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(save, 400);
  };

  /** Keeps the project as it is now, to undo the change about to be made */
  const record = (label: string, merge?: string) => {
    const { project, matchSet, past } = get();
    if (!project) return;
    const now = Date.now();
    const merging = merge !== undefined && lastChange?.merge === merge && now - lastChange.at < MERGE_WITHIN;
    lastChange = merge === undefined ? null : { merge, at: now };
    if (merging) return;
    set({ past: [...past, { project, matchSet, label }].slice(-HISTORY), future: [] });
  };

  /** Goes to a step, keeping where it came from on `to`'s side */
  const travel = (from: "past" | "future") => {
    const state = get();
    const steps = state[from];
    const step = steps[steps.length - 1];
    if (!step || !state.project) return;
    const here: Step = { project: state.project, matchSet: state.matchSet, label: step.label };
    const other = from === "past" ? "future" : "past";
    lastChange = null;
    set({
      [from]: steps.slice(0, -1),
      [other]: [...state[other], here],
      project: step.project,
      ...(step.matchSet !== state.matchSet ? { matchSet: step.matchSet, ...index(step.matchSet) } : {}),
    });
    scheduleSave();
    toast(`${from === "past" ? "Undid" : "Redid"}: ${step.label}`);
  };

  return {
    bibles: [],
    projects: [],
    project: null,
    outline: [],
    matchSet: { analyzedAt: 0, matches: [], runs: [] },
    ...index({ analyzedAt: 0, matches: [], runs: [] }),
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

    openProject: async (id) => {
      await get().flush();
      const project = await api.getProject(id);
      const [outline, matchSet] = await Promise.all([api.bibleOutline(project.bibleId), api.getMatches(id)]);
      try {
        localStorage.setItem(LAST_PROJECT, id);
      } catch {
        // Remembering the last project is only a convenience
      }
      const first =
        outline.find((b) => b.id === project.book) ??
        outline.find((b) => matchSet.matches.some((m) => m.book === b.id)) ??
        outline[0];
      set({
        project,
        outline,
        matchSet,
        ...index(matchSet),
        bookText: {},
        book: first?.id ?? null,
        chapter: first ? 1 : null,
        verse: null,
        tab: project.sources.length === 0 ? "sources" : project.kind === "readings" ? "readings" : "verses",
        past: [],
        future: [],
      });
      lastChange = null;
      if (first) void get().loadBook(first.id);
    },

    closeProject: () => {
      void get().flush();
      try {
        localStorage.removeItem(LAST_PROJECT);
      } catch {
        // ignore
      }
      set({ project: null, outline: [], bookText: {}, book: null, chapter: null, verse: null, past: [], future: [] });
      void get().refreshLists();
    },

    setProject: (project, label) => {
      record(label);
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

    setMatches: (matchSet) => set({ matchSet, ...index(matchSet) }),

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
          for (const key of ["title", "passage"] as const) if (!merged[key]?.trim()) delete merged[key];
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
        await save();
      }
    },
  };
});

export function lastProjectId(): string | null {
  try {
    return localStorage.getItem(LAST_PROJECT);
  } catch {
    return null;
  }
}
