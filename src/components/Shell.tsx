import { useEffect, useMemo } from "react";
import { ArrowLeft, BookOpen, Check, Loader2, Redo2, Undo2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useApp, type Tab } from "@/lib/store";
import { inPassage, projectReference, versesNeeded } from "@/lib/refs";
import { SourcesPanel } from "./SourcesPanel";
import { VersesPanel } from "./VersesPanel";
import { ExportPanel } from "./ExportPanel";
import { TranscriptView } from "./TranscriptView";
import { SearchPanel } from "./SearchPanel";
import { ReadingsPanel } from "./ReadingsPanel";

/** Whether a key press belongs to a text field (which has its own undo) */
function typing(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target instanceof HTMLTextAreaElement) return true;
  return target instanceof HTMLInputElement && !["checkbox", "radio", "range", "button", "submit"].includes(target.type);
}

export function Shell() {
  const { project, bibles, tab, setTab, saving, closeProject, outline, past, future, undo, redo, summary } = useApp();
  const readings = project?.kind === "readings";

  // Ctrl+Z undoes, Ctrl+Shift+Z and Ctrl+Y redo (⌘ on a Mac), except while typing
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey || typing(e.target)) return;
      const key = e.key.toLowerCase();
      if (key === "z" && !e.shiftKey) undo();
      else if ((key === "z" && e.shiftKey) || key === "y") redo();
      else return;
      e.preventDefault();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [undo, redo]);
  const bible = bibles.find((b) => b.id === project?.bibleId);

  const progress = useMemo(() => {
    if (!project) return { done: 0, total: 0 };
    let total = 0;
    const books = outline.filter((b) => project.book === null || b.id === project.book);
    for (const b of books) for (let c = 1; c <= b.chapters.length; c++) total += versesNeeded(b, c, project).length;
    const done = Object.keys(project.selections).filter((k) => {
      const [book, chapter, verse] = k.split(".").map(Number);
      return (project.book === null || book === project.book) && inPassage(project, book, chapter, verse);
    }).length;
    return { done, total };
  }, [project, outline]);

  if (!project) return null;

  return (
    <div className="flex h-screen flex-col bg-background">
      <header className="flex items-center gap-3 border-b px-3 py-2">
        <Button variant="ghost" size="icon-sm" onClick={closeProject} title="All projects">
          <ArrowLeft />
        </Button>
        <div className="flex size-7 items-center justify-center rounded-lg bg-primary text-primary-foreground">
          <BookOpen className="size-4" />
        </div>
        <div className="min-w-0">
          <div className="truncate text-sm leading-tight font-semibold">{project.name}</div>
          <div className="truncate text-xs leading-tight text-muted-foreground">
            {readings ? (
              <>
                Every verse read ·{" "}
                {(project.translations ?? [project.bibleId])
                  .map((id) => bibles.find((b) => b.id === id)?.abbreviation || bibles.find((b) => b.id === id)?.name || id)
                  .join(", ")}{" "}
                · {summary.readings.toLocaleString()} readings found
              </>
            ) : (
              <>
                {project.book !== null ? `${projectReference(project, outline)} · ` : ""}
                {bible?.abbreviation || bible?.name || "Translation"} · {progress.done.toLocaleString()} of {progress.total.toLocaleString()} verses
                chosen
              </>
            )}
          </div>
        </div>
        <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)} className="mx-auto">
          <TabsList>
            {readings ? <TabsTrigger value="readings">Readings</TabsTrigger> : <TabsTrigger value="verses">Verses</TabsTrigger>}
            <TabsTrigger value="sources">
              Sermons <span className="text-muted-foreground">{project.sources.length}</span>
            </TabsTrigger>
            <TabsTrigger value="search">Search</TabsTrigger>
            {!readings && <TabsTrigger value="export">Export</TabsTrigger>}
          </TabsList>
        </Tabs>
        <div className="flex w-56 items-center justify-end gap-1.5 text-xs text-muted-foreground">
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={past.length === 0}
            onClick={undo}
            title={past.length ? `Undo ${past[past.length - 1].label} (Ctrl+Z)` : "Nothing to undo"}
          >
            <Undo2 />
          </Button>
          <Button
            variant="ghost"
            size="icon-sm"
            disabled={future.length === 0}
            onClick={redo}
            title={future.length ? `Redo ${future[future.length - 1].label} (Ctrl+Shift+Z)` : "Nothing to redo"}
          >
            <Redo2 />
          </Button>
          {saving ? (
            <>
              <Loader2 className="size-3 animate-spin" /> Saving
            </>
          ) : (
            <>
              <Check className="size-3" /> Saved
            </>
          )}
        </div>
      </header>
      <main className="min-h-0 flex-1">
        {tab === "verses" && !readings && <VersesPanel />}
        {(tab === "readings" || (tab === "verses" && readings)) && <ReadingsPanel />}
        {tab === "sources" && <SourcesPanel />}
        {tab === "search" && <SearchPanel />}
        {tab === "export" && !readings && <ExportPanel />}
      </main>
      <TranscriptView />
    </div>
  );
}
