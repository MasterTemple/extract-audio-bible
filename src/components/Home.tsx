import { useEffect, useState } from "react";
import { BookOpen, FolderOpen, Plus, Trash2 } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { api, errorMessage, type BookOutline } from "@/lib/api";
import { useApp } from "@/lib/store";
import { BibleList } from "./BiblesPanel";

export function Home() {
  const { projects, bibles, refreshLists, openProject } = useApp();
  const [name, setName] = useState("");
  const [bibleId, setBibleId] = useState<string>("");
  const chosenBible = bibleId || bibles[0]?.id || "";
  const [books, setBooks] = useState<BookOutline[]>([]);
  const [book, setBook] = useState<string>("");

  useEffect(() => {
    if (!chosenBible) return;
    api.bibleOutline(chosenBible).then(setBooks).catch(() => setBooks([]));
  }, [chosenBible]);

  const create = async () => {
    try {
      const project = await api.createProject(name, chosenBible, book && book !== "all" ? Number(book) : null);
      await openProject(project.id);
    } catch (e) {
      toast.error(errorMessage(e));
    }
  };

  const remove = async (id: string, label: string) => {
    if (!confirm(`Delete the project “${label}”? Your audio and transcripts are not touched.`)) return;
    await api.deleteProject(id);
    await refreshLists();
  };

  return (
    <div className="min-h-screen bg-muted/30">
      <div className="mx-auto max-w-5xl space-y-6 p-8">
        <div className="flex items-center gap-3">
          <div className="flex size-10 items-center justify-center rounded-xl bg-primary text-primary-foreground">
            <BookOpen className="size-5" />
          </div>
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">Extract Audio Bible</h1>
            <p className="text-sm text-muted-foreground">
              Build a chapter-by-chapter audio Bible from the Scripture readings in your sermons.
            </p>
          </div>
        </div>

        <div className="grid gap-6 md:grid-cols-2">
          <Card>
            <CardHeader>
              <CardTitle>Projects</CardTitle>
              <CardDescription>Each project is one audio Bible in one translation.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-2">
              {projects.length === 0 && <p className="text-sm text-muted-foreground">No projects yet.</p>}
              {projects.map((p) => (
                <div key={p.id} className="flex items-center gap-2 rounded-lg border bg-background p-2 pl-3">
                  <div className="min-w-0 flex-1">
                    <div className="truncate font-medium">{p.name}</div>
                    <div className="text-xs text-muted-foreground">
                      {bibles.find((b) => b.id === p.bibleId)?.abbreviation || bibles.find((b) => b.id === p.bibleId)?.name || "Missing translation"} · {p.sourceCount}{" "}
                      sermons · {p.selectionCount} verses chosen
                    </div>
                  </div>
                  <Button size="sm" onClick={() => openProject(p.id).catch((e) => toast.error(errorMessage(e)))}>
                    <FolderOpen /> Open
                  </Button>
                  <Button size="icon-sm" variant="ghost" onClick={() => remove(p.id, p.name)}>
                    <Trash2 />
                  </Button>
                </div>
              ))}
            </CardContent>
          </Card>

          <Card>
            <CardHeader>
              <CardTitle>New project</CardTitle>
              <CardDescription>Choose the translation your sermons read from, and the book they preach through.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="space-y-1.5">
                <Label htmlFor="pname">Name</Label>
                <Input id="pname" placeholder="Named after the book, if left empty" value={name} onChange={(e) => setName(e.target.value)} />
              </div>
              <div className="space-y-1.5">
                <Label>Translation</Label>
                {bibles.length === 0 ? (
                  <p className="text-sm text-muted-foreground">Import a translation below first.</p>
                ) : (
                  <Select value={chosenBible} onValueChange={setBibleId}>
                    <SelectTrigger className="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {bibles.map((b) => (
                        <SelectItem key={b.id} value={b.id}>
                          {b.name} {b.abbreviation && `(${b.abbreviation})`}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                )}
              </div>
              <div className="space-y-1.5">
                <Label>Book</Label>
                <Select value={book} onValueChange={setBook} disabled={books.length === 0}>
                  <SelectTrigger className="w-full">
                    <SelectValue placeholder="Choose a book" />
                  </SelectTrigger>
                  <SelectContent>
                    {books.map((b) => (
                      <SelectItem key={b.id} value={String(b.id)}>
                        {b.name}
                      </SelectItem>
                    ))}
                    <SelectItem value="all">The whole Bible</SelectItem>
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  Only this book is searched for in the sermons. You can change it later.
                </p>
              </div>
              <Button disabled={!chosenBible || !book} onClick={create}>
                <Plus /> Create project
              </Button>
            </CardContent>
          </Card>
        </div>

        <BibleList />
      </div>
    </div>
  );
}
