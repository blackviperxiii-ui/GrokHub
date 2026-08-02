import { r as __exportAll } from "../_runtime.mjs";
import { t as __exportAll$1 } from "./rolldown-runtime-D7D4PA-g.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/automation-schedule-fGYqzfa1.js
var automation_schedule_fGYqzfa1_exports = /* @__PURE__ */ __exportAll({
	n: () => automation_schedule_exports,
	r: () => computeNextRun,
	t: () => automationTimes
});
var automation_schedule_exports = /* @__PURE__ */ __exportAll$1({
	automationTimes: () => automationTimes,
	computeNextRun: () => computeNextRun,
	dueAutomations: () => dueAutomations,
	dueHeartbeatAutomations: () => dueHeartbeatAutomations,
	ensureAutomationSchedule: () => ensureAutomationSchedule,
	normalizeTimes: () => normalizeTimes
});
/** Normalize HH:mm strings; drop junk. */
function normalizeTimes(time, times) {
	const raw = [...Array.isArray(times) ? times : [], ...time ? [time] : []];
	const out = [];
	const seen = /* @__PURE__ */ new Set();
	for (const t of raw) {
		const m = String(t || "").trim().match(/^(\d{1,2}):(\d{2})$/);
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
function automationTimes(a) {
	return normalizeTimes(a.time, a.times);
}
/** Next run for a single HH:mm slot. */
function nextForTime(schedule, time, from, afterLast) {
	const [hh, mm] = (time || "09:00").split(":").map((x) => Number(x));
	const hour = Number.isFinite(hh) ? hh : 9;
	const minute = Number.isFinite(mm) ? mm : 0;
	const base = new Date(from);
	const candidate = new Date(base);
	candidate.setSeconds(0, 0);
	candidate.setHours(hour, minute, 0, 0);
	const minFrom = Math.max(from + 1e3, (afterLast || 0) + 3e4);
	const advanceDay = (d, n) => {
		d.setDate(d.getDate() + n);
	};
	if (schedule === "once") {
		if (candidate.getTime() > minFrom) return candidate.getTime();
		return candidate.getTime() + 365 * 24 * 36e5;
	}
	if (schedule === "heartbeat") return Math.max(minFrom, (afterLast || 0) + 5 * 6e4);
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
	return from + 24 * 36e5;
}
/** Next run timestamp after `from` — earliest among all times. */
function computeNextRun(schedule, time, from = Date.now(), afterLast, times, heartbeatEveryMin) {
	if (schedule === "heartbeat") {
		const gap = Math.max(1, Math.min(1440, heartbeatEveryMin || 5)) * 6e4;
		if (!afterLast || from - afterLast >= gap) return from;
		return afterLast + gap;
	}
	const slots = normalizeTimes(time, times);
	let best = Number.POSITIVE_INFINITY;
	for (const slot of slots) {
		const t = nextForTime(schedule, slot, from, afterLast);
		if (t < best) best = t;
	}
	return Number.isFinite(best) ? best : from + 24 * 36e5;
}
function ensureAutomationSchedule(a, now = Date.now()) {
	if (!a.enabled) return {
		...a,
		nextRun: void 0
	};
	const times = automationTimes(a);
	const primary = times[0] || a.time || "09:00";
	if (a.schedule === "heartbeat") {
		const next = computeNextRun("heartbeat", primary, now, a.lastRun, times, a.heartbeatEveryMin);
		if (a.nextRun && a.nextRun > now && a.nextRun <= next + 1e3) return {
			...a,
			times,
			time: primary
		};
		return {
			...a,
			times,
			time: primary,
			nextRun: next
		};
	}
	if (a.nextRun && a.nextRun > now) return {
		...a,
		times,
		time: primary
	};
	return {
		...a,
		times,
		time: primary,
		nextRun: computeNextRun(a.schedule, primary, now, a.lastRun, times, a.heartbeatEveryMin)
	};
}
function dueAutomations(list, now = Date.now()) {
	return list.filter((a) => {
		if (!a.enabled || a.nextRun == null || a.nextRun > now) return false;
		if (a.schedule === "once") return a.runCount === 0;
		if (a.schedule === "heartbeat") {
			const mins = Math.max(1, a.heartbeatEveryMin || 5);
			if (a.lastRun && now - a.lastRun < mins * 6e4 - 5e3) return false;
		}
		return true;
	});
}
/** Heartbeat-only automations that are ready. */
function dueHeartbeatAutomations(list, now = Date.now()) {
	return dueAutomations(list.filter((a) => a.schedule === "heartbeat"), now);
}
//#endregion
export { automation_schedule_fGYqzfa1_exports as n, computeNextRun as r, automationTimes as t };
