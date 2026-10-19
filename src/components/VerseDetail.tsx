import { ChevronDown, ChevronUp, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useApp } from "@/lib/store";
import { bookName, verseKey, versesNeeded } from "@/lib/refs";
import { MatchCard } from "./MatchCard";
import { ClipEditor } from "./ClipEditor";

export function VerseDetail() {
  const { project, outline, book, chapter, verse, bookText, matchesByVerse, setVerse } = useApp();
  const b = outline.find((x) => x.id === book);
  if (!project || !b || !chapter || !verse) return null;
  const key = verseKey(b.id, chapter, verse);
  const text = bookText[b.id]?.[chapter - 1];
  const sel = project.selections[key];
  const matches = [...(matchesByVerse.get(key) ?? [])].sort(
    (x, y) => Number(y.id === sel?.matchId) - Number(x.id === sel?.matchId) || y.score - x.score,
  );
  const verses = versesNeeded(b, chapter);
  const i = verses.indexOf(verse);

  return (
    <aside className="flex w-[34rem] shrink-0 flex-col border-l bg-muted/20">
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

          {sel && <ClipEditor verseKey={key} selection={sel} />}

          <div className="space-y-2">
            <div className="text-xs font-medium tracking-wide text-muted-foreground uppercase">Readings found</div>
            {matches.length === 0 && (
              <p className="rounded-lg border border-dashed p-6 text-center text-sm text-muted-foreground">
                This verse wasn't found in any sermon. Lowering the minimum match on the Sermons tab may find partial
                readings.
              </p>
            )}
            {matches.map((m) => (
              <MatchCard key={m.id} match={m} verseText={text?.[verse - 1] ?? ""} selected={sel?.matchId === m.id} />
            ))}
          </div>
        </div>
      </ScrollArea>
    </aside>
  );
}
