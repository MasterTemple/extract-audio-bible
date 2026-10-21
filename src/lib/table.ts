/**
 * Tables read from CSV, TSV, or JSON, as records of text by field: what an importer shows and
 * lets its columns be matched to what they are.
 */

/** A table: its fields in the order first seen, and its records */
export interface Table {
  fields: string[];
  rows: Record<string, string>[];
}

/** Where a JSON file's records can be: `""` for the whole file, else a dotted path in it */
export interface RecordsAt {
  path: string;
  /** How many records are there */
  count: number;
  /** Whether they're keyed (an object of records), the key read as the `(key)` field */
  keyed: boolean;
}

/** The field a keyed record's key is read as */
export const KEY_FIELD = "(key)";
/** The field a record that's only a value (not an object) is read as */
export const VALUE_FIELD = "(value)";

/** A CSV or TSV file's rows, its first the header; the separator found from the header (comma, tab, or semicolon) */
export function parseDelimited(text: string): Table {
  text = text.replace(/^﻿/, "");
  const header = text.slice(0, text.search(/\r?\n|$/));
  const separator = [",", "\t", ";"].map((s) => [s, header.split(s).length] as const).sort((a, b) => b[1] - a[1])[0][0];
  const lines: string[][] = [];
  let line: string[] = [];
  let cell = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        cell += '"';
        i++;
      } else if (c === '"') quoted = false;
      else cell += c;
    } else if (c === '"' && cell === "") quoted = true;
    else if (c === separator) {
      line.push(cell);
      cell = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      line.push(cell);
      lines.push(line);
      line = [];
      cell = "";
    } else cell += c;
  }
  if (cell !== "" || line.length > 0) {
    line.push(cell);
    lines.push(line);
  }
  const [head = [], ...body] = lines.filter((l) => l.some((c) => c.trim() !== ""));
  // Each field named, and named once (a second `title` is `title (2)`)
  const fields: string[] = [];
  head.forEach((name, i) => {
    let field = name.trim() || `Column ${i + 1}`;
    for (let n = 2; fields.includes(field); n++) field = `${name.trim() || `Column ${i + 1}`} (${n})`;
    fields.push(field);
  });
  const rows = body.map((cells) => Object.fromEntries(fields.map((f, i) => [f, (cells[i] ?? "").trim()])));
  return { fields, rows };
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const isRecord = (v: unknown) => isObject(v) || Array.isArray(v);

/** What's at a dotted path in JSON (`""` is all of it) */
function at(json: unknown, path: string): unknown {
  return path === "" ? json : path.split(".").reduce<unknown>((v, k) => (isObject(v) ? v[k] : Array.isArray(v) ? v[Number(k)] : undefined), json);
}

/**
 * Where a JSON file's records can be, most records first (then lists, then the furthest in): the file itself if it's an array, or an
 * object of objects (keyed records), and arrays or objects of objects inside it, a few levels down
 */
export function recordsIn(json: unknown): RecordsAt[] {
  const found: RecordsAt[] = [];
  const look = (v: unknown, path: string, depth: number) => {
    if (Array.isArray(v)) {
      if (v.length > 0 && (path === "" || v.some(isRecord))) found.push({ path, count: v.length, keyed: false });
      return;
    }
    if (!isObject(v)) return;
    const values = Object.values(v);
    // Keyed records: an object most of whose values are objects (of more than one entry)
    if (values.length > 1 && values.filter(isObject).length >= values.length * 0.8) found.push({ path, count: values.length, keyed: true });
    if (depth < 3) for (const [k, inner] of Object.entries(v)) if (isRecord(inner)) look(inner, path ? `${path}.${k}` : k, depth + 1);
  };
  look(json, "", 0);
  // A tie is more likely records in a list, or further in (an object around them is only a wrapper)
  const depth = (p: RecordsAt) => (p.path ? p.path.split(".").length : 0);
  return found.sort((a, b) => b.count - a.count || Number(a.keyed) - Number(b.keyed) || depth(b) - depth(a));
}

/**
 * A record's fields, nested ones by dotted path (`speaker.name`), as text. An array's values are
 * joined with "; " under one field (`tags`, or `speakers.name` for an array of objects)
 */
function flatten(value: unknown, path: string, out: Record<string, string[]>) {
  if (isObject(value)) {
    for (const [k, v] of Object.entries(value)) flatten(v, path ? `${path}.${k}` : k, out);
  } else if (Array.isArray(value)) {
    for (const v of value) flatten(v, path, out);
  } else if (value !== null && value !== undefined) {
    const text = String(value).trim();
    if (text) (out[path || VALUE_FIELD] ??= []).push(text);
  }
}

/** The records at `where` in a JSON file, as a table */
export function jsonTable(json: unknown, where: RecordsAt): Table {
  const records = at(json, where.path);
  const entries: [string | null, unknown][] =
    where.keyed && isObject(records) ? Object.entries(records) : Array.isArray(records) ? records.map((r) => [null, r]) : [];
  const fields = new Set<string>();
  const rows = entries.map(([key, record]) => {
    const parts: Record<string, string[]> = {};
    if (key !== null) parts[KEY_FIELD] = [key];
    flatten(record, "", parts);
    const row: Record<string, string> = {};
    for (const [field, values] of Object.entries(parts)) {
      fields.add(field);
      row[field] = values.join("; ");
    }
    return row;
  });
  return { fields: [...fields], rows };
}

/**
 * The field that's most likely each of `wanted` (by name), first those named exactly, then those
 * whose last part is (`speaker.name` for `speaker`), then those containing it; each field used once
 */
export function guessFields<K extends string>(fields: string[], wanted: Record<K, string[]>): Record<K, string> {
  const used = new Set<string>();
  const out = {} as Record<K, string>;
  const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9.]/g, "");
  for (const [key, names] of Object.entries(wanted) as [K, string[]][]) {
    const tests = [
      (f: string) => names.some((n) => norm(f) === norm(n)),
      (f: string) => names.some((n) => norm(f.split(".").pop() ?? f) === norm(n) || norm(f).endsWith(`.${norm(n)}`)),
      (f: string) => names.some((n) => norm(f).includes(norm(n))),
    ];
    out[key] = "";
    for (const test of tests) {
      const found = fields.find((f) => !used.has(f) && test(f));
      if (found) {
        out[key] = found;
        used.add(found);
        break;
      }
    }
  }
  return out;
}
