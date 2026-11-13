// Settings → Behavior → what the app changed on its own (src-tauri/src/change_rows.rs).
import { call } from "./transport";

export interface ChangeRow {
  id: string;
  label: string;
  act: "undo" | "restore";
  /** The app's own change is still open, so Keep is offered too. */
  keep: boolean;
}

export interface ChangeGroup {
  kind: string;
  title: string;
  rows: ChangeRow[];
}

export interface ChangeRowsView {
  heading: string;
  empty: string;
  groups: ChangeGroup[];
}

export interface ChangeAnswer {
  view: ChangeRowsView;
  status: string;
}

export const getChangeRows = () => call<ChangeRowsView>("get_change_rows");
export const changeRowUndo = (kind: string, id: string) => call<ChangeAnswer>("change_row_undo", { kind, id });
export const changeRowKeep = (kind: string, id: string) => call<ChangeAnswer>("change_row_keep", { kind, id });
export const changeRowRestore = (kind: string, id: string) => call<ChangeAnswer>("change_row_restore", { kind, id });
