// Settings → Pulse → Know me (src-tauri/src/know_me.rs).
import { call } from "./transport";

export interface KnowMeSignal {
  id: string;
  label: string;
  /** What it reads, for hover. */
  reads: string;
  on: boolean;
}

export interface KnowMeTag {
  tag: string;
  /** "from game library". */
  from: string;
}

export interface KnowMeView {
  on: boolean;
  signals: KnowMeSignal[];
  lastScan: string | null;
  tags: KnowMeTag[];
}

export interface KnowMeAnswer {
  view: KnowMeView;
  status: string;
}

export const getKnowMe = () => call<KnowMeView>("get_know_me");
export const setKnowMeOn = (on: boolean) => call<KnowMeAnswer>("set_know_me_on", { on });
export const setKnowMeSignal = (id: string, on: boolean) => call<KnowMeAnswer>("set_know_me_signal", { id, on });
export const knowMeScan = () => call<KnowMeAnswer>("know_me_scan");
export const knowMeDelete = (tag: string) => call<KnowMeAnswer>("know_me_delete", { tag });
export const knowMeWipe = () => call<KnowMeAnswer>("know_me_wipe");
