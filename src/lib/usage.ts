import type { GrokModeId, SubscriptionPlanId, UsageBucket, UsageSnapshot } from "./types";

export type PlanLimits = {
  id: SubscriptionPlanId;
  label: string;
  /** Monthly compute unit cap (app meter) */
  units: number;
  messages: number;
  imagine: number;
  automations: number;
  host: number;
  heavyAllowed: boolean;
  buildAllowed: boolean;
};

/**
 * Plan caps aligned to SuperGrok consumer tiers (approximate published limits).
 * Units are an internal meter: ~1 unit ≈ 1k input tokens or ~0.5k output tokens,
 * plus fixed costs for Imagine / host / automations.
 */
export const PLAN_LIMITS: Record<SubscriptionPlanId, PlanLimits> = {
  free: {
    id: "free",
    label: "Free",
    units: 100,
    messages: 50,
    imagine: 5,
    automations: 10,
    host: 80,
    heavyAllowed: false,
    buildAllowed: false,
  },
  super: {
    id: "super",
    label: "SuperGrok",
    units: 800,
    messages: 500,
    imagine: 80,
    automations: 150,
    host: 500,
    heavyAllowed: true,
    buildAllowed: true,
  },
  pro: {
    id: "pro",
    label: "SuperGrok Pro",
    units: 3000,
    messages: 2500,
    imagine: 300,
    automations: 600,
    host: 2500,
    heavyAllowed: true,
    buildAllowed: true,
  },
};

/** Mode-weighted compute cost per agent turn (fallback when no token usage returned) */
export const MODE_UNIT_COST: Record<GrokModeId, number> = {
  fast: 0.8,
  auto: 1.2,
  balanced: 1.5,
  build: 2,
  expert: 3.5,
  heavy: 7,
  max: 8,
};

export const BUCKET_UNIT_COST: Record<UsageBucket, number> = {
  message: 1,
  imagine: 6,
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

/** Clean empty usage — no demo seed. */
export function createUsage(plan: SubscriptionPlanId = "pro", now = Date.now()): UsageSnapshot {
  const { start, end } = periodBounds(now);
  return {
    plan,
    periodStart: start,
    periodEnd: end,
    usedUnits: 0,
    messages: 0,
    imagine: 0,
    automations: 0,
    host: 0,
    byMode: { auto: 0, fast: 0, balanced: 0, expert: 0, heavy: 0, max: 0, build: 0 },
    promptTokens: 0,
    completionTokens: 0,
    totalTokens: 0,
    lastPolledAt: now,
    source: "local",
    rateLimitRemaining: null,
    rateLimitLimit: null,
    rateLimitResetAt: null,
    website: null,
  };
}

export function ensurePeriod(u: UsageSnapshot, now = Date.now()): UsageSnapshot {
  if (now < u.periodEnd && now >= u.periodStart) {
    return {
      ...createUsage(u.plan, u.periodStart),
      ...u,
      promptTokens: u.promptTokens ?? 0,
      completionTokens: u.completionTokens ?? 0,
      totalTokens: u.totalTokens ?? 0,
      lastPolledAt: u.lastPolledAt ?? 0,
      source: u.source ?? "local",
      website: u.website ?? null,
    };
  }
  // New calendar period — keep website snapshot until next poll
  const next = createUsage(u.plan, now);
  return { ...next, website: u.website ?? null };
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

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1000) return `${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k`;
  return String(Math.round(n));
}

export function daysLeftInPeriod(u: UsageSnapshot, now = Date.now()): number {
  return Math.max(0, Math.ceil((u.periodEnd - now) / 86_400_000));
}

export function costFor(bucket: UsageBucket, mode?: GrokModeId): number {
  if (bucket === "message" || bucket === "skill") {
    return MODE_UNIT_COST[mode ?? "fast"] ?? 1;
  }
  return BUCKET_UNIT_COST[bucket];
}

/**
 * Convert xAI token usage into app compute units.
 * Input tokens are cheaper; output / reasoning heavier.
 */
export function unitsFromTokens(
  usage: {
    prompt_tokens?: number;
    completion_tokens?: number;
    total_tokens?: number;
  },
  mode?: GrokModeId,
): number {
  const prompt = usage.prompt_tokens ?? 0;
  const completion = usage.completion_tokens ?? Math.max(0, (usage.total_tokens ?? 0) - prompt);
  const modeMul =
    mode === "heavy" ? 1.4 : mode === "expert" ? 1.2 : mode === "build" ? 1.15 : mode === "fast" ? 0.9 : 1;
  const units = prompt / 1000 + (completion / 500) * modeMul;
  return Math.round(Math.max(units, 0.05) * 100) / 100;
}

export function parseRateLimitHeaders(headers: Headers | Record<string, string>): {
  remaining: number | null;
  limit: number | null;
  resetAt: number | null;
} {
  const get = (k: string) => {
    if (headers instanceof Headers) return headers.get(k) || headers.get(k.toLowerCase());
    return (headers as Record<string, string>)[k] || (headers as Record<string, string>)[k.toLowerCase()];
  };
  // Common patterns: x-ratelimit-remaining-requests, x-ratelimit-remaining, etc.
  const remainingRaw =
    get("x-ratelimit-remaining-requests") ||
    get("x-ratelimit-remaining") ||
    get("x-ratelimit-remaining-tokens");
  const limitRaw =
    get("x-ratelimit-limit-requests") ||
    get("x-ratelimit-limit") ||
    get("x-ratelimit-limit-tokens");
  const resetRaw =
    get("x-ratelimit-reset-requests") ||
    get("x-ratelimit-reset") ||
    get("x-ratelimit-reset-tokens");

  const remaining = remainingRaw != null && remainingRaw !== "" ? Number(remainingRaw) : null;
  const limit = limitRaw != null && limitRaw !== "" ? Number(limitRaw) : null;
  let resetAt: number | null = null;
  if (resetRaw) {
    const n = Number(resetRaw);
    if (Number.isFinite(n)) {
      // seconds vs ms vs unix
      resetAt = n > 1e12 ? n : n > 1e9 ? n * 1000 : Date.now() + n * 1000;
    }
  }
  return {
    remaining: Number.isFinite(remaining as number) ? remaining : null,
    limit: Number.isFinite(limit as number) ? limit : null,
    resetAt,
  };
}

/** Infer plan from OAuth email/name heuristics or explicit setting — default pro when SuperGrok OAuth. */
export function inferPlanFromAuth(opts: {
  hasOauth?: boolean;
  hasApiKey?: boolean;
  email?: string | null;
  name?: string | null;
}): SubscriptionPlanId {
  if (opts.hasOauth) return "pro"; // SuperGrok / X Premium device login
  if (opts.hasApiKey) return "super";
  return "free";
}
