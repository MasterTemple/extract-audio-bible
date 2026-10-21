import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { FileText, FolderOpen, Loader2, Pencil, Trash2, Upload } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { api, errorMessage, type BibleSummary } from "@/lib/api";
import { useApp } from "@/lib/store";
import { fileName } from "@/lib/refs";

const FORMATS = ["usfm", "sfm", "usx", "xml", "osis", "thml", "gbf", "json", "csv", "tsv", "txt"];

function ImportBibleDialog() {
  const refreshLists = useApp((s) => s.refreshLists);
  const [openDialog, setOpenDialog] = useState(false);
  const [paths, setPaths] = useState<string[]>([]);
  const [name, setName] = useState("");
  const [abbreviation, setAbbreviation] = useState("");
  const [language, setLanguage] = useState("");
  const [busy, setBusy] = useState(false);

  const pick = async (directory: boolean) => {
    const picked = await open({
      multiple: true,
      directory,
      filters: directory ? undefined : [{ name: "Bible text", extensions: FORMATS }],
    });
    if (!picked) return;
    setPaths(Array.isArray(picked) ? picked : [picked]);
  };

  const run = async () => {
    setBusy(true);
    try {
      const bible = await api.importBible(paths, name, abbreviation, language || null);
      toast.success(`Imported ${bible.name}: ${bible.bookCount} books, ${bible.verseCount.toLocaleString()} verses`);
      if (bible.warnings.length > 0) {
        toast.warning(`${bible.warnings.length} ${bible.warnings.length === 1 ? "thing was" : "things were"} left out`, {
          duration: 30_000,
          description: (
            <div className="max-h-40 overflow-auto text-xs">
              {bible.warnings.map((w) => (
                <div key={w}>{w}</div>
              ))}
            </div>
          ),
        });
      }
      await refreshLists();
      setOpenDialog(false);
      setPaths([]);
      setName("");
      setAbbreviation("");
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={openDialog} onOpenChange={setOpenDialog}>
      <DialogTrigger asChild>
        <Button size="sm">
          <Upload /> Import translation
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Import a translation</DialogTitle>
          <DialogDescription>
            The text the sermons read from. USFM, USX, OSIS, ThML, and GBF are read with logoxide (a file per book or
            the whole Bible in one). You can also bring your own text as JSON, CSV/TSV (book, chapter, verse, text),
            or plain lines like <code className="text-xs">John 3:16 For God so loved…</code>
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-4">
          <div className="flex gap-2">
            <Button variant="outline" onClick={() => pick(false)}>
              <FileText /> Choose files
            </Button>
            <Button variant="outline" onClick={() => pick(true)}>
              <FolderOpen /> Choose folder
            </Button>
          </div>
          {paths.length > 0 && (
            <div className="max-h-28 overflow-auto rounded-md border bg-muted/40 p-2 text-xs">
              {paths.map((p) => (
                <div key={p} className="truncate" title={p}>
                  {fileName(p)}
                </div>
              ))}
            </div>
          )}
          <div className="grid grid-cols-[1fr_8rem] gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="bname">Name</Label>
              <Input id="bname" placeholder="King James Version" value={name} onChange={(e) => setName(e.target.value)} />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="babbr">Abbreviation</Label>
              <Input id="babbr" placeholder="KJV" value={abbreviation} onChange={(e) => setAbbreviation(e.target.value)} />
            </div>
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="blang">Language (optional)</Label>
            <Input id="blang" placeholder="eng" value={language} onChange={(e) => setLanguage(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button disabled={paths.length === 0 || busy} onClick={run}>
            {busy && <Loader2 className="animate-spin" />} Import
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/** Renaming a translation: its name, abbreviation, and language, wherever it's shown */
function EditBibleDialog({ bible }: { bible: BibleSummary }) {
  const refreshLists = useApp((s) => s.refreshLists);
  const [openDialog, setOpenDialog] = useState(false);
  const [name, setName] = useState(bible.name);
  const [abbreviation, setAbbreviation] = useState(bible.abbreviation);
  const [language, setLanguage] = useState(bible.language ?? "");
  const [busy, setBusy] = useState(false);

  const start = (open: boolean) => {
    if (open) {
      setName(bible.name);
      setAbbreviation(bible.abbreviation);
      setLanguage(bible.language ?? "");
    }
    setOpenDialog(open);
  };
  const save = async () => {
    setBusy(true);
    try {
      await api.renameBible(bible.id, name, abbreviation, language || null);
      await refreshLists();
      setOpenDialog(false);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={openDialog} onOpenChange={start}>
      <DialogTrigger asChild>
        <Button size="icon-sm" variant="ghost" title="Rename">
          <Pencil />
        </Button>
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Rename {bible.name}</DialogTitle>
          <DialogDescription>Its name and abbreviation are what projects, readings, and exported files call it.</DialogDescription>
        </DialogHeader>
        <div className="grid gap-3 sm:grid-cols-[1fr_8rem_8rem]">
          <div className="space-y-1.5">
            <Label htmlFor="ename">Name</Label>
            <Input id="ename" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="eabbr">Abbreviation</Label>
            <Input id="eabbr" value={abbreviation} onChange={(e) => setAbbreviation(e.target.value)} />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="elang">Language</Label>
            <Input id="elang" placeholder="en" value={language} onChange={(e) => setLanguage(e.target.value)} />
          </div>
        </div>
        <DialogFooter>
          <Button disabled={!name.trim() || busy} onClick={save}>
            {busy && <Loader2 className="animate-spin" />} Save
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export function BibleList() {
  const { bibles, projects, refreshLists } = useApp();

  const remove = async (id: string, label: string) => {
    const using = projects.filter((p) => p.bibleId === id || p.translations.includes(id)).length;
    if (using > 0) {
      toast.error(`${label} is used by ${using} project${using > 1 ? "s" : ""}`);
      return;
    }
    if (!confirm(`Delete ${label}?`)) return;
    await api.deleteBible(id);
    await refreshLists();
  };

  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-4">
        <div className="space-y-1.5">
          <CardTitle>Translations</CardTitle>
          <CardDescription>Imported Bible texts, kept on this computer.</CardDescription>
        </div>
        <ImportBibleDialog />
      </CardHeader>
      <CardContent className="space-y-2">
        {bibles.length === 0 && <p className="text-sm text-muted-foreground">None yet.</p>}
        {bibles.map((b) => (
          <div key={b.id} className="flex items-center gap-3 rounded-lg border bg-background p-2 pl-3">
            <div className="min-w-0 flex-1">
              <div className="truncate font-medium">
                {b.name} {b.abbreviation && <span className="text-muted-foreground">({b.abbreviation})</span>}
              </div>
              <div className="text-xs text-muted-foreground">
                {b.bookCount} books · {b.verseCount.toLocaleString()} verses
                {b.language && ` · ${b.language}`}
              </div>
            </div>
            <Badge variant="secondary" className="uppercase">
              {b.format}
            </Badge>
            <EditBibleDialog bible={b} />
            <Button size="icon-sm" variant="ghost" title="Delete" onClick={() => remove(b.id, b.name)}>
              <Trash2 />
            </Button>
          </div>
        ))}
      </CardContent>
    </Card>
  );
}
