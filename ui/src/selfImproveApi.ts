// Settings → Behavior → self-improvement (src-tauri/src/self_improve.rs).
import { call } from "./transport";

export interface HistoryRow {
  title: string;
  hint: string;
  /** Set on the newest kept change of a target: Revert offers it. */
  revert: string | null;
}

export interface DomainBar {
  domain: string;
  runs: number;
}

export interface SavedIdea {
  title: string;
  text: string;
  at: number;
}

export interface SelfImproveView {
  quiet: boolean;
  quietNote: string | null;
  history: HistoryRow[];
  domains: string | null;
  bars: DomainBar[];
  ideas: SavedIdea[];
}

export function getSelfImprove(): Promise<SelfImproveView> {
  return call<SelfImproveView>("get_self_improve");
}

export function setSelfImproveQuiet(on: boolean): Promise<SelfImproveView> {
  return call<SelfImproveView>("set_self_improve_quiet", { on });
}

export function selfImproveRevert(version: string): Promise<string> {
  return call<string>("self_improve_revert", { version });
}

export function selfImproveIssue(title: string): Promise<{ text: string; copied: string }> {
  return call<{ text: string; copied: string }>("self_improve_issue", { title });
}
