// Settings → General → Your browser (src-tauri/src/browser_attach.rs): the
// agent's browser tool may join a browser the user runs, signed in, on the
// sites they allow. Desktop only: the web app and the tests get `null`.
import { call, inShell } from "./transport";

export interface BrowserAttach {
  endpoint: string;
  /** The sites the agent may open there, as origins. */
  origins: string[];
  /** The browser tool joins this browser. */
  active: boolean;
  problem: string | null;
}

export async function getBrowserAttach(): Promise<BrowserAttach | null> {
  if (!inShell()) return null;
  return call<BrowserAttach>("get_browser_attach");
}

/** `sites` are URLs or origins; each is kept as its origin. */
export async function saveBrowserAttach(endpoint: string, sites: string[]): Promise<BrowserAttach | null> {
  if (!inShell()) return null;
  return call<BrowserAttach>("save_browser_attach", { endpoint, sites });
}
