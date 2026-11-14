// Settings → Permissions → "What Loadout can read" (src-tauri/src/scopes.rs).
import type { UnlistenFn } from "@tauri-apps/api/event";
import { call, subscribe } from "./transport";

export interface GrantedScope {
  id: string;
  title: string;
  hint: string;
  /** The whole folder path, for hover. */
  detail: string | null;
  /** What it taught the app, and whether "Forget these" shows. */
  learned: Learned;
}

export interface Learned {
  head: string | null;
  lines: string[];
  more: string | null;
  note: string | null;
  canForget: boolean;
}

export interface ScopeRow {
  kind: string;
  granted: GrantedScope[];
  /** The Allow row, when one shows. */
  add: { title: string; hint: string } | null;
}

export interface ScopesView {
  heading: string;
  note: string;
  folderHint: string;
  browsers: [string, string][];
  rows: ScopeRow[];
  locked: string | null;
  lockHover: string | null;
}

export interface ScopeAnswer {
  view: ScopesView;
  status: string;
}

export const getScopes = () => call<ScopesView>("get_scopes");
export const allowScope = (kind: string, value: string) =>
  call<ScopeAnswer>("allow_scope", { kind, value });
export const revokeScope = (id: string) => call<ScopeAnswer>("revoke_scope", { id });
export const pickScopeFolder = () => call<string | null>("pick_scope_folder");
export const forgetScopeFacts = (id: string) => call<ScopeAnswer>("forget_scope_facts", { id });

/** New facts landed from an indexer look (`scopes::SCOPES_EVENT`). */
export const SCOPES_EVENT = "scopes://changed";

/** Runs `handler` each time the indexers learn something new. */
export function onScopesChanged(handler: () => void): Promise<UnlistenFn> {
  return subscribe<null>(SCOPES_EVENT, () => handler());
}
