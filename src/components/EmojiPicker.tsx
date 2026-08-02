import { Search, Smile, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  EMOJI_CATEGORIES,
  loadRecentEmojis,
  pushRecentEmoji,
  searchEmojis,
  type EmojiCategoryId,
} from "@/lib/emoji-data";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";
import { Input } from "./ui/input";

type Props = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onPick: (emoji: string) => void;
  disabled?: boolean;
  className?: string;
  /** Icon button size classes */
  buttonClassName?: string;
};

export function EmojiPicker({
  open,
  onOpenChange,
  onPick,
  disabled,
  className,
  buttonClassName,
}: Props) {
  const rootRef = useRef<HTMLDivElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<EmojiCategoryId>("smileys");
  const [recent, setRecent] = useState<string[]>([]);

  useEffect(() => {
    if (!open) return;
    setRecent(loadRecentEmojis());
    setQuery("");
    const t = window.setTimeout(() => searchRef.current?.focus(), 30);
    return () => window.clearTimeout(t);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) onOpenChange(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        onOpenChange(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open, onOpenChange]);

  const results = useMemo(() => (query.trim() ? searchEmojis(query) : null), [query]);

  const activeItems = useMemo(() => {
    if (results) return results;
    if (cat === "recent") {
      return recent.map((e) => ({ e, n: "recent" }));
    }
    return EMOJI_CATEGORIES.find((c) => c.id === cat)?.items ?? [];
  }, [results, cat, recent]);

  const handlePick = (emoji: string) => {
    setRecent(pushRecentEmoji(emoji));
    onPick(emoji);
  };

  return (
    <div ref={rootRef} className={cn("relative", className)}>
      <Button
        type="button"
        size="icon"
        variant={open ? "default" : "secondary"}
        disabled={disabled}
        title="Emoji"
        aria-label="Insert emoji"
        aria-expanded={open}
        aria-haspopup="dialog"
        className={cn("h-9 w-9 sm:h-10 sm:w-10", buttonClassName)}
        onClick={() => onOpenChange(!open)}
      >
        <Smile className="h-4 w-4" />
      </Button>

      {open && (
        <div
          role="dialog"
          aria-label="Emoji picker"
          className="absolute bottom-[calc(100%+0.5rem)] left-0 z-50 flex h-[min(22rem,55dvh)] w-[min(22rem,calc(100vw-1.5rem))] flex-col overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] shadow-[var(--shadow-soft)]"
        >
          <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-2 py-2">
            <div className="relative min-w-0 flex-1">
              <Search className="pointer-events-none absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-[var(--color-subtle)]" />
              <Input
                ref={searchRef}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search emoji…"
                className="h-8 pl-8 text-xs"
                aria-label="Search emoji"
              />
            </div>
            <Button
              type="button"
              size="icon"
              variant="ghost"
              className="h-8 w-8 shrink-0"
              aria-label="Close emoji picker"
              onClick={() => onOpenChange(false)}
            >
              <X className="h-3.5 w-3.5" />
            </Button>
          </div>

          {!query.trim() && (
            <div className="flex shrink-0 gap-0.5 overflow-x-auto scroll-hide border-b border-[var(--color-border)] px-1.5 py-1.5">
              <button
                type="button"
                title="Recent"
                aria-label="Recent"
                className={cn(
                  "flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-base transition-colors",
                  cat === "recent"
                    ? "bg-[var(--color-elevated)]"
                    : "hover:bg-[var(--color-elevated)]",
                )}
                onClick={() => setCat("recent")}
              >
                🕒
              </button>
              {EMOJI_CATEGORIES.map((c) => (
                <button
                  key={c.id}
                  type="button"
                  title={c.label}
                  aria-label={c.label}
                  className={cn(
                    "flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-sm)] text-base transition-colors",
                    cat === c.id
                      ? "bg-[var(--color-elevated)]"
                      : "hover:bg-[var(--color-elevated)]",
                  )}
                  onClick={() => setCat(c.id)}
                >
                  {c.icon}
                </button>
              ))}
            </div>
          )}

          <div className="scroll-panel min-h-0 flex-1 p-2">
            <div className="mb-1.5 px-0.5 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
              {query.trim()
                ? results && results.length
                  ? `Results · ${results.length}`
                  : "No matches"
                : cat === "recent"
                  ? recent.length
                    ? "Recent"
                    : "No recent emoji"
                  : EMOJI_CATEGORIES.find((c) => c.id === cat)?.label}
            </div>
            {activeItems.length === 0 ? (
              <p className="px-1 py-6 text-center text-xs text-[var(--color-muted)]">
                {cat === "recent" && !query.trim()
                  ? "Picked emoji show up here."
                  : "Try another search."}
              </p>
            ) : (
              <div className="grid grid-cols-8 gap-0.5 sm:grid-cols-9">
                {activeItems.map((item) => (
                  <button
                    key={`${item.e}-${item.n}`}
                    type="button"
                    title={item.n}
                    aria-label={item.n}
                    className="flex h-9 w-full items-center justify-center rounded-[var(--radius-sm)] text-xl leading-none transition-colors hover:bg-[var(--color-elevated)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-ring)] active:scale-95"
                    onClick={() => handlePick(item.e)}
                  >
                    <span className="select-none">{item.e}</span>
                  </button>
                ))}
              </div>
            )}
          </div>
        </div>
      )}
    </div>
  );
}
