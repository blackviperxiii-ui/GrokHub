import { invoke } from "@tauri-apps/api/core";

/** Settings → Your PC → PC helpers, files, system and printers
 * (src-tauri/src/pc_files.rs and pc_files_run.rs). */
export interface PrinterRow {
  key: string;
  name: string;
  kind: string;
  enabled: boolean;
}

export interface FilesHelpers {
  downloadsTidy: boolean;
  modelDrop: boolean;
  modelsFolder: string;
  backupNudge: boolean;
  importantFolders: string[];
  resumeCode: boolean;
  codeFolders: string[];
  netLog: boolean;
  netDrops: number;
  netByHour: number[];
  netDown: boolean;
  netLastSpeedMbps: number | null;
  printAlerts: boolean;
  printCamera: boolean;
  printers: PrinterRow[];
}

export type FilesKey =
  | "downloadsTidy"
  | "modelDrop"
  | "backupNudge"
  | "resumeCode"
  | "netLog"
  | "printAlerts"
  | "printCamera";

export type FolderKey = "codeFolders" | "importantFolders";

export interface TidyRow {
  name: string;
  bytes: number;
  size: string;
  kind: string;
  why: string;
}

export interface TidyDone {
  moved: number;
  note: string;
}

export interface ResumeRow {
  name: string;
  line: string;
  staleBranches: number;
}

export function getPcFiles(): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("get_pc_files");
}

export function setPcFilesSwitch(key: FilesKey, on: boolean): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("set_pc_files_switch", { key, on });
}

export function setPcFolders(key: FolderKey, folders: string[]): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("set_pc_folders", { key, folders });
}

export function setPcModelsFolder(folder: string): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("set_pc_models_folder", { folder });
}

export function addPcPrinter(endpoint: string): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("add_pc_printer", { endpoint });
}

export function removePcPrinter(key: string): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("remove_pc_printer", { key });
}

export function setPcPrinterOn(key: string, on: boolean): Promise<FilesHelpers> {
  return invoke<FilesHelpers>("set_pc_printer_on", { key, on });
}

export function pcTidyReview(): Promise<TidyRow[]> {
  return invoke<TidyRow[]>("pc_tidy_review");
}

export function pcTidyTrash(names: string[]): Promise<TidyDone> {
  return invoke<TidyDone>("pc_tidy_trash", { names });
}

export function pcResumeRows(): Promise<ResumeRow[]> {
  return invoke<ResumeRow[]>("pc_resume_rows");
}

export function pcSpeedTest(): Promise<number> {
  return invoke<number>("pc_speed_test");
}

export function pcNetCsv(): Promise<string> {
  return invoke<string>("pc_net_csv");
}
