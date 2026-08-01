import type { GrokModeId, SubscriptionPlanId, UsageBucket, UsageSnapshot } from "./types";

export type PlanLimits = {
  id: SubscriptionPlanId;
  label: string;
  /** Monthly compute unit cap */
  units: number;
  /** Soft caps (for display bars); -1 = unlimited within unit budget */
  messages: number;
  imagine: number;
  automations: number;
  host: number;
  heavyAllowed: boolean;
  buildAllowed: boolean;
};

export const PLAN_LIMITS: Record<SubscriptionPlanId, PlanLimits> = {
  free: {
    id: "free",
    label: "Free",
    units: 80,
    messages: 40,
    imagine: 5,
    automations: 10,
    host: 50,
    heavyAllowed: false,
    buildAllowed: false,
  },
  super: {
    id: "super",
    label: "SuperGrok",
    units: 600,
    messages: 400,
    imagine: 60,
    automations: 120,
    host: 400,
    heavyAllowed: true,
    buildAllowed: true,
  },
  pro: {
    id: "pro",
    label: "SuperGrok Pro",
    units: 2500,
    messages: 2000,
    imagine: 250,
    automations: 500,
    host: 2000,
    heavyAllowed: true,
    buildAllowed: true,
  },
};

/** Mode-weighted compute cost per agent turn */
export const MODE_UNIT_COST: Record<GrokModeId, number> = {
  fast: 1,
  auto: 1.5,
  build: 2,
  expert: 4,
  heavy: 8,
};

export const BUCKET_UNIT_COST: Record<UsageBucket, number> = {
  message: 1, // overridden by mode
  imagine: 5,
  automation: 3,
  skill: 2,
  host: 0.25,
};

export function periodBounds(now = Date.now()): { start: number; end: number } {
  const d = new Date(now);
  const start = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1)).getTime();
  const end = new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1)).getTime();
  return { start, end };
}

export function createUsage(plan: SubscriptionPlanId = "pro", now = Date.now()): UsageSnapshot {
  const { start, end } = periodBounds(now);
  // Seed partial demo usage so the meter isn't empty
  const usedUnits = plan === "pro" ? 842 : plan === "super" ? 210 : 28;
  return {
    plan,
    periodStart: start,
    periodEnd: end,
    usedUnits,
    messages: plan === "pro" ? 186 : plan === "super" ? 72 : 12,
    imagine: plan === "pro" ? 24 : plan === "super" ? 8 : 2,
    automations: plan === "pro" ? 41 : plan === "super" ? 14 : 3,
    host: plan === "pro" ? 93 : plan === "super" ? 30 : 5,
    byMode: {
      auto: 22,
      fast: 48,
      expert: 31,
      heavy: 19,
      build: plan === "pro" ? 66 : 12,
    },
  };
}

export function ensurePeriod(u: UsageSnapshot, now = Date.now()): UsageSnapshot {
  if (now < u.periodEnd && now >= u.periodStart) return u;
  return createUsage(u.plan, now);
}

export function usagePercent(u: UsageSnapshot): number {
  const lim = PLAN_LIMITS[u.plan].units;
  if (lim <= 0) return 0;
  return Math.min(100, (u.usedUnits / lim) * 100);
}

export function usageTone(pct: number): "ok" | "warn" | "danger" {
  if (pct >= 92) return "danger";
  if (pct >= 75) return "warn";
  return "ok";
}

export function formatUnits(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`;
  if (Number.isInteger(n)) return String(n);
  return n.toFixed(1);
}

export function daysLeftInPeriod(u: UsageSnapshot, now = Date.now()): number {
  return Math.max(0, Math.ceil((u.periodEnd - now) / 86_400_000));
}

export function costFor(
  bucket: UsageBucket,
  mode?: GrokModeId,
): number {
  if (bucket === "message" || bucket === "skill") {
    return MODE_UNIT_COST[mode ?? "fast"] ?? 1;
  }
  return BUCKET_UNIT_COST[bucket];
}
