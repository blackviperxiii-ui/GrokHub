// How the chat's state shows on her desktop pedestal (src-tauri/src/desktop_nyx.rs VoiceLook).
import type { DesktopNyxLook } from "./desktopNyxApi";
import type { VoiceState } from "./VoiceMode";

/** Listening and speaking follow the voice; a reply running here, or the
 * voice waiting on one, is thinking; anything else is idle. */
export function deskLook(voice: VoiceState, busy: boolean): DesktopNyxLook {
  if (voice === "listening" || voice === "speaking") return voice;
  if (voice === "thinking" || busy) return "thinking";
  return "idle";
}
