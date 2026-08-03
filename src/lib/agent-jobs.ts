/**
 * Durable agent job queue — foundation for always-on autonomy.
 */

export type AutonomyLevel = 0 | 1 | 2 | 3 | 4;

export const AUTONOMY_LABEL: Record<AutonomyLevel, string> = {
  0: "On request",
  1: "Aware",
  2: "Helpful",
  3: "Proactive",
  4: "Hands-on",
};

export const AUTONOMY_HINT: Record<AutonomyLevel, string> = {
  0: "Only acts when you ask — no self-heal",
  1: "Notices stuck UI/stream glitches and cleans them up",
  2: "Auto-continues incomplete answers; light self-correction",
  3: "Handles small unsolicited fixes (retries, empty replies, chat errors)",
  4: "Also resumes multi-step goals / workboard when safe",
};

export type AgentJobType =
  | "chat"
  | "automation"
  | "workboard"
  | "goal_step"
  | "reflect"
  | "skill";

export type AgentJobStatus =
  | "queued"
  | "running"
  | "waiting_user"
  | "blocked"
  | "done"
  | "failed"
  | "cancelled";

export type AgentJob = {
  id: string;
  type: AgentJobType;
  status: AgentJobStatus;
  priority: number;
  title: string;
  prompt: string;
  createdAt: number;
  updatedAt: number;
  notBefore?: number;
  threadId?: string | null;
  workItemId?: string | null;
  goalId?: string | null;
  parentId?: string | null;
  automationId?: string | null;
  mode?: string;
  maxRounds?: number;
  stepIndex?: number;
  maxSteps?: number;
  failCount?: number;
  lastError?: string;
  resultSummary?: string;
  needsApproval?: boolean;
  approval?: "pending" | "granted" | "denied";
};

export type AutonomyConfig = {
  level: AutonomyLevel;
  paused: boolean;
  dailyUnitBudget: number;
  spentUnitsToday: number;
  budgetDayKey: string;
  quietStartHour: number | null;
  quietEndHour: number | null;
  autoClaimWorkboard: boolean;
  autoGoalResume: boolean;
  maxQueue: number;
  maxStepsPerGoal: number;
  circuitBreakerFails: number;
};

export type AgentQueueState = {
  jobs: AgentJob[];
  runningId: string | null;
  lastTickAt: number;
};

export function defaultAutonomyConfig(): AutonomyConfig {
  return {
    // Safe 1.0 default — user opts into Helpful/Proactive in Settings
    level: 1,
    paused: false,
    dailyUnitBudget: 0,
    spentUnitsToday: 0,
    budgetDayKey: dayKey(),
    quietStartHour: null,
    quietEndHour: null,
    autoClaimWorkboard: false,
    autoGoalResume: false,
    maxQueue: 40,
    maxStepsPerGoal: 20,
    circuitBreakerFails: 3,
  };
}

export function emptyAgentQueue(): AgentQueueState {
  return { jobs: [], runningId: null, lastTickAt: 0 };
}

export function dayKey(d = new Date()): string {
  return d.toISOString().slice(0, 10);
}

export function uidJob(prefix = "job"): string {
  return `${prefix}_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

export function normalizeAutonomy(raw: unknown): AutonomyConfig {
  const d = defaultAutonomyConfig();
  if (!raw || typeof raw !== "object") return d;
  const o = raw as Partial<AutonomyConfig>;
  const level = Math.min(4, Math.max(0, Number(o.level ?? d.level))) as AutonomyLevel;
  return {
    ...d,
    ...o,
    level,
    paused: Boolean(o.paused),
    dailyUnitBudget: Math.max(0, Number(o.dailyUnitBudget ?? 0) || 0),
    spentUnitsToday: Math.max(0, Number(o.spentUnitsToday ?? 0) || 0),
    budgetDayKey: String(o.budgetDayKey || dayKey()),
    autoClaimWorkboard: o.autoClaimWorkboard !== false,
    autoGoalResume: o.autoGoalResume !== false,
    maxQueue: Math.min(100, Math.max(5, Number(o.maxQueue ?? 40) || 40)),
    maxStepsPerGoal: Math.min(50, Math.max(3, Number(o.maxStepsPerGoal ?? 20) || 20)),
    circuitBreakerFails: Math.min(10, Math.max(2, Number(o.circuitBreakerFails ?? 3) || 3)),
  };
}

export function normalizeAgentQueue(raw: unknown): AgentQueueState {
  if (!raw || typeof raw !== "object") return emptyAgentQueue();
  const o = raw as AgentQueueState;
  const jobs = Array.isArray(o.jobs) ? o.jobs.filter(Boolean) : [];
  return {
    jobs: jobs.map((j) =>
      j.status === "running" ? { ...j, status: "queued" as const, updatedAt: Date.now() } : j,
    ),
    runningId: null,
    lastTickAt: Number(o.lastTickAt) || 0,
  };
}

export function rollBudgetDay(cfg: AutonomyConfig, now = Date.now()): AutonomyConfig {
  const key = dayKey(new Date(now));
  if (cfg.budgetDayKey === key) return cfg;
  return { ...cfg, budgetDayKey: key, spentUnitsToday: 0 };
}

export function inQuietHours(cfg: AutonomyConfig, now = Date.now()): boolean {
  if (cfg.quietStartHour == null || cfg.quietEndHour == null) return false;
  const h = new Date(now).getHours();
  const s = cfg.quietStartHour;
  const e = cfg.quietEndHour;
  if (s === e) return false;
  if (s < e) return h >= s && h < e;
  return h >= s || h < e;
}

export function budgetOk(cfg: AutonomyConfig): boolean {
  if (!cfg.dailyUnitBudget) return true;
  return cfg.spentUnitsToday < cfg.dailyUnitBudget;
}

export function sortJobs(jobs: AgentJob[]): AgentJob[] {
  return [...jobs].sort((a, b) => {
    if (a.priority !== b.priority) return b.priority - a.priority;
    return a.createdAt - b.createdAt;
  });
}

export function pickNextJob(
  state: AgentQueueState,
  cfg: AutonomyConfig,
  now = Date.now(),
): AgentJob | null {
  if (cfg.paused || cfg.level < 1) return null;
  if (state.runningId) return null;
  if (inQuietHours(cfg, now) && cfg.level < 4) return null;
  if (!budgetOk(cfg)) return null;
  const queued = sortJobs(
    state.jobs.filter((j) => {
      if (j.status !== "queued") return false;
      if (j.notBefore && j.notBefore > now) return false;
      if (j.needsApproval && j.approval !== "granted") return false;
      return true;
    }),
  );
  return queued[0] || null;
}

export function enqueueJob(
  state: AgentQueueState,
  input: Omit<AgentJob, "id" | "createdAt" | "updatedAt" | "status"> & {
    id?: string;
    status?: AgentJobStatus;
  },
  maxQueue = 40,
): AgentQueueState {
  const now = Date.now();
  const job: AgentJob = {
    id: input.id || uidJob(),
    type: input.type,
    status: input.status || "queued",
    priority: input.priority ?? 0,
    title: input.title,
    prompt: input.prompt,
    createdAt: now,
    updatedAt: now,
    notBefore: input.notBefore,
    threadId: input.threadId,
    workItemId: input.workItemId,
    goalId: input.goalId,
    parentId: input.parentId,
    automationId: input.automationId,
    mode: input.mode,
    maxRounds: input.maxRounds,
    stepIndex: input.stepIndex,
    maxSteps: input.maxSteps,
    failCount: input.failCount || 0,
    lastError: input.lastError,
    resultSummary: input.resultSummary,
    needsApproval: input.needsApproval,
    approval: input.approval,
  };
  let jobs = [job, ...state.jobs.filter((j) => j.id !== job.id)];
  if (jobs.length > maxQueue) {
    const active = jobs.filter((j) => !["done", "cancelled", "failed"].includes(j.status));
    const done = jobs.filter((j) => ["done", "cancelled", "failed"].includes(j.status));
    jobs = [...active, ...done].slice(0, maxQueue);
  }
  return { ...state, jobs };
}

export function updateJob(
  state: AgentQueueState,
  id: string,
  patch: Partial<AgentJob>,
): AgentQueueState {
  let runningId = state.runningId;
  if (patch.status) {
    if (patch.status === "running") runningId = id;
    else if (state.runningId === id) runningId = null;
  }
  return {
    ...state,
    runningId,
    jobs: state.jobs.map((j) =>
      j.id === id ? { ...j, ...patch, updatedAt: Date.now() } : j,
    ),
  };
}

export function approveJob(state: AgentQueueState, id: string, grant: boolean): AgentQueueState {
  return updateJob(state, id, {
    approval: grant ? "granted" : "denied",
    status: grant ? "queued" : "cancelled",
    needsApproval: false,
  });
}

export function queueStats(state: AgentQueueState) {
  const c = { queued: 0, running: 0, waiting: 0, failed: 0, done: 0 };
  for (const j of state.jobs) {
    if (j.status === "queued") c.queued++;
    else if (j.status === "running") c.running++;
    else if (j.status === "waiting_user" || j.status === "blocked") c.waiting++;
    else if (j.status === "failed") c.failed++;
    else if (j.status === "done" || j.status === "cancelled") c.done++;
  }
  return c;
}

export function shouldQueueWhenBusy(level: AutonomyLevel): boolean {
  return level >= 2;
}

export function shouldAutoClaimWorkboard(cfg: AutonomyConfig): boolean {
  return !cfg.paused && cfg.level >= 3 && cfg.autoClaimWorkboard;
}

export function shouldAutoGoalResume(cfg: AutonomyConfig): boolean {
  return !cfg.paused && cfg.level >= 4 && cfg.autoGoalResume;
}
