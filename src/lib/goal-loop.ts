/**
 * Goal / workboard resume helpers for autonomy levels 3–4.
 */

import type { AgentJob } from "./agent-jobs";
import type { WorkItem } from "./workboard";

export function looksIncomplete(assistantText: string): boolean {
  const t = (assistantText || "").toLowerCase();
  if (!t.trim()) return true;
  if (/next step|still need|todo|to-do|remaining|i'll continue|continue with|partially|in progress|not done yet|blocked on/i.test(t))
    return true;
  if (/all done|completed successfully|nothing else|task is complete|fully done|✓ done|marking .* done/i.test(t))
    return false;
  // Short acknowledgements without tool results often incomplete for work tasks
  if (t.length < 80 && !/done|complete|fixed|shipped/.test(t)) return true;
  return false;
}

export function buildGoalStepPrompt(opts: {
  workItem?: WorkItem | null;
  priorSummary?: string;
  stepIndex: number;
  maxSteps: number;
}): string {
  const title = opts.workItem?.title || "Continue goal";
  const detail = opts.workItem?.detail || "";
  return [
    `[Goal step ${opts.stepIndex + 1}/${opts.maxSteps}]`,
    `Task: ${title}`,
    detail ? `Detail: ${detail}` : "",
    opts.priorSummary ? `Last progress:\n${opts.priorSummary.slice(0, 1200)}` : "",
    "",
    "Continue autonomously. Use HOST_CMD / tools as needed.",
    "When fully finished, say clearly: GOAL_COMPLETE",
    "If blocked on the user, say: GOAL_BLOCKED: <reason>",
    opts.workItem?.id
      ? `If still in progress, end with WORK_UPDATE: ${opts.workItem.id} | status=in_progress`
      : "",
  ]
    .filter(Boolean)
    .join("\n");
}

export function parseGoalOutcome(
  text: string,
): "complete" | "blocked" | "continue" {
  if (/GOAL_COMPLETE/i.test(text)) return "complete";
  if (/GOAL_BLOCKED:/i.test(text)) return "blocked";
  if (looksIncomplete(text)) return "continue";
  return "complete";
}

export function nextGoalJob(
  prev: AgentJob,
  outcome: "continue" | "blocked" | "complete",
  assistantText: string,
  maxSteps: number,
): AgentJob | null {
  if (outcome !== "continue") return null;
  const step = (prev.stepIndex || 0) + 1;
  if (step >= (prev.maxSteps || maxSteps)) return null;
  if ((prev.failCount || 0) >= 3) return null;
  return {
    ...prev,
    id: undefined as unknown as string, // reassigned by enqueue
    status: "queued",
    stepIndex: step,
    priority: (prev.priority || 5) + 1,
    title: `${prev.title} · step ${step + 1}`,
    prompt: buildGoalStepPrompt({
      workItem: prev.workItemId
        ? ({ id: prev.workItemId, title: prev.title, detail: "", status: "in_progress" } as WorkItem)
        : null,
      priorSummary: assistantText.slice(0, 1500),
      stepIndex: step,
      maxSteps: prev.maxSteps || maxSteps,
    }),
    parentId: prev.id,
    resultSummary: undefined,
    lastError: undefined,
    createdAt: Date.now(),
    updatedAt: Date.now(),
  } as AgentJob;
}
