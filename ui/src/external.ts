// Outside links. The webview never navigates away from the app and Tauri
// ignores target="_blank", so web pages open through here: the desktop's
// browser in the shell, a new tab on this device in the web app (never on
// the paired desktop).
import { call as invoke, inShell } from "./transport";

/** Only plain web pages leave the app: http(s) with a host. */
export function externalUrlOk(url: string): boolean {
  try {
    const u = new URL(url);
    return (u.protocol === "https:" || u.protocol === "http:") && u.hostname !== "";
  } catch {
    return false;
  }
}

/** Open a web page in the system browser. In the shell the desktop opens it;
 * the web app opens a new tab on this device (never on the paired desktop). */
export function openExternal(url: string): Promise<void> {
  if (!externalUrlOk(url)) return Promise.reject(new Error("That link can't be opened."));
  if (!inShell()) {
    window.open(url, "_blank", "noopener,noreferrer");
    return Promise.resolve();
  }
  return invoke<void>("open_external", { url });
}
