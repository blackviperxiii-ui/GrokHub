// Settings → Bot Desktop (src-tauri/src/bot_screen.rs): the desktop tools
// work on a private screen of their own. Desktop only: the web app and the
// tests get `null` and show nothing.
import { call, inShell } from "./transport";

export interface BotDesktop {
  /** This computer can run one (Linux). */
  supported: boolean;
  on: boolean;
  /** The screen is up: `:N`. */
  display: string | null;
  /** Why it isn't running, or what to install. */
  problem: string | null;
}

export async function getBotDesktop(): Promise<BotDesktop | null> {
  if (!inShell()) return null;
  return call<BotDesktop>("get_bot_desktop");
}

/** Resolves with the switch as it ended up: a screen that can't start stays off. */
export async function setBotDesktop(on: boolean): Promise<BotDesktop | null> {
  if (!inShell()) return null;
  return call<BotDesktop>("set_bot_desktop", { on });
}
