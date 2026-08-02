import type { Automation, AutomationSchedule } from "./types";

/** Normalize HH:mm strings; drop junk. */
export function normalizeTimes(time?: string, times?: string[]): string[] {
  const raw = [
    ...(Array.isArray(times) ? times : []),
    ...(time ? [time] : []),
  ];
  const out: string[] = [];
  const seen = new Set<string>();
  for (const t of raw) {
    const m = String(t || "")
      .trim()
      .match(/^(\d{1,2}):(\d{2})$/);
    if (!m) continue;
    let h = Number(m[1]);
    let min = Number(m[2]);
    if (!Number.isFinite(h) || !Number.isFinite(min)) continue;
    h = Math.min(23, Math.max(0, h));
    min = Math.min(59, Math.max(0, min));
    const key = `${String(h).padStart(2, "0")}:${String(min).padStart(2, "0")}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(key);
  }
  out.sort();
  return out.length ? out : ["09:00"];
}

export function automationTimes(a: Pick<Automation, "time" | "times">): string[] {
  return normalizeTimes(a.time, a.times);
}

/** Next run for a single HH:mm slot. */
function nextForTime(
  schedule: AutomationSchedule,
  time: string,
  from: number,
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
    return candidate.getTime() + 365 * 24 * 3600_000;
  }

  if (schedule === "heartbeat") {
    const every = 5 * 60_000; // placeholder; real gap handled in ensure
    return Math.max(minFrom, (afterLast || 0) + every);
  }

  for (let i = 0; i < 400; i++) {
    const t = candidate.getTime();
    if (t > minFrom) {
      const day = candidate.getDay();
      if (schedule === "daily") return t;
      if (schedule === "weekdays" && day >= 1 && day <= 5) return t;
      if (schedule === "weekly" && day === 1) return t;
      if (schedule === "monthly" && candidate.getDate() === 1) return t;
    }
    advanceDay(candidate, 1);
    candidate.setHours(hour, minute, 0, 0);
  }
  return from + 24 * 3600_000;
}

/** Next run timestamp after `from` — earliest among all times. */
export function computeNextRun(
  schedule: AutomationSchedule,
  time: string,
  from = Date.now(),
  afterLast?: number,
  times?: string[],
  heartbeatEveryMin?: number,
): number {
  if (schedule === "heartbeat") {
    const mins = Math.max(1, Math.min(24 * 60, heartbeatEveryMin || 5));
    const gap = mins * 60_000;
    // If interval already elapsed, become due immediately
    if (!afterLast || from - afterLast >= gap) {
      return from;
    }
    return afterLast + gap;
  }
  const slots = normalizeTimes(time, times);
  let best = Number.POSITIVE_INFINITY;
  for (const slot of slots) {
    const t = nextForTime(schedule, slot, from, afterLast);
    if (t < best) best = t;
  }
  return Number.isFinite(best) ? best : from + 24 * 3600_000;
}

export function ensureAutomationSchedule(a: Automation, now = Date.now()): Automation {
  if (!a.enabled) return { ...a, nextRun: undefined };
  const times = automationTimes(a);
  const primary = times[0] || a.time || "09:00";
  // Heartbeat: always keep a nextRun based on lastRun + interval
  if (a.schedule === "heartbeat") {
    const next = computeNextRun(
      "heartbeat",
      primary,
      now,
      a.lastRun,
      times,
      a.heartbeatEveryMin,
    );
    if (a.nextRun && a.nextRun > now && a.nextRun <= next + 1_000) return { ...a, times, time: primary };
    return { ...a, times, time: primary, nextRun: next };
  }
  if (a.nextRun && a.nextRun > now) return { ...a, times, time: primary };
  return {
    ...a,
    times,
    time: primary,
    nextRun: computeNextRun(a.schedule, primary, now, a.lastRun, times, a.heartbeatEveryMin),
  };
}

export function dueAutomations(list: Automation[], now = Date.now()): Automation[] {
  return list.filter((a) => {
    if (!a.enabled || a.nextRun == null || a.nextRun > now) return false;
    if (a.schedule === "once") return a.runCount === 0;
    if (a.schedule === "heartbeat") {
      const mins = Math.max(1, a.heartbeatEveryMin || 5);
      if (a.lastRun && now - a.lastRun < mins * 60_000 - 5_000) return false;
    }
    return true;
  });
}

/** Heartbeat-only automations that are ready. */
export function dueHeartbeatAutomations(list: Automation[], now = Date.now()): Automation[] {
  return dueAutomations(
    list.filter((a) => a.schedule === "heartbeat"),
    now,
  );
}
