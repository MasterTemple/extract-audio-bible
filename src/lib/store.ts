import { create } from "zustand";
import { toast } from "sonner";
import {
  api,
  errorMessage,
  type BibleSummary,
  type BookOutline,
  type Match,
  type MatchSet,
  type Project,
  type ProjectSummary,
  type Run,
  type Selection,
} from "./api";
import { verseKey } from "./refs";

export type Tab = "verses" | "sources" | "export" | "bibles";

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

  setTab: (tab: Tab) => void;
  refreshLists: () => Promise<void>;
  openProject: (id: string) => Promise<void>;
  closeProject: () => void;
  setProject: (project: Project) => void;
  /** Change the project and save it shortly after */
  update: (recipe: (p: Project) => Project) => void;
  setMatches: (set: MatchSet) => void;
  loadBook: (book: number) => Promise<string[][] | undefined>;
  go: (book: number, chapter: number, verse?: number | null) => void;
  setVerse: (verse: number | null) => void;
  select: (match: Match) => void;
  selectMany: (matches: Match[]) => void;
  unselect: (key: string) => void;
  editSelection: (key: string, change: Partial<Selection>) => void;
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

const LAST_PROJECT = "cab:lastProject";
let saveTimer: ReturnType<typeof setTimeout> | null = null;

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
        tab: project.sources.length === 0 ? "sources" : "verses",
      });
      if (first) void get().loadBook(first.id);
    },

    closeProject: () => {
      void get().flush();
      try {
        localStorage.removeItem(LAST_PROJECT);
      } catch {
        // ignore
      }
      set({ project: null, outline: [], bookText: {}, book: null, chapter: null, verse: null });
      void get().refreshLists();
    },

    setProject: (project) => set({ project }),

    update: (recipe) => {
      const project = get().project;
      if (!project) return;
      set({ project: recipe(project) });
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = setTimeout(save, 400);
    },

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

    select: (m) => get().selectMany([m]),

    selectMany: (matches) =>
      get().update((p) => {
        const selections = { ...p.selections };
        for (const m of matches) {
          const key = verseKey(m.book, m.chapter, m.verse);
          selections[key] = selectionFor(m, selections[key]);
        }
        return { ...p, selections };
      }),

    unselect: (key) =>
      get().update((p) => {
        const selections = { ...p.selections };
        delete selections[key];
        return { ...p, selections };
      }),

    editSelection: (key, change) =>
      get().update((p) => {
        const current = p.selections[key];
        if (!current) return p;
        return { ...p, selections: { ...p.selections, [key]: { ...current, ...change } } };
      }),

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
