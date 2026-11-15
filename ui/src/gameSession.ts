import type { UnlistenFn } from "@tauri-apps/api/event";
import { subscribe } from "./transport";

/** The game companion's running or just-closed game changed
 * (src-tauri/src/game_companion.rs `EVENT`): chip rows ask again. */
export const GAME_SESSION_EVENT = "game-session";
/** A companion ping went out (src-tauri/src/os_extras.rs `GAME_NOTICE_EVENT`). */
export const GAME_NOTICE_EVENT = "game-companion-notice";

export function onGameSession(handler: () => void): Promise<UnlistenFn> {
  return subscribe<null>(GAME_SESSION_EVENT, () => handler());
}

/** A game companion ping went out: its title and body, for the bell. */
export function onGameCompanionNotice(handler: (line: string) => void): Promise<UnlistenFn> {
  return subscribe<string>(GAME_NOTICE_EVENT, handler);
}
