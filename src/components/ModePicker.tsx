import {
  Brain,
  Hammer,
  Sparkles,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useRef } from "react";
import { getModesWithCatalog, modeBadge } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import { cn } from "@/lib/utils";

const ICONS: Record<GrokModeId, LucideIcon> = {
  auto: Sparkles,
  fast: Zap,
  expert: Brain,
  heavy: Users,
  build: Hammer,
};

export function ModePicker() {
  const mode = useGrokHub((s) => s.mode);
  const open = useGrokHub((s) => s.modeMenuOpen);
  const setMode = useGrokHub((s) => s.setMode);
  const setModeMenuOpen = useGrokHub((s) => s.setModeMenuOpen);
  const catalog = useGrokHub((s) => s.modelCatalog);
  const ref = useRef<HTMLDivElement>(null);
  const modes = getModesWithCatalog(catalog);
  const active = modes.find((m) => m.id === mode) ?? modes[0]!;
  const ActiveIcon = ICONS[active.id];

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setModeMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setModeMenuOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, setModeMenuOpen]);

  return (
    <div className="relative" ref={ref}>
      <button
        type="button"
        onClick={() => setModeMenuOpen(!open)}
        className={cn(
          "flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 text-left transition-colors hover:border-[var(--color-border-strong)]",
          open && "border-[var(--color-border-strong)]",
        )}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Model mode: ${modeBadge(active.id, catalog)}`}
      >
        <ActiveIcon className="h-3.5 w-3.5 text-[var(--color-muted)]" />
        <span className="text-xs font-medium text-[var(--color-fg)]">{active.label}</span>
        {active.id === "build" && (
          <span className="rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]">
            Beta
          </span>
        )}
        <span className="hidden max-w-[7.5rem] truncate font-mono text-[10px] text-[var(--color-subtle)] sm:inline">
          {active.model}
        </span>
      </button>

      {open && (
        <div
          role="listbox"
          className="absolute right-0 top-[calc(100%+6px)] z-50 w-[min(100vw-2rem,340px)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] p-1.5 shadow-[var(--shadow-soft)]"
        >
          {modes.map((m) => {
            const Icon = ICONS[m.id];
            const selected = m.id === mode;
            return (
              <button
                key={m.id}
                type="button"
                role="option"
                aria-selected={selected}
                onClick={() => {
                  setMode(m.id);
                  setModeMenuOpen(false);
                }}
                className={cn(
                  "flex w-full items-start gap-3 rounded-[var(--radius-md)] px-2.5 py-2.5 text-left transition-colors",
                  selected
                    ? "bg-[var(--color-elevated)]"
                    : "hover:bg-[var(--color-elevated)]/70",
                )}
              >
                <Icon className="mt-0.5 h-4 w-4 shrink-0 text-[var(--color-muted)]" />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-[var(--color-fg)]">
                      {m.label}
                    </span>
                    {m.id === "build" && (
                      <span className="rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]">
                        Beta
                      </span>
                    )}
                    {selected && (
                      <span className="ml-auto text-[var(--color-muted)]">✓</span>
                    )}
                  </div>
                  <div className="text-xs text-[var(--color-muted)]">{m.subtitle}</div>
                  {m.id !== "auto" && (
                    <div className="mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]">
                      {m.modelId}
                    </div>
                  )}
                </div>
              </button>
            );
          })}
          {catalog.source === "live" && (
            <div className="border-t border-[var(--color-border)] px-2.5 py-1.5 text-[10px] text-[var(--color-subtle)]">
              Live · {catalog.essential.length} essential models
            </div>
          )}
        </div>
      )}
    </div>
  );
}
