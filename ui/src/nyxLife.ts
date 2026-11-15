// How the chat's state shows on her desktop pedestal (src-tauri/src/desktop_nyx.rs VoiceLook).
import type { DesktopNyxLook } from "./desktopNyxApi";
import type { VoiceState } from "./VoiceMode";

/** The chat that speaks for her desk: the one last on screen. */
let speaker: symbol | null = null;

export function claimDesk(chat: symbol) {
  speaker = chat;
}

export function deskSpeaker(chat: symbol): boolean {
  return speaker === chat;
}

/** A chat going away stops speaking for her. */
export function releaseDesk(chat: symbol) {
  if (speaker === chat) speaker = null;
}

/** Listening and speaking follow the voice; a reply running here, or the
 * voice waiting on one, is thinking; anything else is idle. */
export function deskLook(voice: VoiceState, busy: boolean): DesktopNyxLook {
  if (voice === "listening" || voice === "speaking") return voice;
  if (voice === "thinking" || busy) return "thinking";
  return "idle";
}
