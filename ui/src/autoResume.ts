// The agent's own interrupted runs resume from their checkpoints by
// themselves, twice a day at most. Past that a card waits in Blocked, and the
// app says so once, for the bell.
import type { UnlistenFn } from "@tauri-apps/api/event";
import { subscribe } from "./transport";

/** Sent when a card's auto-resume stops for the day. */
export const AUTO_RESUME_EVENT = "workboard://auto-resume";

export interface AutoResumeStopped {
  cardId: string;
  /** "Auto-resume stopped: <card>. Resume it from the Workboard." */
  text: string;
}

export function onAutoResumeStopped(
  handler: (stopped: AutoResumeStopped) => void,
): Promise<UnlistenFn> {
  return subscribe<AutoResumeStopped>(AUTO_RESUME_EVENT, handler);
}
