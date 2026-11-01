// The palette's rows beyond pages and Settings sections: commands, slash
// commands and recent chats. Each comes from a source the shell hands the
// palette, so a row shows only once the shell can do what it says.
import { slashHits, type Capabilities } from "./api";
import type { PaletteRow } from "./Palette";
import SYNONYMS from "./paletteSynonyms.json";

export type CommandId = "new-chat" | "halt" | "sign-in" | "shortcuts";

/** What the shell can run now, by command. A command with no runner here has
 * no row, no key and no line on the shortcut sheet. */
export type CommandRunners = Partial<Record<CommandId, () => void>>;

/** What the palette knows when it lists the commands. */
export interface CommandState {
  runners: CommandRunners;
  /** No API key yet: the Sign in row shows. */
  signedOut: boolean;
}

interface Command {
  id: CommandId;
  label: string;
  keys?: string;
  /** Whether the row shows on this platform, in this state. */
  shows: (caps: Capabilities, state: CommandState) => boolean;
}

const always = () => true;

/** The commands, in palette order. Desktop-only ones drop out of the web app. */
export const COMMANDS: readonly Command[] = [
  { id: "new-chat", label: "New chat", keys: "Ctrl+N", shows: always },
  {
    id: "halt",
    label: "Halt",
    keys: "Ctrl+Alt+H",
    shows: (caps) => caps.desktopControl && !caps.remote,
  },
  // The web app signs in on the computer, so it has no Sign in row.
  { id: "sign-in", label: "Sign in", shows: (caps, state) => state.signedOut && !caps.remote },
  { id: "shortcuts", label: "Shortcuts", keys: "Ctrl+/", shows: always },
];

/** The Settings page answers to Ctrl+, as well. */
export const SETTINGS_KEYS = "Ctrl+,";

export function commandRows(caps: Capabilities, state: CommandState): PaletteRow[] {
  return COMMANDS.filter((c) => state.runners[c.id] && c.shows(caps, state)).map((c) => ({
    label: c.label,
    keys: c.keys,
    action: { kind: "command", id: c.id },
  }));
}

/** One chat from the chat history, newest first. */
export interface ChatHit {
  id: string;
  title: string;
}

/** Chats whose titles match the query (recent ones for an empty query). */
export type ChatSource = (query: string) => Promise<ChatHit[]>;

/** One slash command the composer can run, such as `/plan`, and what a pick
 * puts in the chat box (`/plan ` for one that takes text). */
export interface SlashHit {
  name: string;
  insert: string;
}

/** Slash commands that start with the typed `/` text. */
export type SlashSource = (query: string) => Promise<SlashHit[]>;

/** Until the shell keeps a chat history, no chat rows. */
export const NO_CHATS: ChatSource = () => Promise.resolve([]);

/** No slash rows. */
export const NO_SLASH: SlashSource = () => Promise.resolve([]);

/** The composer's slash commands. Ones that can't run here (the cabin's own)
 * stay out, since a pick would only fill the box with a command that fails. */
export const COMPOSER_SLASH: SlashSource = (query) =>
  slashHits(query).then((hits) =>
    hits.filter((hit) => !hit.disabled).map((hit) => ({ name: hit.cmd, insert: hit.insert })),
  );

/** Rows of recent chats the palette lists at most. */
export const CHAT_ROWS = 5;

export function chatRows(hits: readonly ChatHit[]): PaletteRow[] {
  return hits.slice(0, CHAT_ROWS).map((hit) => ({
    label: `Chat: ${hit.title}`,
    action: { kind: "chat", id: hit.id },
  }));
}

/** A slash pick puts the command in the chat box, where Enter runs it. */
export function slashRows(hits: readonly SlashHit[]): PaletteRow[] {
  return hits.map((hit) => ({ label: hit.name, action: { kind: "ask", text: hit.insert } }));
}

/** The older words the cabin's palette also answers to, by the row they find.
 * paletteSynonyms.json is shared with a Rust test that checks each word finds
 * the same row in the cabin. */
export const PALETTE_SYNONYMS: readonly { word: string; row: string }[] = SYNONYMS;

/** Whether a row labelled `label` answers to `q` (trimmed, lower case): the
 * label contains it, or a synonym of the row starts with it, as in the cabin. */
export function answersTo(label: string, q: string): boolean {
  return (
    label.toLowerCase().includes(q) ||
    PALETTE_SYNONYMS.some((s) => s.row === label && s.word.startsWith(q))
  );
}

/** The keys the shortcut sheet lists: the palette, each command the shell can
 * run, Settings, and the chat box. */
export function shellKeys(caps: Capabilities, runners: CommandRunners): { keys: string; action: string }[] {
  const state = { runners, signedOut: false };
  return [
    { keys: "Ctrl+K", action: "Command palette" },
    ...COMMANDS.flatMap((c) =>
      c.keys && runners[c.id] && c.shows(caps, state)
        ? [{ keys: c.keys, action: c.id === "shortcuts" ? "This sheet" : c.label }]
        : [],
    ),
    { keys: SETTINGS_KEYS, action: "Settings" },
    { keys: "Enter", action: "Send message" },
    { keys: "Shift+Enter", action: "New line" },
    { keys: "Esc", action: "Close the palette or this sheet" },
  ];
}
