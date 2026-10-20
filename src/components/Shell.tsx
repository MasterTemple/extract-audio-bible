import { useMemo } from "react";
import { ArrowLeft, BookOpen, Check, Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useApp, type Tab } from "@/lib/store";
import { versesNeeded } from "@/lib/refs";
import { SourcesPanel } from "./SourcesPanel";
import { VersesPanel } from "./VersesPanel";
import { ExportPanel } from "./ExportPanel";
import { TranscriptView } from "./TranscriptView";

export function Shell() {
  const { project, bibles, tab, setTab, saving, closeProject, outline } = useApp();
  const bible = bibles.find((b) => b.id === project?.bibleId);

  const progress = useMemo(() => {
    if (!project) return { done: 0, total: 0 };
    let total = 0;
    const books = outline.filter((b) => project.book === null || b.id === project.book);
    for (const b of books) for (let c = 1; c <= b.chapters.length; c++) total += versesNeeded(b, c).length;
    const done = Object.keys(project.selections).filter(
      (k) => project.book === null || k.startsWith(`${project.book}.`),
    ).length;
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
            {project.book !== null ? `${outline.find((b) => b.id === project.book)?.name ?? "Book"} · ` : ""}
            {bible?.abbreviation || bible?.name || "Translation"} · {progress.done.toLocaleString()} of {progress.total.toLocaleString()} verses
            chosen
          </div>
        </div>
        <Tabs value={tab} onValueChange={(v) => setTab(v as Tab)} className="mx-auto">
          <TabsList>
            <TabsTrigger value="verses">Verses</TabsTrigger>
            <TabsTrigger value="sources">
              Sermons <span className="text-muted-foreground">{project.sources.length}</span>
            </TabsTrigger>
            <TabsTrigger value="export">Export</TabsTrigger>
          </TabsList>
        </Tabs>
        <div className="flex w-40 items-center justify-end gap-1.5 text-xs text-muted-foreground">
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
        {tab === "verses" && <VersesPanel />}
        {tab === "sources" && <SourcesPanel />}
        {tab === "export" && <ExportPanel />}
      </main>
      <TranscriptView />
    </div>
  );
}
