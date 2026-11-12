// Settings → Permissions → "What Loadout can read" (src-tauri/src/scopes.rs).
import { call } from "./transport";

export interface GrantedScope {
  id: string;
  title: string;
  hint: string;
  /** The whole folder path, for hover. */
  detail: string | null;
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
