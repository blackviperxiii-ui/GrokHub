import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { Capabilities } from "./api";
import {
  answersTo,
  chatRows,
  commandRows,
  NO_CHATS,
  NO_SLASH,
  SETTINGS_KEYS,
  slashRows,
  type ChatHit,
  type ChatSource,
  type CommandId,
  type CommandRunners,
  type SlashHit,
  type SlashSource,
} from "./paletteCommands";
import { revealItem, usePeek } from "./peek";
import { SECTION_LABELS, sections, type Section } from "./sections";
import { PRODUCT_NAME } from "./brand";

export type View = "chat" | "pulse" | "imagine" | "journal" | "automations" | "agents" | "memory" | "skills" | "connectors" | "settings" | "library" | "workboard" | "devices";

/** What a palette row does. */
export type PaletteAction =
  | { kind: "view"; view: View }
  | { kind: "settings"; section: Section }
  | { kind: "ask"; text: string }
  | { kind: "command"; id: CommandId }
  | { kind: "chat"; id: string };

export interface PaletteRow {
  label: string;
  action: PaletteAction;
  /** The key that does the same, shown at the row's right. */
  keys?: string;
}

/** Where the rows beyond pages and Settings sections come from. */
export interface PaletteSources {
  runners: CommandRunners;
  signedOut: boolean;
  chats: ChatSource;
  slash: SlashSource;
}

/** Pages and Settings sections only. */
export const NO_SOURCES: PaletteSources = { runners: {}, signedOut: false, chats: NO_CHATS, slash: NO_SLASH };

/** The sources' rows for one query, already looked up. */
export interface PaletteExtras {
  runners: CommandRunners;
  signedOut: boolean;
  chats: readonly ChatHit[];
  slash: readonly SlashHit[];
}

const NO_EXTRAS: PaletteExtras = { runners: {}, signedOut: false, chats: [], slash: [] };

export const VIEW_LABELS: Record<View, string> = {
  chat: "Chat",
  pulse: "Pulse",
  imagine: "Imagine",
  journal: "Journal",
  automations: "Automations",
  agents: "Agents",
  memory: "Memory",
  skills: "Skills",
  connectors: "Connectors",
  settings: "Settings",
  workboard: "Workboard",
  devices: "Devices",
  library: "Library",
};

/** The pages this platform shows, in tab order. The journal is this
 * computer's file, so the web app leaves it on the desktop. Devices is about this computer's Web access, so
 * only a desktop that has it shows it. */
export function views(caps: Capabilities): View[] {
  return [
    "chat",
    ...(caps.pulse ? (["pulse"] as const) : []),
    "imagine",
    ...(caps.remote ? [] : (["journal"] as const)),
    "automations",
    "agents",
    "memory",
    "skills",
    "connectors",
    "workboard",
    ...(caps.webAccess && !caps.remote ? (["devices"] as const) : []),
    ...(caps.library ? (["library"] as const) : []),
    "settings",
  ];
}

/** The rows for `query`, in order: pages, commands, slash commands (for a
 * query that starts with `/`), recent chats, Settings sections, then Ask
 * with the typed text, which puts it in the chat box like the cabin's palette. */
export function paletteRows(caps: Capabilities, query: string, extras: PaletteExtras = NO_EXTRAS): PaletteRow[] {
  const q = query.trim().toLowerCase();
  const slash = q.startsWith("/");
  const pages: PaletteRow[] = views(caps).map((view) => ({
    label: VIEW_LABELS[view],
    action: { kind: "view", view },
    ...(view === "settings" ? { keys: SETTINGS_KEYS } : {}),
  }));
  // The web app's Settings is one page on the desktop's terms.
  const settings: PaletteRow[] = (caps.remote ? [] : sections(caps)).map((section) => ({
    label: `Settings: ${SECTION_LABELS[section]}`,
    action: { kind: "settings", section },
  }));
  const named = (rows: PaletteRow[]) => (q ? rows.filter((row) => answersTo(row.label, q)) : rows);
  const all = [
    ...named(pages),
    ...named(commandRows(caps, extras)),
    ...(slash ? slashRows(extras.slash) : []),
    ...(slash ? [] : chatRows(extras.chats)),
    ...named(settings),
  ];
  if (!q) return all;
  return [...all, { label: `Ask ${PRODUCT_NAME}: ${query.trim()}`, action: { kind: "ask", text: query.trim() } }];
}

const NOTHING_FOUND = { chats: [], slash: [] };

/** Rows can share a label (two chats with one title), so keys use the action. */
function rowKey(row: PaletteRow): string {
  return row.action.kind === "chat" ? `chat:${row.action.id}` : row.label;
}

/** `Ctrl+Alt+H` as aria-keyshortcuts spells it. */
function ariaKeys(keys: string): string {
  return keys.replace(/\bCtrl\b/g, "Control");
}

/** The list may take half the window, as in the stylesheet. */
const paletteRoom = () => window.innerHeight * 0.5;

/** Ctrl+K (⌘K on a Mac) or the Command button opens it; Escape, a click
 * outside or a pick closes it. */
export function Palette({
  caps,
  sources = NO_SOURCES,
  onPick,
  onClose,
}: {
  caps: Capabilities;
  sources?: PaletteSources;
  onPick: (action: PaletteAction) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState(0);
  const [found, setFound] = useState<{ chats: readonly ChatHit[]; slash: readonly SlashHit[] }>(NOTHING_FOUND);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  // The way the last arrow key moved, so only key moves (not hovers) scroll.
  const moved = useRef<1 | -1 | null>(null);
  const rows = paletteRows(caps, query, { ...sources, ...found });
  const { chats: findChats, slash: findSlash } = sources;
  const at = Math.min(pick, rows.length - 1);
  usePeek(list, "y", rows.length, paletteRoom);

  useEffect(() => {
    input.current?.focus();
  }, []);

  // Chats and slash commands come from the desktop, so they arrive a moment
  // later; an answer for an older query is dropped.
  useEffect(() => {
    let live = true;
    const q = query.trim();
    const keep = (next: typeof found) => {
      if (!live) return;
      setFound((now) => (now.chats.length + now.slash.length + next.chats.length + next.slash.length === 0 ? now : next));
    };
    const lost = () => keep(NOTHING_FOUND);
    if (q.startsWith("/")) findSlash(q).then((slash) => keep({ chats: [], slash }), lost);
    else findChats(q).then((chats) => keep({ chats, slash: [] }), lost);
    return () => {
      live = false;
    };
  }, [query, findChats, findSlash]);

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
              key={rowKey(row)}
              id={`palette-row-${i}`}
              role="option"
              aria-selected={i === at}
              aria-keyshortcuts={row.keys && ariaKeys(row.keys)}
              onMouseEnter={() => setPick(i)}
              onClick={() => choose(row)}
            >
              <span>{row.label}</span>
              {row.keys && <kbd aria-hidden="true">{row.keys}</kbd>}
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
