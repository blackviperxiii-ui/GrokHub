// Automations → Routines: saved routines and their runs (src-tauri/src/routines_page.rs).
import { call } from "./transport";
import type { RunResult } from "./api";

export type RoutineConsent = "read_only" | "allowed" | "edited" | "not_allowed";

/** A fix from a repair that kept working, ready to apply. */
export interface RoutineFix {
  step: string;
  /** The step's arguments now and with the fix, as JSON. */
  before: string;
  after: string;
  /** Runs the repair worked in. */
  seen: number;
}

export interface RoutineRow {
  name: string;
  about: string;
  /** `daily 09:00`, `webhook deploys`, `new *.csv in /home/user/in`, `chat`. */
  triggers: string[];
  steps: number;
  /** The steps that change something, as `id: tool`. */
  changes: string[];
  /** Null when the routine doesn't load; `problems` says why. */
  consent: RoutineConsent | null;
  on: boolean;
  runs: number;
  modelCalls: number;
  lastAtMs: number | null;
  lastOk: boolean | null;
  lastLine: string | null;
  problems: string[];
  fix: RoutineFix | null;
}

export const listRoutines = () => call<RoutineRow[]>("list_routines");
export const setRoutineOn = (name: string, on: boolean) => call<RoutineRow[]>("set_routine_on", { name, on });
export const allowRoutine = (name: string) => call<RoutineRow[]>("allow_routine", { name });
export const revokeRoutine = (name: string) => call<RoutineRow[]>("revoke_routine", { name });
export const undoRoutine = (name: string) => call<string>("undo_routine", { name });
export const runRoutineNow = (name: string) => call<RunResult>("run_routine_now", { name });
export const applyRoutineFix = (name: string) => call<string>("apply_routine_fix", { name });
export const dismissRoutineFix = (name: string) => call<RoutineRow[]>("dismiss_routine_fix", { name });
