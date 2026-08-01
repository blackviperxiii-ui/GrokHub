import type { Automation, AutomationSchedule } from "./types";

/** Next run timestamp after `from` for a schedule + HH:mm local time. */
export function computeNextRun(
  schedule: AutomationSchedule,
  time: string,
  from = Date.now(),
  afterLast?: number,
): number {
  const [hh, mm] = (time || "09:00").split(":").map((x) => Number(x));
  const hour = Number.isFinite(hh) ? hh : 9;
  const minute = Number.isFinite(mm) ? mm : 0;
  const base = new Date(from);
  const candidate = new Date(base);
  candidate.setSeconds(0, 0);
  candidate.setHours(hour, minute, 0, 0);

  const minFrom = Math.max(from + 1_000, (afterLast || 0) + 30_000);

  const advanceDay = (d: Date, n: number) => {
    d.setDate(d.getDate() + n);
  };

  if (schedule === "once") {
    if (candidate.getTime() > minFrom) return candidate.getTime();
    // already passed — leave far future so it won't auto-fire again
    return candidate.getTime() + 365 * 24 * 3600_000;
  }

  // Find next matching slot
  for (let i = 0; i < 400; i++) {
    const t = candidate.getTime();
    if (t > minFrom) {
      const day = candidate.getDay(); // 0 Sun
      if (schedule === "daily") return t;
      if (schedule === "weekdays" && day >= 1 && day <= 5) return t;
      if (schedule === "weekly" && day === 1) return t; // Mondays
      if (schedule === "monthly" && candidate.getDate() === 1) return t;
    }
    advanceDay(candidate, 1);
    candidate.setHours(hour, minute, 0, 0);
  }
  return from + 24 * 3600_000;
}

export function ensureAutomationSchedule(a: Automation, now = Date.now()): Automation {
  if (!a.enabled) return { ...a, nextRun: undefined };
  if (a.nextRun && a.nextRun > now) return a;
  return {
    ...a,
    nextRun: computeNextRun(a.schedule, a.time, now, a.lastRun),
  };
}

export function dueAutomations(list: Automation[], now = Date.now()): Automation[] {
  return list.filter((a) => {
    if (!a.enabled || a.nextRun == null || a.nextRun > now) return false;
    if (a.schedule === "once") return a.runCount === 0;
    return true;
  });
}
