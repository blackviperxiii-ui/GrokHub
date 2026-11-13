// Auto setup (src-tauri/src/auto_setup.rs): the app sets itself up for this
// PC and the person, and keeps tuning. Desktop only: the web app and the
// tests get `null` and show nothing.
import type { Theme } from "./api";
import { applyTheme } from "./theme";
import { call, inShell } from "./transport";

/** A setting on the shared list, as `grokhub_core::autotune::settings::Key`. */
export type Key =
  | "textSize"
  | "reduceMotion"
  | "theme"
  | "closeToTray"
  | "quietHours"
  | "notifications"
  | "suggestions"
  | "personality"
  | "approvals";

export type Value =
  | { t: "bool"; v: boolean }
  | { t: "pct"; v: number }
  | { t: "hour"; v: number }
  | { t: "hours"; v: [number, number] }
  | { t: "pick"; v: string };

export type Pace = "quick" | "detailed";

export interface Line {
  key: Key;
  /** "Text size: 150%" or "Turn on reduce motion?". */
  text: string;
  reason: string;
}

export interface Note {
  line: Line;
  /** An offer (Yes, Not now); otherwise a change (Undo, OK). */
  offer: boolean;
}

export interface Question {
  id: string;
  prompt: string;
  choices: { label: string; value: string }[];
}

export interface Row {
  key: Key;
  label: string;
  text: string;
  /** Undo puts the old value back; otherwise Hand back. */
  undo: boolean;
}

export interface AutoReply {
  firstRun: boolean;
  textSize: number;
  reduceMotion: boolean;
  theme: Theme;
  question: Question | null;
  summary: boolean;
  made: Line[];
  offered: Line[];
  note: Note | null;
  rows: Row[];
  said: string | null;
}

export type Act =
  | { act: "view" }
  | { act: "pass"; back: boolean }
  | { act: "bigger" }
  | { act: "start"; pace: Pace }
  | { act: "answer"; id: string; value?: string }
  | { act: "finish" }
  | { act: "undo"; key: Key }
  | { act: "accept"; key: Key }
  | { act: "dismiss"; key: Key }
  | { act: "set"; key: Key; value: Value }
  | { act: "handBack"; key: Key }
  | { act: "handBackAll" }
  | { act: "forget" }
  | { act: "chat"; text: string };

/** Text sizes on offer, as `autotune::settings::TEXT_STEPS`. */
export const TEXT_STEPS = [100, 115, 130, 150] as const;

/** Chat asks for a setting are short ("make the text bigger"). */
const ASK_MAX = 80;

/** Runs one act and puts its look on the page. `null` off the desktop. */
export async function autoSetup(act: Act): Promise<AutoReply | null> {
  if (!inShell()) return null;
  const reply = await call<AutoReply>("auto_setup", { act });
  applyLook(reply);
  return reply;
}

/** Reduce motion and the theme; the window's zoom follows the text size itself. */
export function applyLook(reply: Pick<AutoReply, "reduceMotion" | "theme">) {
  document.documentElement.dataset.reduceMotion = reply.reduceMotion ? "true" : "false";
  applyTheme(reply.theme);
}

/** What auto setup says back when a chat line asks for a setting, else `null`. */
export async function settingAsk(text: string): Promise<string | null> {
  if (!inShell() || text.length > ASK_MAX || text.includes("\n")) return null;
  try {
    return (await autoSetup({ act: "chat", text }))?.said ?? null;
  } catch {
    return null;
  }
}
