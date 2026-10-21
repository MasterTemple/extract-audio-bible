import { useEffect, useRef, useState } from "react";
import { Search } from "lucide-react";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";
import { api, type ReferenceCompletion } from "@/lib/api";

/** Ways to finish it shown at once */
const SHOWN = 8;

/**
A box for a reference, offering ways to finish it as it's typed (topos's: `2 Co` → `2 Corinthians`,
`Romans 8` → `Romans 8:1`, …), only those `has` keeps. Arrow keys move, Enter or Tab takes one,
Escape closes
*/
export function ReferenceInput({
  value,
  onChange,
  has,
  placeholder,
  className,
}: {
  value: string;
  onChange: (value: string) => void;
  /** Whether a book (and chapter, if given) is worth offering */
  has: (book: number, chapter: number | null) => boolean;
  placeholder?: string;
  className?: string;
}) {
  const [completions, setCompletions] = useState<ReferenceCompletion[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!value.trim()) {
      setCompletions([]);
      return;
    }
    let live = true;
    api.completeReference(value, 40).then(
      (found) => {
        if (!live) return;
        const chapterOf = (c: ReferenceCompletion) => {
          const m = /\s(\d+)(?::\d+)?$/.exec(c.text);
          return m ? Number(m[1]) : null;
        };
        // Nothing to offer that's only what's typed already
        const kept = found.filter((c) => has(c.book, chapterOf(c)) && c.text !== value.trim());
        setCompletions(kept.slice(0, SHOWN));
        setActive(0);
      },
      () => live && setCompletions([]),
    );
    return () => {
      live = false;
    };
    // `has` changes with what's shown, which is read again once the reference changes
  }, [value]);

  const take = (c: ReferenceCompletion) => {
    onChange(c.text);
    input.current?.focus();
  };
  const showing = open && completions.length > 0;

  return (
    <div className={cn("relative", className)}>
      <Search className="absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
      <Input
        ref={input}
        className="h-8 pl-8 text-sm"
        placeholder={placeholder}
        value={value}
        role="combobox"
        aria-expanded={showing}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={(e) => {
          if (!showing) {
            if (e.key === "ArrowDown") setOpen(true);
            return;
          }
          if (e.key === "ArrowDown") setActive((a) => Math.min(completions.length - 1, a + 1));
          else if (e.key === "ArrowUp") setActive((a) => Math.max(0, a - 1));
          else if (e.key === "Enter" || e.key === "Tab") take(completions[active]);
          else if (e.key === "Escape") setOpen(false);
          else return;
          e.preventDefault();
        }}
      />
      {showing && (
        <div role="listbox" className="absolute top-full left-0 z-50 mt-1 w-full rounded-md border bg-popover p-1 text-popover-foreground shadow-md">
          {completions.map((c, i) => (
            <div
              key={c.text}
              role="option"
              aria-selected={i === active}
              onMouseEnter={() => setActive(i)}
              // Before the input's blur closes the list
              onMouseDown={(e) => {
                e.preventDefault();
                take(c);
              }}
              className={cn("cursor-pointer truncate rounded px-2 py-1 text-sm", i === active && "bg-accent text-accent-foreground")}
            >
              {c.label}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
