// Desktop companion window + wardrobe (src-tauri/src/desktop_nyx.rs).
import { call } from "./transport";

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

export function desktopNyxSavePos(
  x: number,
  y: number,
  monitorW: number,
  monitorH: number,
): Promise<DesktopNyxState> {
  return call<DesktopNyxState>("desktop_nyx_save_pos", {
    x,
    y,
    monitorW,
    monitorH,
  });
}

export function desktopNyxSetPassthrough(ignore: boolean): Promise<void> {
  return call<void>("desktop_nyx_set_passthrough", { ignore });
}
