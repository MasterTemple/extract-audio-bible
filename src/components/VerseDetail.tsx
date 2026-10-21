import { useState } from "react";
import { ChevronDown, ChevronUp, Loader2, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { partsOf, useApp } from "@/lib/store";
import { bookName, verseKey, versesNeeded } from "@/lib/refs";
import { MatchCard } from "./MatchCard";
import { ClipEditor } from "./ClipEditor";

/** Readings shown at first, and more each time more are asked for (a verse can have thousands) */
const PAGE = 50;

export function VerseDetail() {
  const { project, outline, book, chapter, verse, bookText, matchesByVerse, loadedChapters, setVerse } = useApp();
  const b = outline.find((x) => x.id === book);
  const [shown, setShown] = useState({ key: "", count: PAGE });
  if (!project || !b || !chapter || !verse) return null;
  const key = verseKey(b.id, chapter, verse);
  const text = bookText[b.id]?.[chapter - 1];
  const sel = project.selections[key];
  const chosen = sel ? partsOf(sel).map((p) => p.matchId) : [];
  // Where each plays among the chosen readings, if chosen
  const part = (id: string) => (chosen.includes(id) ? chosen.indexOf(id) : null);
  const order = (id: string) => part(id) ?? Infinity;
  const matches = [...(matchesByVerse.get(key) ?? [])].sort((x, y) => order(x.id) - order(y.id) || y.score - x.score);
  const loaded = loadedChapters.has(`${b.id}.${chapter}`);
  const count = shown.key === key ? shown.count : PAGE;
  const verses = versesNeeded(b, chapter);
  const i = verses.indexOf(verse);

  return (
    <aside className="flex w-[40rem] shrink-0 flex-col border-l bg-muted/20">
      <div className="flex items-center gap-1 border-b px-3 py-2">
        <h3 className="font-semibold">
          {bookName(outline, b.id)} {chapter}:{verse}
        </h3>
        <span className="ml-1 text-xs text-muted-foreground">
          {matches.length} {matches.length === 1 ? "reading" : "readings"}
        </span>
        <div className="ml-auto flex">
          <Button variant="ghost" size="icon-sm" disabled={i <= 0} onClick={() => setVerse(verses[i - 1])} title="Previous verse">
            <ChevronUp />
          </Button>
          <Button variant="ghost" size="icon-sm" disabled={i >= verses.length - 1} onClick={() => setVerse(verses[i + 1])} title="Next verse">
            <ChevronDown />
          </Button>
          <Button variant="ghost" size="icon-sm" onClick={() => setVerse(null)}>
            <X />
          </Button>
        </div>
      </div>
      <ScrollArea className="min-h-0 flex-1">
        <div className="space-y-4 p-3">
          {text && (
            <div className="space-y-1 rounded-lg border bg-background p-3 text-sm leading-relaxed">
              {verse > 1 && text[verse - 2] && (
                <p className="text-muted-foreground">
                  <sup className="mr-1 font-semibold">{verse - 1}</sup>
                  {text[verse - 2]}
                </p>
              )}
              <p className="text-[15px] font-medium">
                <sup className="mr-1 font-semibold text-primary">{verse}</sup>
                {text[verse - 1]}
              </p>
              {text[verse] && (
                <p className="text-muted-foreground">
                  <sup className="mr-1 font-semibold">{verse + 1}</sup>
                  {text[verse]}
                </p>
              )}
            </div>
          )}

          {sel && <ClipEditor verseKey={key} selection={sel} verseText={text?.[verse - 1] ?? ""} />}

          <div className="space-y-2">
            <div className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Readings found</div>
            {!loaded && (
              <div className="flex justify-center p-6">
                <Loader2 className="animate-spin text-muted-foreground" />
              </div>
            )}
            {loaded && matches.length === 0 && (
              <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
                This verse wasn't found in any sermon. Lowering the minimum match on the Sermons tab may find partial
                readings.
              </p>
            )}
            {matches.slice(0, count).map((m) => (
              <MatchCard key={m.id} match={m} verseText={text?.[verse - 1] ?? ""} part={part(m.id)} parts={chosen.length} />
            ))}
            {matches.length > count && (
              <Button variant="outline" className="w-full" onClick={() => setShown({ key, count: count + PAGE })}>
                Show {Math.min(PAGE, matches.length - count).toLocaleString()} more ({(matches.length - count).toLocaleString()} not shown)
              </Button>
            )}
          </div>
        </div>
      </ScrollArea>
    </aside>
  );
}
