// Desktop companion window + wardrobe (src-tauri/src/desktop_nyx.rs).
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call, subscribe } from "./transport";

export interface DesktopNyxEffects {
  transparent: boolean;
  onTop: boolean;
}

export interface DesktopNyxOutfit {
  id: string;
  label: string;
  file: string;
  adult: boolean;
}

export interface DesktopNyxState {
  enabled: boolean;
  undocked: boolean;
  behind: boolean;
  adult: boolean;
  adultsOk: boolean;
  outfit: string | null;
  outfitFile: string;
  outfits: DesktopNyxOutfit[];
  pos: [number, number] | null;
  effects: DesktopNyxEffects;
  companion: string;
}

export interface DesktopNyxPatch {
  enabled?: boolean;
  undocked?: boolean;
  behind?: boolean;
  adult?: boolean;
}

/** Sent after every desktop-companion change, from any page or the tray. */
export const DESKTOP_NYX_EVENT = "desktop-nyx://changed";

/** Hears every desktop-companion change, with the whole new state. */
export function onDesktopNyx(handler: (state: DesktopNyxState) => void): Promise<UnlistenFn> {
  return subscribe<DesktopNyxState>(DESKTOP_NYX_EVENT, handler);
}

export function desktopNyxGet(): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_get");
}

export function desktopNyxSet(patch: DesktopNyxPatch): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_set", { patch });
}

export function desktopNyxUndock(): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_undock");
}

export function desktopNyxDock(): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_dock");
}

export function desktopNyxSetOutfit(id: string | null): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_set_outfit", { id });
}

/** A part of her page that takes the mouse, in CSS pixels from the window's
 * top-left. Clicks pass through everywhere else. */
export interface DesktopNyxHitBox {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function desktopNyxSetHit(boxes: DesktopNyxHitBox[]): Promise<void> {
  return call<void>("desktop_nyx_set_hit", { boxes });
}

/** A press on her: the system drags her window; where she lands is saved. */
export function desktopNyxDrag(): Promise<void> {
  return call<void>("desktop_nyx_drag");
}
