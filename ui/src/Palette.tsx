import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { decisionInbox, type Capabilities, type QuickChip } from "./api";
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
import { useModalFocus } from "./ui";

export type View =
  | "home"
  | "objectives"
  | "chat"
  | "code"
  | "command"
  | "history"
  | "pulse"
  | "imagine"
  | "journal"
  | "automations"
  | "agents"
  | "memory"
  | "skills"
  | "connectors"
  | "settings"
  | "library"
  | "mods"
  | "workboard"
  | "devices"
  | "updates";

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
  /** The suggestion this row came from, taught on pick. */
  chip?: QuickChip;
  /** The key that does the same, shown at the row's right. */
  keys?: string;
}

/** Where the rows beyond pages and Settings sections come from. */
export interface PaletteSources {
  runners: CommandRunners;
  signedOut: boolean;
  chats: ChatSource;
  slash: SlashSource;
  /** Suggested next steps, shown first while nothing is typed. */
  chips?: readonly QuickChip[];
  /** A suggestion was picked. */
  onChip?: (chip: QuickChip) => void;
}

/** Pages and Settings sections only. */
export const NO_SOURCES: PaletteSources = { runners: {}, signedOut: false, chats: NO_CHATS, slash: NO_SLASH };

/** The sources' rows for one query, already looked up. */
export interface PaletteExtras {
  runners: CommandRunners;
  signedOut: boolean;
  chats: readonly ChatHit[];
  slash: readonly SlashHit[];
  chips?: readonly QuickChip[];
  /** Only these pages, and nothing else: the rail's More list. */
  only?: readonly View[];
}

const NO_EXTRAS: PaletteExtras = { runners: {}, signedOut: false, chats: [], slash: [] };

/** A suggestion as a palette row: a Go chip opens its page when this app has
 * it, an Ask chip puts its words in the chat box. */
function chipRow(chip: QuickChip, views: readonly View[]): PaletteRow | null {
  const label = `Suggested: ${chip.label}`;
  if (chip.action === "ask") return { label, chip, action: { kind: "ask", text: chip.value } };
  const view = views.find((v) => v === chip.view);
  return view ? { label, chip, action: { kind: "view", view } } : null;
}

export const VIEW_LABELS: Record<View, string> = {
  home: "Home",
  objectives: "Objectives",
  chat: "Chat",
  code: "Code",
  command: "Command line",
  history: "History",
  pulse: "Pulse",
  imagine: "Imagine",
  journal: "Journal",
  automations: "Automations",
  agents: "Agents",
  memory: "Personal",
  skills: "Skills",
  connectors: "Connectors",
  settings: "Settings",
  workboard: "Workboard",
  devices: "Devices",
  library: "Library",
  mods: "Mods",
  updates: "Updates & Drivers",
};

/** The pages this platform shows, in tab order. Code, History and the journal
 * are this computer's files, so the web app leaves them on the desktop (a
 * browser sees only the chats it started); Devices is about this computer's
 * Web access, so only a desktop that has it shows it. */
export function views(caps: Capabilities): View[] {
  return [
    "home",
    "chat",
    ...(caps.code && !caps.remote ? (["code"] as const) : []),
    ...(caps.remote ? [] : (["history"] as const)),
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
    ...(caps.mods && !caps.remote ? (["mods"] as const) : []),
    "settings",
  ];
}

/** Every page the palette opens: the tabs, Objectives right after Home, the
 * Command line right after Code, and Updates & Drivers (this computer's
 * drivers, so not in the web app) right before Settings. Home's tiles open
 * Objectives and Updates & Drivers too. */
export function pageViews(caps: Capabilities): View[] {
  const tabs = views(caps);
  const at = tabs.indexOf("home") + 1;
  const pages: View[] = [...tabs.slice(0, at), "objectives", ...tabs.slice(at)];
  const code = pages.indexOf("code");
  const withCommand: View[] = code < 0 ? pages : [...pages.slice(0, code + 1), "command", ...pages.slice(code + 1)];
  if (caps.remote) return withCommand;
  const settings = withCommand.indexOf("settings");
  return [...withCommand.slice(0, settings), "updates", ...withCommand.slice(settings)];
}

/** Whether this app has `view` to open. */
export function hasPage(caps: Capabilities, view: View): boolean {
  return pageViews(caps).includes(view);
}

/** The pages on the left rail, top to bottom; Settings sits at the rail's foot. */
export const RAIL: readonly View[] = ["home", "chat", "code", "pulse", "library", "settings"];

/** The rail pages this platform has. */
export function railViews(caps: Capabilities): View[] {
  const shown = views(caps);
  return RAIL.filter((view) => shown.includes(view));
}

/** The pages the rail's More opens: every page that is not on the rail. */
export function moreViews(caps: Capabilities): View[] {
  const rail = railViews(caps);
  return pageViews(caps).filter((view) => !rail.includes(view));
}

/** The rows for `query`, in order: suggestions (while nothing is typed),
 * pages, commands, slash commands (for a query that starts with `/`), recent
 * chats, Settings sections, then Ask with the typed text, which puts it in the
 * chat box like the cabin's palette. */
export function paletteRows(caps: Capabilities, query: string, extras: PaletteExtras = NO_EXTRAS): PaletteRow[] {
  const q = query.trim().toLowerCase();
  const slash = q.startsWith("/");
  const pages: PaletteRow[] = pageViews(caps).map((view) => ({
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
  if (extras.only) {
    const only = extras.only;
    return named(pages.filter((row) => row.action.kind === "view" && only.includes(row.action.view)));
  }
  const views = pageViews(caps);
  const suggested = q ? [] : (extras.chips ?? []).flatMap((chip) => chipRow(chip, views) ?? []);
  const all = [
    ...suggested,
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
  if (row.chip) return `chip:${row.chip.id}`;
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
  only,
  onPick,
  onClose,
}: {
  caps: Capabilities;
  sources?: PaletteSources;
  /** Only these pages: the rail's More list, with no commands, chats or Ask. */
  only?: readonly View[];
  onPick: (action: PaletteAction) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState(0);
  const [found, setFound] = useState<{ chats: readonly ChatHit[]; slash: readonly SlashHit[] }>(NOTHING_FOUND);
  const input = useRef<HTMLInputElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const list = useRef<HTMLUListElement>(null);
  // The way the last arrow key moved, so only key moves (not hovers) scroll.
  const moved = useRef<1 | -1 | null>(null);
  // The decision inbox (Order 733): while anything waits on you, the first
  // row opens Chat, where the inbox sits.
  const [waiting, setWaiting] = useState(0);
  useEffect(() => {
    if (only) return;
    let live = true;
    Promise.resolve()
      .then(() => decisionInbox())
      .then((rows) => {
        if (live) setWaiting(rows.length);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [only]);
  const inboxLabel = `Decisions waiting · ${waiting}`;
  const inboxRow: PaletteRow[] =
    waiting > 0 && (!query.trim() || answersTo(inboxLabel, query.trim().toLowerCase()))
      ? [{ label: inboxLabel, action: { kind: "view", view: "chat" } }]
      : [];
  const rows = only
    ? paletteRows(caps, query, { ...NO_EXTRAS, only })
    : [...inboxRow, ...paletteRows(caps, query, { ...sources, ...found })];
  const none = rows.length === 0;
  const { onChip } = sources;
  const { chats: findChats, slash: findSlash } = sources;
  const at = Math.min(pick, rows.length - 1);
  usePeek(list, "y", rows.length, paletteRoom);
  useModalFocus(box);

  useEffect(() => {
    input.current?.focus();
  }, []);

  // Chats and slash commands come from the desktop, so they arrive a moment
  // later; an answer for an older query is dropped.
  useEffect(() => {
    if (only) return;
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
  }, [query, findChats, findSlash, only]);

  useEffect(() => {
    const dir = moved.current;
    moved.current = null;
    if (dir && list.current) revealItem(list.current, at, dir);
  }, [at]);

  function choose(row: PaletteRow | undefined) {
    if (!row) return;
    if (row.chip) onChip?.(row.chip);
    onClose();
    onPick(row.action);
  }

  function onKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      onClose();
    } else if (none && ["ArrowDown", "ArrowUp", "Enter"].includes(event.key)) {
      event.preventDefault();
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
        ref={box}
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label={only ? "More pages" : "Command palette"}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <input
          ref={input}
          aria-label={only ? "Search pages" : "Search pages and commands"}
          placeholder={only ? "Search pages" : "Search pages and commands"}
          value={query}
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-rows"
          aria-activedescendant={none ? undefined : `palette-row-${at}`}
          onChange={(e) => {
            setQuery(e.target.value);
            moved.current = null;
            setPick(0);
          }}
        />
        <ul ref={list} id="palette-rows" role="listbox" aria-label={only ? "Pages" : "Pages and commands"}>
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
        {none && <p className="palette-none">No page matches “{query.trim()}”.</p>}
        <button type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
