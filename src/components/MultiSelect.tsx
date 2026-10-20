import { useMemo, useRef, useState } from "react";
import { Check, ChevronsUpDown, X } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { cn } from "@/lib/utils";

export interface Option {
  value: string;
  label: string;
  /** A heading it's listed under ("Genres") */
  group?: string;
  /** More, shown faintly after it */
  hint?: string;
}

/**
A combo box choosing several options: the chosen ones as badges (each removed with its ×), and a
list to search, grouped if the options are. Arrow keys move, Enter chooses or unchooses,
Backspace in an empty search removes the last chosen, Escape closes
*/
export function MultiSelect({
  options,
  value,
  onChange,
  placeholder = "Choose…",
  searchPlaceholder = "Search…",
  empty = "Nothing matches",
  badgeClassName,
  className,
  disabled,
}: {
  options: Option[];
  value: string[];
  onChange: (value: string[]) => void;
  placeholder?: string;
  searchPlaceholder?: string;
  empty?: string;
  /** Styles a chosen option's badge */
  badgeClassName?: (value: string) => string | undefined;
  className?: string;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const list = useRef<HTMLDivElement>(null);

  const label = (v: string) => options.find((o) => o.value === v)?.label ?? v;
  const shown = useMemo(() => {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return options.filter((o) => words.every((w) => `${o.label} ${o.group ?? ""} ${o.hint ?? ""}`.toLowerCase().includes(w)));
  }, [options, query]);

  const toggle = (v: string) => onChange(value.includes(v) ? value.filter((x) => x !== v) : [...value, v]);
  const move = (to: number) => {
    const next = Math.max(0, Math.min(shown.length - 1, to));
    setActive(next);
    list.current?.querySelector(`[data-index="${next}"]`)?.scrollIntoView({ block: "nearest" });
  };

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setQuery("");
          setActive(0);
        }
      }}
    >
      <PopoverTrigger asChild disabled={disabled}>
        <div
          role="combobox"
          aria-expanded={open}
          tabIndex={disabled ? -1 : 0}
          className={cn(
            "flex min-h-9 w-full cursor-pointer flex-wrap items-center gap-1 rounded-md border bg-background px-2 py-1 text-sm shadow-xs outline-none focus-visible:ring-2 focus-visible:ring-ring/50",
            disabled && "pointer-events-none opacity-50",
            className,
          )}
        >
          {value.length === 0 && <span className="px-1 text-muted-foreground">{placeholder}</span>}
          {value.map((v) => (
            <span
              key={v}
              className={cn("flex items-center gap-0.5 rounded bg-secondary py-0.5 pr-0.5 pl-1.5 text-xs text-secondary-foreground", badgeClassName?.(v))}
            >
              {label(v)}
              <button
                type="button"
                aria-label={`Remove ${label(v)}`}
                className="rounded p-0.5 hover:bg-foreground/10"
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  toggle(v);
                }}
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
          <ChevronsUpDown className="ml-auto size-4 shrink-0 text-muted-foreground" />
        </div>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-(--radix-popover-trigger-width) min-w-64 gap-0 p-0">
        <input
          autoFocus
          className="border-b bg-transparent px-3 py-2 text-sm outline-none placeholder:text-muted-foreground"
          placeholder={searchPlaceholder}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") move(active + 1);
            else if (e.key === "ArrowUp") move(active - 1);
            else if (e.key === "Enter" && shown[active]) toggle(shown[active].value);
            else if (e.key === "Backspace" && !query && value.length) onChange(value.slice(0, -1));
            else return;
            e.preventDefault();
          }}
        />
        <div ref={list} className="max-h-72 overflow-y-auto p-1" role="listbox" aria-multiselectable>
          {shown.length === 0 && <p className="px-2 py-3 text-center text-xs text-muted-foreground">{empty}</p>}
          {shown.map((o, i) => (
            <div key={o.value}>
              {o.group && o.group !== shown[i - 1]?.group && (
                <div className="px-2 pt-2 pb-1 text-[11px] font-medium text-muted-foreground">{o.group}</div>
              )}
              <div
                role="option"
                aria-selected={value.includes(o.value)}
                data-index={i}
                onMouseEnter={() => setActive(i)}
                onClick={() => toggle(o.value)}
                className={cn("flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm", i === active && "bg-accent text-accent-foreground")}
              >
                <Check className={cn("size-4 shrink-0", !value.includes(o.value) && "invisible")} />
                <span className="truncate">{o.label}</span>
                {o.hint && <span className="ml-auto truncate text-xs text-muted-foreground">{o.hint}</span>}
              </div>
            </div>
          ))}
        </div>
        {value.length > 0 && (
          <button className="border-t px-3 py-1.5 text-left text-xs text-muted-foreground hover:text-foreground" onClick={() => onChange([])}>
            Clear {value.length} chosen
          </button>
        )}
      </PopoverContent>
    </Popover>
  );
}
