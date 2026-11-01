import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { Capabilities } from "./api";
import { revealItem, usePeek } from "./peek";
import { SECTION_LABELS, sections, type Section } from "./sections";

export type View = "chat" | "pulse" | "journal" | "automations" | "memory" | "connectors" | "settings" | "workboard";

/** What a palette row does. */
export type PaletteAction =
  | { kind: "view"; view: View }
  | { kind: "settings"; section: Section }
  | { kind: "ask"; text: string };

export interface PaletteRow {
  label: string;
  action: PaletteAction;
}

export const VIEW_LABELS: Record<View, string> = {
  chat: "Chat",
  pulse: "Pulse",
  journal: "Journal",
  automations: "Automations",
  memory: "Memory",
  connectors: "Connectors",
  settings: "Settings",
  workboard: "Workboard",
};

/** The pages this platform shows, in tab order. The journal is this
 * computer's file, so the web app leaves it on the desktop. */
export function views(caps: Capabilities): View[] {
  return [
    "chat",
    ...(caps.pulse ? (["pulse"] as const) : []),
    ...(caps.remote ? [] : (["journal"] as const)),
    "automations",
    "memory",
    "connectors",
    "workboard",
    "settings",
  ];
}

/** The rows for `query`: pages, then Settings sections, then Ask Grok with
 * the typed text, which puts it in the chat box like the cabin's palette. */
export function paletteRows(caps: Capabilities, query: string): PaletteRow[] {
  const all: PaletteRow[] = [
    ...views(caps).map((view) => ({
      label: VIEW_LABELS[view],
      action: { kind: "view", view } as const,
    })),
    // The web app's Settings is one page on the desktop's terms.
    ...(caps.remote ? [] : sections(caps)).map((section) => ({
      label: `Settings: ${SECTION_LABELS[section]}`,
      action: { kind: "settings", section } as const,
    })),
  ];
  const q = query.trim().toLowerCase();
  if (!q) return all;
  const hits = all.filter((row) => row.label.toLowerCase().includes(q));
  return [...hits, { label: `Ask Grok: ${query.trim()}`, action: { kind: "ask", text: query.trim() } }];
}

/** The list may take half the window, as in the stylesheet. */
const paletteRoom = () => window.innerHeight * 0.5;

/** Ctrl+K (⌘K on a Mac) or the Command button opens it; Escape, a click
 * outside or a pick closes it. */
export function Palette({
  caps,
  onPick,
  onClose,
}: {
  caps: Capabilities;
  onPick: (action: PaletteAction) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  // The way the last arrow key moved, so only key moves (not hovers) scroll.
  const moved = useRef<1 | -1 | null>(null);
  const rows = paletteRows(caps, query);
  const at = Math.min(pick, rows.length - 1);
  usePeek(list, "y", rows.length, paletteRoom);

  useEffect(() => {
    input.current?.focus();
  }, []);

  useEffect(() => {
    const dir = moved.current;
    moved.current = null;
    if (dir && list.current) revealItem(list.current, at, dir);
  }, [at]);

  function choose(row: PaletteRow | undefined) {
    if (!row) return;
    onClose();
    onPick(row.action);
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      moved.current = 1;
      setPick((at + 1) % rows.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      moved.current = -1;
      setPick((at - 1 + rows.length) % rows.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      choose(rows[at]);
    }
  }

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <input
          ref={input}
          aria-label="Search pages and commands"
          placeholder="Search pages and commands"
          value={query}
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-rows"
          aria-activedescendant={`palette-row-${at}`}
          onChange={(e) => {
            setQuery(e.target.value);
            moved.current = null;
            setPick(0);
          }}
        />
        <ul ref={list} id="palette-rows" role="listbox" aria-label="Pages and commands">
          {rows.map((row, i) => (
            <li
              key={row.label}
              id={`palette-row-${i}`}
              role="option"
              aria-selected={i === at}
              onMouseEnter={() => setPick(i)}
              onClick={() => choose(row)}
            >
              {row.label}
            </li>
          ))}
        </ul>
        <button type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
