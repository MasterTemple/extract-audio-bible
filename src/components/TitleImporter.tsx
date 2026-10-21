import { useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Loader2, Tags } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import { api, errorMessage, type FileMetadata } from "@/lib/api";
import { useApp } from "@/lib/store";
import { fileIdOf } from "@/lib/refs";
import { guessFields, jsonTable, parseDelimited, recordsIn, type RecordsAt, type Table } from "@/lib/table";

/** What each column can be, and the names it's guessed from */
const ROLES = {
  id: { label: "Sermon id", names: ["fileId", "file_id", "id", "letschurch_id", "short_id", "shortId", "(key)", "key", "file", "filename"] },
  title: { label: "Title", names: ["title", "name", "sermon"] },
  passage: { label: "Passage", names: ["passage", "scripture", "reference", "text", "verses"] },
  speaker: { label: "Speaker", names: ["speaker", "preacher", "author", "pastor", "artist"] },
  series: { label: "Series", names: ["series", "album", "collection"] },
} as const;
type Role = keyof typeof ROLES;
const NONE = "__none";
/** Rows shown in the preview */
const PREVIEW = 30;

/** What's read from the file: its table, and for JSON, where else records are */
interface Loaded {
  name: string;
  table: Table;
  json?: { value: unknown; places: RecordsAt[]; at: RecordsAt };
}

/** The first value a field has, to show what it is */
const sample = (table: Table, field: string) => table.rows.find((r) => r[field])?.[field] ?? "";

/**
Imports sermons' titles, passages, speakers, and series from a table (CSV, TSV, or JSON: an array
of records, or keyed records, nested fields by dotted path), each column matched to what it is
*/
export function TitleImporter() {
  const project = useApp((s) => s.project);
  const setFileMetadata = useApp((s) => s.setFileMetadata);
  const [loaded, setLoaded] = useState<Loaded | null>(null);
  const [reading, setReading] = useState(false);
  const [columns, setColumns] = useState<Record<Role, string>>({ id: "", title: "", passage: "", speaker: "", series: "" });

  const guess = (table: Table) => setColumns(guessFields(table.fields, Object.fromEntries(Object.entries(ROLES).map(([k, r]) => [k, [...r.names]])) as Record<Role, string[]>));

  const pick = async () => {
    const path = await open({ filters: [{ name: "Table of sermons (CSV, TSV, JSON)", extensions: ["csv", "tsv", "txt", "json"] }] });
    if (typeof path !== "string") return;
    setReading(true);
    try {
      const text = await api.readTextFile(path);
      const name = path.split(/[\\/]/).pop() ?? path;
      let next: Loaded;
      if (/^\s*[[{]/.test(text)) {
        const value: unknown = JSON.parse(text);
        const places = recordsIn(value);
        if (places.length === 0) throw new Error(`${name}: no records found (an array of them, or an object of them by id)`);
        next = { name, table: jsonTable(value, places[0]), json: { value, places, at: places[0] } };
      } else next = { name, table: parseDelimited(text) };
      if (next.table.rows.length === 0) throw new Error(`${name}: no rows`);
      setLoaded(next);
      guess(next.table);
    } catch (e) {
      toast.error(errorMessage(e));
    } finally {
      setReading(false);
    }
  };

  const recordsAt = (path: string) => {
    if (!loaded?.json) return;
    const at = loaded.json.places.find((p) => p.path === path)!;
    const table = jsonTable(loaded.json.value, at);
    setLoaded({ ...loaded, table, json: { ...loaded.json, at } });
    guess(table);
  };

  // What each sermon is given (empty cells leave what's known as it is), and how many are in the project
  const given = useMemo(() => {
    const out: Record<string, FileMetadata> = {};
    if (!loaded || !columns.id) return out;
    for (const row of loaded.table.rows) {
      const id = row[columns.id]?.trim();
      if (!id) continue;
      const info: FileMetadata = {};
      for (const role of ["title", "passage", "speaker", "series"] as const) {
        const value = columns[role] ? row[columns[role]]?.trim() : "";
        if (value) info[role] = value;
      }
      if (Object.keys(info).length > 0) out[id] = { ...out[id], ...info };
    }
    return out;
  }, [loaded, columns]);
  const ids = useMemo(() => new Set(project?.sources.map(fileIdOf) ?? []), [project?.sources]);
  const inProject = Object.keys(given).filter((id) => ids.has(id)).length;
  const roleOf = (field: string) => (Object.keys(ROLES) as Role[]).find((r) => columns[r] === field);

  const apply = () => {
    setFileMetadata(given);
    toast.success(`Imported ${Object.keys(given).length.toLocaleString()} sermons' information (${inProject.toLocaleString()} in this project)`);
    setLoaded(null);
  };

  if (!project) return null;
  const table = loaded?.table;

  return (
    <>
      <Button variant="outline" size="sm" disabled={reading} onClick={pick} title="Read titles, passages, speakers, and series from a CSV or JSON file">
        {reading ? <Loader2 className="animate-spin" /> : <Tags />} Import titles
      </Button>
      <Dialog open={loaded !== null} onOpenChange={(o) => !o && setLoaded(null)}>
        <DialogContent className="flex max-h-[90vh] flex-col sm:max-w-5xl">
          <DialogHeader>
            <DialogTitle>Import titles from {loaded?.name}</DialogTitle>
            <DialogDescription>
              Choose which column is each sermon's id (its file name, or its transcript's id) and what the others are. Empty
              cells leave what's known as it is.
            </DialogDescription>
          </DialogHeader>
          {table && (
            <div className="min-h-0 space-y-3 overflow-hidden">
              {loaded.json && loaded.json.places.length > 1 && (
                <div className="flex items-center gap-3">
                  <Label className="w-28 shrink-0">Records</Label>
                  <Select value={loaded.json.at.path} onValueChange={recordsAt}>
                    <SelectTrigger size="sm" className="w-80">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      {loaded.json.places.map((p) => (
                        <SelectItem key={p.path} value={p.path}>
                          {p.path || "The whole file"} · {p.count.toLocaleString()} {p.keyed ? "by key" : "in a list"}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                </div>
              )}
              <div className="grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-x-4 gap-y-2">
                {(Object.keys(ROLES) as Role[]).map((role) => (
                  <div key={role} className="space-y-1">
                    <Label className="text-xs">
                      {ROLES[role].label}
                      {role === "id" && <span className="text-destructive">*</span>}
                    </Label>
                    <Select value={columns[role] || NONE} onValueChange={(v) => setColumns((c) => ({ ...c, [role]: v === NONE ? "" : v }))}>
                      <SelectTrigger size="sm" className="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent>
                        <SelectItem value={NONE}>{role === "id" ? "Choose a column" : "Not imported"}</SelectItem>
                        {table.fields.map((f) => (
                          <SelectItem key={f} value={f}>
                            <span className="font-mono text-xs">{f}</span>
                            <span className="max-w-48 truncate text-xs text-muted-foreground">{sample(table, f)}</span>
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                  </div>
                ))}
              </div>
              <p className="text-sm">
                {table.rows.length.toLocaleString()} rows, {table.fields.length} columns.{" "}
                {columns.id ? (
                  <>
                    <span className="font-medium">{Object.keys(given).length.toLocaleString()}</span> sermons given information,{" "}
                    <span className={cn("font-medium", inProject === 0 && "text-destructive")}>{inProject.toLocaleString()}</span> of them in this
                    project
                    {inProject === 0 && " (is the id column the sermons' ids?)"}.
                  </>
                ) : (
                  <span className="text-destructive">Choose the column with each sermon's id.</span>
                )}
              </p>
              <div className="max-h-[45vh] overflow-auto rounded-md border">
                <table className="w-max min-w-full text-xs">
                  <thead className="sticky top-0 bg-muted">
                    <tr>
                      <th className="px-2 py-1.5 text-left font-medium text-muted-foreground" title="Whether it's a sermon in this project">
                        Here
                      </th>
                      {table.fields.map((f) => {
                        const role = roleOf(f);
                        return (
                          <th key={f} className={cn("px-2 py-1.5 text-left align-bottom font-mono font-medium", role && "bg-primary/10")}>
                            {role && <div className="mb-0.5 font-sans text-[10px] text-primary uppercase">{ROLES[role].label}</div>}
                            {f}
                          </th>
                        );
                      })}
                    </tr>
                  </thead>
                  <tbody>
                    {table.rows.slice(0, PREVIEW).map((row, i) => (
                      <tr key={i} className="border-t">
                        <td className="px-2 py-1 text-center">{columns.id && ids.has(row[columns.id]?.trim() ?? "") ? "✓" : ""}</td>
                        {table.fields.map((f) => (
                          <td key={f} className={cn("max-w-64 truncate px-2 py-1", roleOf(f) && "bg-primary/5")} title={row[f]}>
                            {row[f]}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {table.rows.length > PREVIEW && (
                <p className="text-xs text-muted-foreground">The first {PREVIEW} of {table.rows.length.toLocaleString()} rows.</p>
              )}
            </div>
          )}
          <DialogFooter>
            <Button variant="ghost" onClick={() => setLoaded(null)}>
              Cancel
            </Button>
            <Button disabled={Object.keys(given).length === 0} onClick={apply}>
              Import {Object.keys(given).length.toLocaleString()} sermons' information
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
}
