import { useEffect, useState } from "react";
import { BookPlus, Loader2, Play, Square } from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import { Label } from "@/components/ui/label";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { api, errorMessage, type Match, type Said } from "@/lib/api";
import { useApp } from "@/lib/store";
import { useWords } from "@/lib/hooks";
import { play, usePlayer } from "@/lib/player";
import { audioPathOf, bookName, formatTime, kindLabel, sourceOf, sourceTitle, verseKey } from "@/lib/refs";
import { HeardText, kindClass } from "./MatchCard";
import { ReferenceInput } from "./ReferenceInput";

/** Words of the transcript shown around what was found, to widen it to */
const AROUND = 40;

type How = "add" | "choose" | "join";

/**
Makes a verse's reading of words said in a sermon (a search's place): the verse, and the words (from
the place found, widened or narrowed a word at a time), compared with the verse; then added, chosen
for the verse, or joined to its reading (a verse read in pieces)
*/
export function AddReading({ said }: { said: Said }) {
  const { project, outline, book, chapter, verse, addReading, go, loadBook } = useApp();
  const player = usePlayer();
  const [open, setOpen] = useState(false);
  const [reference, setReference] = useState("");
  const [range, setRange] = useState<[number, number]>([said.wordStart, said.wordEnd]);
  const [how, setHow] = useState<How>("choose");
  const [made, setMade] = useState<{ match: Match; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [working, setWorking] = useState(false);
  const source = project ? sourceOf(project, said.sourceId) : undefined;
  const words = useWords(open ? source?.transcriptPath : undefined, Math.max(0, said.wordStart - AROUND), said.wordEnd + AROUND);

  // Starts at the verse last open, and the words found
  const start = (o: boolean) => {
    if (o) {
      setReference(book && chapter && verse ? `${bookName(outline, book)} ${chapter}:${verse}` : "");
      setRange([said.wordStart, said.wordEnd]);
      setMade(null);
      setError(null);
    }
    setOpen(o);
  };

  // Made again as the verse or the words change
  useEffect(() => {
    if (!open || !project) return;
    let live = true;
    const timer = setTimeout(async () => {
      try {
        const found = await api.referenceVerse(reference);
        if (!found) {
          if (live) {
            setMade(null);
            setError(reference.trim() ? "Choose one verse (John 3:16)" : null);
          }
          return;
        }
        const [b, c, v] = found;
        const match = await api.readingFromSaid(project.id, said.sourceId, range[0], range[1], b, c, v);
        const text = (await loadBook(b))?.[c - 1]?.[v - 1] ?? "";
        if (live) {
          setMade({ match, text });
          setError(null);
        }
      } catch (e) {
        if (live) {
          setMade(null);
          setError(errorMessage(e));
        }
      }
    }, 150);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [open, reference, range]);

  if (!project || !source) return null;
  const m = made?.match;
  const chosen = m ? project.selections[verseKey(m.book, m.chapter, m.verse)] : undefined;
  const outside = m && !(project.book === null || project.book === m.book);

  // A word clicked moves the nearer end of the words taken to it
  const pickWord = (i: number) =>
    setRange(([s, e]) => (i < s ? [i, e] : i > e ? [s, i] : i - s <= e - i ? [i, e] : [s, i]));

  const clipId = m ? `added:${m.id}` : "";
  const listen = () =>
    m && play(clipId, () => api.renderClip({ audioPath: audioPathOf(project, source), start: m.start, end: m.end }), m.start);

  const add = () => {
    if (!m) return;
    setWorking(true);
    addReading(m, how === "join" && !chosen ? "choose" : how);
    const where = `${bookName(outline, m.book)} ${m.chapter}:${m.verse}`;
    toast.success(how === "add" ? `Added a reading of ${where}` : how === "choose" || !chosen ? `Chose it for ${where}` : `Joined it to ${where}'s reading`, {
      action: { label: "Show the verse", onClick: () => go(m.book, m.chapter, m.verse) },
    });
    setWorking(false);
    setOpen(false);
  };

  return (
    <Dialog open={open} onOpenChange={start}>
      <DialogTrigger asChild>
        <Button size="icon-xs" variant="ghost" title="Use this as a verse's reading">
          <BookPlus />
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Use as a verse's reading</DialogTitle>
          <DialogDescription>
            From {sourceTitle(project, source)} at {formatTime(said.start)}. Click a word to start or end there; it's compared
            with the verse as a search would.
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label>Verse</Label>
            <ReferenceInput
              value={reference}
              onChange={setReference}
              placeholder="John 3:16"
              has={(b) => project.book === null || project.book === b}
            />
          </div>
          <div className="max-h-48 overflow-y-auto rounded-md border bg-muted/30 p-2 text-[13px] leading-relaxed">
            {words ? (
              words.words.map((w, k) => {
                const i = words.from + k;
                const inside = i >= range[0] && i <= range[1];
                return (
                  <span key={i}>
                    <span
                      onClick={() => pickWord(i)}
                      title={formatTime(w.start, true)}
                      className={cn(
                        "cursor-pointer rounded-sm px-px hover:bg-muted",
                        inside ? "bg-primary/15 text-foreground" : "text-muted-foreground/70",
                        m?.extraWords.includes(i) && "text-amber-600 italic dark:text-amber-400",
                      )}
                    >
                      {w.text}
                    </span>{" "}
                  </span>
                );
              })
            ) : (
              <Loader2 className="size-4 animate-spin text-muted-foreground" />
            )}
          </div>
          {error && <p className="text-sm text-destructive">{error}</p>}
          {m && (
            <div className="space-y-1.5 rounded-md border p-2">
              <div className="flex items-center gap-2 text-xs">
                <span className={cn("rounded px-1.5 py-0.5 font-medium", kindClass[m.kind])}>
                  {kindLabel[m.kind]} · {Math.round(m.coverage * 100)}% of the verse
                </span>
                <span className="text-muted-foreground tabular-nums">
                  {formatTime(m.start)}–{formatTime(m.end)}
                </span>
                {outside && <span className="text-amber-600 dark:text-amber-400">Not this project's book</span>}
                <Button size="xs" variant="ghost" className="ml-auto" onClick={listen}>
                  {player.loading === clipId ? <Loader2 className="animate-spin" /> : player.id === clipId ? <Square /> : <Play />} Listen
                </Button>
              </div>
              {made.text && <HeardText text={made.text} heard={m.verseWordsHeard} />}
            </div>
          )}
          <ToggleGroup type="single" variant="outline" size="sm" value={how} onValueChange={(v) => v && setHow(v as How)}>
            <ToggleGroupItem value="choose">Choose it for the verse</ToggleGroupItem>
            <ToggleGroupItem value="join" title="Played with the verse's reading, in the order said: for a verse read in pieces">
              Join it to the verse's reading
            </ToggleGroupItem>
            <ToggleGroupItem value="add">Only add it</ToggleGroupItem>
          </ToggleGroup>
        </div>
        <DialogFooter>
          <Button disabled={!m || working} onClick={add}>
            {how === "add" ? "Add the reading" : how === "join" && chosen ? "Add and join" : "Add and choose"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
