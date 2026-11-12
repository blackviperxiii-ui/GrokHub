// Settings → Privacy & data → Storage (src-tauri/src/storage.rs).
import { call } from "./transport";

export interface FolderRow {
  /** What Open sends back: a folder's name, or "models". */
  key: string;
  line: string;
}

export interface StorageView {
  filesLine: string;
  canReset: boolean;
  folders: FolderRow[];
  models: FolderRow;
  waiting: string | null;
  /** What Bring them over would do: "from → to", one per folder. */
  moves: string[];
}

export interface StorageAnswer {
  view: StorageView;
  status: string;
}

export const getStorage = () => call<StorageView>("get_storage");
export const storageReset = () => call<StorageAnswer>("storage_reset");
export const storageBringOver = () => call<StorageAnswer>("storage_bring_over");
export const storageOpen = (key: string) => call<void>("storage_open", { key });
