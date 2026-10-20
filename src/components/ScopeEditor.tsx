import { useEffect, useMemo, useState } from "react";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";
import { api, errorMessage, type BibleSummary, type BookOutline, type BookScope, type Genre, type Testament } from "@/lib/api";
import { MultiSelect, type Option } from "./MultiSelect";

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

/** Which translations to search: at least one */
export function TranslationPicker({ bibles, value, onChange }: { bibles: BibleSummary[]; value: string[]; onChange: (ids: string[]) => void }) {
  const options = bibles.map((b) => ({ value: b.id, label: b.abbreviation || b.name, hint: b.abbreviation ? b.name : undefined }));
  return (
    <MultiSelect
      options={options}
      value={value}
      onChange={(ids) => ids.length > 0 && onChange(ids)}
      placeholder="Choose translations"
      searchPlaceholder="Search translations…"
    />
  );
}

const TESTAMENTS: [Testament, string][] = [
  ["old", "Old Testament"],
  ["new", "New Testament"],
];

/** A scope's included (or excluded) testaments, genres, and books, as one list of choices */
function chosen(scope: BookScope, exclude: boolean): string[] {
  const [t, g, b] = exclude ? [scope.excludeTestaments, scope.excludeGenres, scope.excludeBooks] : [scope.testaments, scope.genres, scope.books];
  return [...(t ?? []).map((x) => `t:${x}`), ...(g ?? []).map((x) => `g:${x}`), ...(b ?? []).map((x) => `b:${x}`)];
}

/** A scope with its included (or excluded) choices replaced */
function withChosen(scope: BookScope, exclude: boolean, values: string[]): BookScope {
  const of = (prefix: string) => values.filter((v) => v.startsWith(prefix)).map((v) => v.slice(2));
  const keys = exclude ? (["excludeTestaments", "excludeGenres", "excludeBooks"] as const) : (["testaments", "genres", "books"] as const);
  const out: BookScope = { ...scope, [keys[0]]: of("t:") as Testament[], [keys[1]]: of("g:"), [keys[2]]: of("b:") };
  for (const key of keys) if (!out[key]?.length) delete out[key];
  return out;
}

/** Which books to search, by testament, genre, and book, as topos filters them */
export function ScopeEditor({ scope, onChange, books }: { scope: BookScope; onChange: (s: BookScope) => void; books: BookOutline[] }) {
  const genres = useGenres();
  const [searched, setSearched] = useState<{ books: number[] | null; error: string | null } | null>(null);

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

  const options = useMemo<Option[]>(
    () => [
      ...TESTAMENTS.map(([t, label]) => ({ value: `t:${t}`, label, group: "Testaments" })),
      ...genres.map((g) => ({ value: `g:${g.name}`, label: g.name, group: "Genres", hint: `${g.books.length} books` })),
      ...books.map((b) => ({ value: `b:${b.name}`, label: b.name, group: "Books" })),
    ],
    [genres, books],
  );
  const only = chosen(scope, false);
  const leftOut = chosen(scope, true);
  // Choosing something in one list takes it out of the other
  const set = (exclude: boolean, values: string[]) => {
    const other = (exclude ? only : leftOut).filter((v) => !values.includes(v));
    onChange(withChosen(withChosen(scope, exclude, values), !exclude, other));
  };
  const names = (ids: number[]) => ids.map((id) => books.find((b) => b.id === id)?.name ?? `Book ${id}`);

  return (
    <div className="space-y-3">
      <div className="space-y-1.5">
        <Label className="text-xs text-muted-foreground">Only</Label>
        <MultiSelect
          options={options}
          value={only}
          onChange={(v) => set(false, v)}
          placeholder="Every book"
          searchPlaceholder="Search testaments, genres, and books…"
        />
      </div>
      <div className="space-y-1.5">
        <Label className="text-xs text-muted-foreground">Leave out</Label>
        <MultiSelect
          options={options}
          value={leftOut}
          onChange={(v) => set(true, v)}
          placeholder="Nothing"
          searchPlaceholder="Search testaments, genres, and books…"
          badgeClassName={() => "bg-red-100 text-red-800 dark:bg-red-950 dark:text-red-300"}
        />
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
        Testaments chosen limit the search; genres and books chosen add up within them; whatever is left out stays out.
      </p>
    </div>
  );
}
