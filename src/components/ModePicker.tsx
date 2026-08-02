import {
  Brain,
  Crown,
  Hammer,
  Sparkles,
  Users,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { getModesWithCatalog, modeBadge } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId } from "@/lib/types";
import { cn } from "@/lib/utils";

const ICONS: Record<GrokModeId, LucideIcon> = {
  auto: Sparkles,
  fast: Zap,
  expert: Brain,
  heavy: Users,
  max: Crown,
  build: Hammer,
};

/**
 * Mode picker for the title bar. Menu is portaled to document.body so it is not
 * clipped by the frameless shell overflow / app-region drag.
 */
export function ModePicker() {
  const mode = useGrokHub((s) => s.mode);
  const open = useGrokHub((s) => s.modeMenuOpen);
  const setMode = useGrokHub((s) => s.setMode);
  const setModeMenuOpen = useGrokHub((s) => s.setModeMenuOpen);
  const catalog = useGrokHub((s) => s.modelCatalog);
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ top: number; right: number }>({ top: 0, right: 0 });
  const modes = getModesWithCatalog(catalog);
  const active = modes.find((m) => m.id === mode) ?? modes[0]!;
  const ActiveIcon = ICONS[active.id];

  const noDrag = { WebkitAppRegion: "no-drag" } as CSSProperties;

  useLayoutEffect(() => {
    if (!open || !btnRef.current) return;
    const update = () => {
      const r = btnRef.current!.getBoundingClientRect();
      setPos({
        top: r.bottom + 6,
        right: Math.max(8, window.innerWidth - r.right),
      });
    };
    update();
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      const t = e.target as Node;
      if (btnRef.current?.contains(t) || menuRef.current?.contains(t)) return;
      setModeMenuOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setModeMenuOpen(false);
        btnRef.current?.focus();
        return;
      }
      const ids = modes.map((m) => m.id);
      const cur = Math.max(0, ids.indexOf(mode));
      if (e.key === "ArrowDown" || e.key === "ArrowRight") {
        e.preventDefault();
        const next = ids[(cur + 1) % ids.length]!;
        setMode(next);
      } else if (e.key === "ArrowUp" || e.key === "ArrowLeft") {
        e.preventDefault();
        const next = ids[(cur - 1 + ids.length) % ids.length]!;
        setMode(next);
      } else if (e.key === "Home") {
        e.preventDefault();
        setMode(ids[0]!);
      } else if (e.key === "End") {
        e.preventDefault();
        setMode(ids[ids.length - 1]!);
      } else if (e.key === "Enter") {
        e.preventDefault();
        setModeMenuOpen(false);
        btnRef.current?.focus();
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, setModeMenuOpen, modes, mode, setMode]);

  const menu =
    open &&
    typeof document !== "undefined" &&
    createPortal(
      <div
        ref={menuRef}
        role="listbox"
        style={{
          ...noDrag,
          position: "fixed",
          top: pos.top,
          right: pos.right,
          zIndex: 9999,
        }}
        className="w-[min(100vw-1.5rem,340px)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] p-1.5 shadow-[var(--shadow-soft)]"
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
              style={noDrag}
              onClick={(e) => {
                e.preventDefault();
                e.stopPropagation();
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
        {catalog.essential.length > 0 && (
          <div className="border-t border-[var(--color-border)] px-2.5 py-1.5 text-[10px] text-[var(--color-subtle)]">
            {catalog.source === "live" ? "Live" : "Fallback"} · {catalog.essential.length}{" "}
            models · slots by {catalog.classifiedBy === "grok" ? "Grok" : "heuristic"}
          </div>
        )}
      </div>,
      document.body,
    );

  return (
    <div className="relative" style={noDrag}>
      <button
        ref={btnRef}
        type="button"
        style={noDrag}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setModeMenuOpen(!open);
        }}
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
      {menu}
    </div>
  );
}
