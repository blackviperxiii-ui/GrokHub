// Get to know you (src-tauri/src/onboard.rs): the stage after Auto Setup's
// first run that drafts USER.md, SOUL.md and MEMORY.md for review.
import { call, inShell } from "./transport";

export type Step = "welcome" | "about" | "soul" | "life" | "review" | "done";
export type Stage = "pending" | "done" | "skipped";

export interface Answers {
  name: string;
  callMe: string;
  about: string;
  /** "friendly", "direct", "playful", or empty. */
  tone: string;
  soulNotes: string;
  goals: string;
  interests: string;
  routines: string;
  apps: string;
  prefs: string;
  remember: string;
}

export interface Row {
  /** "about", "likes", "routines", "needs" or "thinking". */
  group: string;
  text: string;
}

export interface Drafts {
  user: string;
  soul: string;
  memory: string;
  rows: Row[];
}

export interface Saved {
  files: string[];
  /** Files over the size cap, which Save left alone. */
  skipped: string[];
  rows: number;
  amr: boolean;
}

export interface OnboardReply {
  pending: boolean;
  stage: Stage;
  step: Step;
  answers: Answers;
  drafts: Drafts | null;
  amr: boolean;
  openclaw: boolean;
  saved: Saved | null;
  note: string | null;
}

export type OnboardAct =
  | { act: "view" }
  | { act: "keep"; step: Step; answers: Answers }
  | { act: "draft"; answers: Answers }
  | { act: "openclaw"; drafts: Drafts }
  | { act: "save"; drafts: Drafts }
  | { act: "skip" }
  | { act: "reopen" };

/** Settings → Personal sends this so the stage opens again. With
 * `detail: { import: true }` it is /import: Review only, see `openImport`. */
export const OPEN_EVENT = "onboard:open";

export const NO_WORKSPACE = "No OpenClaw or Hermes workspace on this computer.";

export const EMPTY_ANSWERS: Answers = {
  name: "",
  callMe: "",
  about: "",
  tone: "",
  soulNotes: "",
  goals: "",
  interests: "",
  routines: "",
  apps: "",
  prefs: "",
  remember: "",
};

/** One act. `null` off the desktop: the web app has no first run. */
export async function onboard(act: OnboardAct): Promise<OnboardReply | null> {
  if (!inShell()) return null;
  return call<OnboardReply>("onboard_identity", { act });
}

/** /import: opens Review with the workspace's notes added. When there is no
 * workspace to read, or the shell refuses, nothing opens and nothing changes:
 * the line to show in the chat comes back instead. */
export async function openImport(): Promise<string | null> {
  try {
    const seen = await onboard({ act: "view" });
    if (!seen?.openclaw) return NO_WORKSPACE;
  } catch (err) {
    return String(err);
  }
  window.dispatchEvent(new CustomEvent(OPEN_EVENT, { detail: { import: true } }));
  return null;
}
