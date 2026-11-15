// How the chat's state shows on her desktop pedestal (src-tauri/src/desktop_nyx.rs VoiceLook).
import { desktopNyxLook, type DesktopNyxLook } from "./desktopNyxApi";
import type { VoiceState } from "./VoiceMode";

/** Every mounted chat's way to send its look to her desk. */
const senders = new Map<symbol, () => void>();
/** The chat last on screen, and the chat whose voice mode is live. */
let screen: symbol | null = null;
let voice: symbol | null = null;

/** The chat that speaks for her desk: a live voice first (it keeps talking
 * with the user while another tab is on screen), else the chat last on
 * screen, which keeps it while no chat is (Settings). */
export function deskSpeaker(): symbol | null {
  return voice ?? screen;
}

/** A chat's state for her desk changed: it sends its look when it speaks
 * for her, and a chat that takes over from another sends its own. */
export function updateDesk(chat: symbol, send: () => void, now: { onScreen: boolean; voiced: boolean }) {
  senders.set(chat, send);
  const before = deskSpeaker();
  if (now.onScreen) screen = chat;
  if (now.voiced) voice = chat;
  else if (voice === chat) voice = null;
  settle(before, chat);
}

/** A chat going away hands her desk to the one that speaks next, or settles
 * her when none is left. */
export function leaveDesk(chat: symbol) {
  senders.delete(chat);
  const before = deskSpeaker();
  if (screen === chat) screen = null;
  if (voice === chat) voice = null;
  settle(before, chat);
}

function settle(before: symbol | null, chat: symbol) {
  const after = deskSpeaker();
  if (after === chat) senders.get(chat)?.();
  else if (after !== before) {
    if (after) senders.get(after)?.();
    else desktopNyxLook("idle", "hanging-out").catch(() => {});
  }
}

/** Listening and speaking follow the voice; a reply running here, or the
 * voice waiting on one, is thinking; anything else is idle. */
export function deskLook(voice: VoiceState, busy: boolean): DesktopNyxLook {
  if (voice === "listening" || voice === "speaking") return voice;
  if (voice === "thinking" || busy) return "thinking";
  return "idle";
}
