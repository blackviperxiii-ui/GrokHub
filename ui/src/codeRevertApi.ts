// Code tab → Roll back (src-tauri/src/code_rollback.rs).
import { call } from "./transport";

export interface CodeCommit {
  sha: string;
  short: string;
  subject: string;
  merge: boolean;
}

export interface CodeRevertPlan {
  sha: string;
  short: string;
  subject: string;
  branch: string | null;
  revertCommand: string;
  pushCommand: string;
  blocked: string | null;
}

export interface CodeReverted {
  commit: string;
  summary: string;
  output: string;
}

export function codeRevertList(): Promise<CodeCommit[]> {
  return call<CodeCommit[]>("code_revert_list");
}

export function codeRevertPlan(sha: string): Promise<CodeRevertPlan> {
  return call<CodeRevertPlan>("code_revert_plan", { sha });
}

/** Always confirmed: the two commands are the ones the confirm showed. */
export function codeRevertRun(plan: CodeRevertPlan): Promise<CodeReverted> {
  return call<CodeReverted>("code_revert_run", {
    sha: plan.sha,
    confirmed: true,
    revertCommand: plan.revertCommand,
    pushCommand: plan.pushCommand,
  });
}
