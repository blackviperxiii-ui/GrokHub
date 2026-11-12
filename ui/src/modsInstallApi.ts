// Mods 2 of 2 (src-tauri/src/mods_install.rs): a mod's files, the install
// sheet, uninstall and updates. Desktop only.
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call, inShell, subscribe } from "./transport";

/** Progress lines for the one install at a time. */
export const MODS_JOB_EVENT = "mods://job";

export type ModOffer =
  | { kind: "install" }
  | { kind: "update"; was: string }
  | { kind: "installed" }
  | { kind: "browser"; url: string }
  | { kind: "steam"; url: string }
  | { kind: "blocked" };

export interface ModFileRow {
  id: string;
  name: string;
  /** "1.2.0 · 4.2 MB · download on the site". */
  line: string;
  offer: ModOffer;
  /** The page waits for this browser download: "I downloaded it" shows. */
  waiting: boolean;
}

export interface ModDrawer {
  files: ModFileRow[];
  /** Why the app won't install for this game. */
  blocked: string | null;
  needs: string[];
  about: string;
  note: string | null;
}

export interface ModSheetList {
  title: string;
  rows: string[];
}

export interface ModSheet {
  token: number;
  title: string;
  primary: string;
  into: string | null;
  lists: ModSheetList[];
  notes: string[];
}

export interface ModOurs {
  key: string;
  name: string;
  version: string;
  offlineOnly: string | null;
}

export interface ModOther {
  name: string;
  detail: string;
  link: string | null;
}

export interface ModInstalled {
  ours: ModOurs[];
  others: ModOther[];
  recovered: string | null;
}

export function modsFiles(
  game: string,
  source: string,
  id: string,
  summary: string,
): Promise<ModDrawer> {
  return call<ModDrawer>("mods_files", { game, source, id, summary });
}

export function modsPlanInstall(
  game: string,
  source: string,
  id: string,
  name: string,
  file: string,
): Promise<ModSheet> {
  return call<ModSheet>("mods_plan_install", { game, source, id, name, file });
}

export function modsWaitForBrowser(
  game: string,
  source: string,
  id: string,
  file: string,
): Promise<void> {
  return call<void>("mods_wait_for_browser", { game, source, id, file });
}

export function modsPickupDownload(
  game: string,
  source: string,
  id: string,
  name: string,
  file: string,
): Promise<ModSheet> {
  return call<ModSheet>("mods_pickup_download", {
    game,
    source,
    id,
    name,
    file,
  });
}

export function modsPlanUninstall(
  game: string,
  key: string,
): Promise<ModSheet> {
  return call<ModSheet>("mods_plan_uninstall", { game, key });
}

export function modsConfirm(token: number): Promise<string> {
  return call<string>("mods_confirm", { token });
}

export function modsCancel(token: number): Promise<void> {
  return call<void>("mods_cancel", { token });
}

export function modsInstalled(game: string): Promise<ModInstalled> {
  return call<ModInstalled>("mods_installed", { game });
}

export function onModsJob(
  handler: (line: string) => void,
): Promise<UnlistenFn> {
  if (!inShell()) return Promise.resolve(() => {});
  return subscribe<string>(MODS_JOB_EVENT, handler);
}
