/** Built-in advanced settings. Not user-editable. */

import type { AutonomyConfig } from "./agent-jobs";

export type LockedDesktop = {
  startMinimized: boolean;
  launchOnLogin: boolean;
  wayland: boolean;
  tray: boolean;
  globalHotkey: string;
  confirmHostCommands: boolean;
  confirmDestructiveOnly: boolean;
  selfModifyEnabled: boolean;
  hostSafeMode: boolean;
};

export type LockedAgentPrefs = {
  temperature: number;
  hostToolsEnabled: boolean;
  connectorToolsEnabled: boolean;
  computerUseEnabled: boolean;
};

export const LOCKED = {
  autonomy: {
    level: 4 as const,
    autoClaimWorkboard: true,
    autoGoalResume: true,
    dailyUnitBudget: 0,
    quietStartHour: null as number | null,
    quietEndHour: null as number | null,
    maxQueue: 40,
    maxStepsPerGoal: 20,
    circuitBreakerFails: 3,
  },
  desktop: {
    startMinimized: false,
    launchOnLogin: true,
    wayland: true,
    tray: true,
    globalHotkey: "Super+Space",
    confirmHostCommands: true,
    confirmDestructiveOnly: true,
    selfModifyEnabled: false,
    hostSafeMode: true,
  } satisfies LockedDesktop,
  agentPrefs: {
    temperature: 0.7,
    hostToolsEnabled: true,
    connectorToolsEnabled: true,
    computerUseEnabled: true,
  } satisfies LockedAgentPrefs,
  hostAllowlist: [] as string[],
};

export type SettingsCat = "account" | "devices" | "app";

export function resolveSettingsCat(cat: string | null | undefined): SettingsCat {
  if (cat === "devices") return "devices";
  if (cat === "account") return "account";
  return "app";
}

export function applyLockedAutonomy(cfg: AutonomyConfig): AutonomyConfig {
  return {
    ...cfg,
    level: LOCKED.autonomy.level,
    autoClaimWorkboard: LOCKED.autonomy.autoClaimWorkboard,
    autoGoalResume: LOCKED.autonomy.autoGoalResume,
    dailyUnitBudget: LOCKED.autonomy.dailyUnitBudget,
    quietStartHour: LOCKED.autonomy.quietStartHour,
    quietEndHour: LOCKED.autonomy.quietEndHour,
    maxQueue: LOCKED.autonomy.maxQueue,
    maxStepsPerGoal: LOCKED.autonomy.maxStepsPerGoal,
    circuitBreakerFails: LOCKED.autonomy.circuitBreakerFails,
    paused: Boolean(cfg.paused),
    spentUnitsToday: cfg.spentUnitsToday,
    budgetDayKey: cfg.budgetDayKey,
  };
}

export function applyLockedDesktop(_raw?: Partial<LockedDesktop> | null): LockedDesktop {
  return { ...LOCKED.desktop };
}

export function applyLockedAgentPrefs(raw: {
  temperature?: number;
  hostToolsEnabled?: boolean;
  connectorToolsEnabled?: boolean;
  computerUseEnabled?: boolean;
  memoryNotes?: string;
}): {
  temperature: number;
  hostToolsEnabled: boolean;
  connectorToolsEnabled: boolean;
  computerUseEnabled: boolean;
  memoryNotes: string;
} {
  return {
    ...LOCKED.agentPrefs,
    memoryNotes: typeof raw?.memoryNotes === "string" ? raw.memoryNotes : "",
  };
}
