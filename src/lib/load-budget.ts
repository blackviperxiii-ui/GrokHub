/**
 * Hard caps so Max mode + Hands-on autonomy cannot pile tool loops,
 * reflections, and desktop chores until the machine OOMs.
 */

export const LOAD_BUDGET = {
  maxToolRounds: {
    max: 8,
    heavy: 8,
    build: 8,
    default: 6,
  },
  maxFinishNudges: {
    max: 3,
    default: 2,
  },
  maxHostNudges: 3,
  maxHostCmdsPerRound: 3,
  maxParallelHost: 2,
  reflectEveryTurns: 6,
  selfImproveEveryTurns: 24,
  housekeepingMinMs: 60_000,
  maxFreeRoamChores: 1,
} as const;

export function toolRoundBudget(mode: string): number {
  const m = String(mode || "").toLowerCase();
  if (m === "max") return LOAD_BUDGET.maxToolRounds.max;
  if (m === "heavy") return LOAD_BUDGET.maxToolRounds.heavy;
  if (m === "build") return LOAD_BUDGET.maxToolRounds.build;
  return LOAD_BUDGET.maxToolRounds.default;
}

export function finishNudgeBudget(mode: string): number {
  const m = String(mode || "").toLowerCase();
  if (m === "max" || m === "heavy") return LOAD_BUDGET.maxFinishNudges.max;
  return LOAD_BUDGET.maxFinishNudges.default;
}

export function shouldReflectThisTurn(totalTurns: number): boolean {
  const n = Math.max(0, Number(totalTurns) || 0);
  return n > 0 && n % LOAD_BUDGET.reflectEveryTurns === 0;
}

export function shouldSelfImproveThisTurn(totalTurns: number): boolean {
  const n = Math.max(0, Number(totalTurns) || 0);
  return n > 0 && n % LOAD_BUDGET.selfImproveEveryTurns === 0;
}

/** Run at most `limit` async workers at a time. */
export async function mapPool<T, R>(
  items: T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const n = Math.max(1, Math.floor(limit) || 1);
  const out: R[] = new Array(items.length);
  let next = 0;
  const workers = Array.from({ length: Math.min(n, items.length) }, async () => {
    while (next < items.length) {
      const i = next;
      next += 1;
      out[i] = await fn(items[i]!, i);
    }
  });
  await Promise.all(workers);
  return out;
}
