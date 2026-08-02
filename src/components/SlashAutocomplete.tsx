import { filterSlashCommands, type SlashDef } from "@/lib/slash-commands";
import { cn } from "@/lib/utils";
import { useEffect, useMemo, useState } from "react";

export function SlashAutocomplete({
  draft,
  open,
  onPick,
  onClose,
}: {
  draft: string;
  open: boolean;
  onPick: (cmd: SlashDef) => void;
  onClose: () => void;
}) {
  const items = useMemo(() => filterSlashCommands(draft), [draft]);
  const [idx, setIdx] = useState(0);

  useEffect(() => {
    setIdx(0);
  }, [draft, open]);

  useEffect(() => {
    if (!open || !items.length) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        e.stopPropagation();
        setIdx((i) => (i + 1) % items.length);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        e.stopPropagation();
        setIdx((i) => (i - 1 + items.length) % items.length);
      } else if (e.key === "Tab" || (e.key === "Enter" && !e.shiftKey)) {
        // Tab always accepts; Enter only when we intercept (parent may send)
        if (e.key === "Tab") {
          e.preventDefault();
          e.stopPropagation();
          onPick(items[idx]!);
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, items, idx, onPick, onClose]);

  if (!open || !items.length) return null;

  return (
    <div
      className="absolute bottom-full left-0 z-40 mb-1 w-full max-w-md overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-panel)] shadow-[var(--shadow-soft)]"
      role="listbox"
      aria-label="Slash commands"
    >
      {items.map((it, i) => (
        <button
          key={it.cmd}
          type="button"
          role="option"
          aria-selected={i === idx}
          className={cn(
            "flex w-full items-center justify-between gap-3 px-3 py-2 text-left text-sm",
            i === idx
              ? "bg-[var(--color-elevated)] text-[var(--color-fg)]"
              : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/70 hover:text-[var(--color-fg)]",
          )}
          onMouseEnter={() => setIdx(i)}
          onClick={() => onPick(it)}
        >
          <span className="font-mono text-xs text-[var(--color-fg)]">{it.cmd}</span>
          <span className="truncate text-xs text-[var(--color-subtle)]">{it.hint}</span>
        </button>
      ))}
      <div className="border-t border-[var(--color-border)] px-3 py-1 text-[10px] text-[var(--color-subtle)]">
        Tab to accept · ↑↓ move · Esc dismiss
      </div>
    </div>
  );
}
