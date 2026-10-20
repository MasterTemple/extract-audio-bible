import { useEffect, useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { api, errorMessage, type BibleSummary, type BookOutline, type BookScope, type Genre, type Testament } from "@/lib/api";

let genreList: Promise<Genre[]> | null = null;
/** topos's genres, asked for once */
function useGenres() {
  const [genres, setGenres] = useState<Genre[]>([]);
  useEffect(() => {
    genreList ??= api.genres();
    genreList.then(setGenres).catch(() => (genreList = null));
  }, []);
  return genres;
}

/** Whether something is included, excluded, or neither */
type State = "include" | "exclude" | null;

/** A choice that cycles: not chosen → included → left out → not chosen */
function Chip({ label, state, onChange, title }: { label: string; state: State; onChange: (s: State) => void; title?: string }) {
  const next: Record<string, State> = { none: "include", include: "exclude", exclude: null };
  return (
    <button
      type="button"
      title={title ?? (state === "include" ? "Included: click to leave it out" : state === "exclude" ? "Left out: click to clear" : "Click to include")}
      onClick={() => onChange(next[state ?? "none"])}
      className={cn(
        "rounded-full border px-2.5 py-0.5 text-xs transition-colors",
        state === "include" && "border-primary bg-primary text-primary-foreground",
        state === "exclude" && "border-red-300 bg-red-50 text-red-700 line-through dark:border-red-800 dark:bg-red-950 dark:text-red-300",
        state === null && "hover:bg-muted",
      )}
    >
      {label}
    </button>
  );
}

/** Puts `value` in `include` or `exclude` (or neither), taking it out of the other */
function choose<T>(scope: BookScope, include: keyof BookScope, exclude: keyof BookScope, value: T, state: State): BookScope {
  const without = (list: unknown) => ((list as T[] | undefined) ?? []).filter((x) => x !== value);
  const out: BookScope = { ...scope, [include]: without(scope[include]), [exclude]: without(scope[exclude]) };
  if (state === "include") (out[include] as T[]).push(value);
  if (state === "exclude") (out[exclude] as T[]).push(value);
  for (const key of [include, exclude]) if (!(out[key] as T[]).length) delete out[key];
  return out;
}

function stateOf<T>(scope: BookScope, include: keyof BookScope, exclude: keyof BookScope, value: T): State {
  if ((scope[include] as T[] | undefined)?.includes(value)) return "include";
  if ((scope[exclude] as T[] | undefined)?.includes(value)) return "exclude";
  return null;
}

/** Which translations to search: at least one */
export function TranslationPicker({ bibles, value, onChange }: { bibles: BibleSummary[]; value: string[]; onChange: (ids: string[]) => void }) {
  return (
    <div className="grid gap-1.5 sm:grid-cols-2">
      {bibles.map((b) => {
        const checked = value.includes(b.id);
        return (
          <label key={b.id} className="flex cursor-pointer items-center gap-2 text-sm">
            <Checkbox
              checked={checked}
              disabled={checked && value.length === 1}
              onCheckedChange={(on) => onChange(on ? [...value, b.id] : value.filter((id) => id !== b.id))}
            />
            <span className="truncate">
              {b.name} {b.abbreviation && <span className="text-muted-foreground">({b.abbreviation})</span>}
            </span>
          </label>
        );
      })}
    </div>
  );
}

/** Which books to search, by testament, genre, and book, as topos filters them */
export function ScopeEditor({ scope, onChange, books }: { scope: BookScope; onChange: (s: BookScope) => void; books: BookOutline[] }) {
  const genres = useGenres();
  const [searched, setSearched] = useState<{ books: number[] | null; error: string | null } | null>(null);
  const [showBooks, setShowBooks] = useState(!!(scope.books?.length || scope.excludeBooks?.length));

  useEffect(() => {
    let live = true;
    api.scopeBooks(scope).then(
      (b) => live && setSearched({ books: b, error: null }),
      (e) => live && setSearched({ books: [], error: errorMessage(e) }),
    );
    return () => {
      live = false;
    };
  }, [scope]);

  const testaments: [Testament, string][] = [
    ["old", "Old Testament"],
    ["new", "New Testament"],
  ];
  const names = (ids: number[]) => ids.map((id) => books.find((b) => b.id === id)?.name ?? `Book ${id}`);

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <Label className="text-xs text-muted-foreground">Testaments</Label>
        <div className="flex flex-wrap gap-1.5">
          {testaments.map(([t, label]) => (
            <Chip
              key={t}
              label={label}
              state={stateOf(scope, "testaments", "excludeTestaments", t)}
              onChange={(s) => onChange(choose(scope, "testaments", "excludeTestaments", t, s))}
            />
          ))}
        </div>
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs text-muted-foreground">Genres</Label>
        <div className="flex flex-wrap gap-1.5">
          {genres.map((g) => (
            <Chip
              key={g.name}
              label={g.name}
              state={stateOf(scope, "genres", "excludeGenres", g.name)}
              onChange={(s) => onChange(choose(scope, "genres", "excludeGenres", g.name, s))}
            />
          ))}
        </div>
      </div>
      <div className="space-y-1.5">
        <button type="button" className="text-xs text-muted-foreground hover:text-foreground" onClick={() => setShowBooks(!showBooks)}>
          {showBooks ? "▾" : "▸"} Books
        </button>
        {showBooks && (
          <div className="flex flex-wrap gap-1">
            {books.map((b) => (
              <Chip
                key={b.id}
                label={b.name}
                state={stateOf(scope, "books", "excludeBooks", b.name)}
                onChange={(s) => onChange(choose(scope, "books", "excludeBooks", b.name, s))}
              />
            ))}
          </div>
        )}
      </div>
      <p className={cn("text-xs", searched?.error ? "text-destructive" : "text-muted-foreground")}>
        {!searched
          ? "…"
          : searched.error
            ? searched.error
            : searched.books === null
              ? "Searching every book."
              : `Searching ${searched.books.length} book${searched.books.length === 1 ? "" : "s"}: ${names(searched.books).slice(0, 8).join(", ")}${searched.books.length > 8 ? ", …" : ""}`}
      </p>
      <p className="text-[11px] text-muted-foreground">
        Click once to include, again to leave out. Included testaments limit the search; included genres and books add up within
        them; whatever is left out stays out.
      </p>
    </div>
  );
}
