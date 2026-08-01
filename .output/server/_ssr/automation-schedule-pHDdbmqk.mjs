import { r as __exportAll } from "../_runtime.mjs";
import { t as __exportAll$1 } from "./rolldown-runtime-D7D4PA-g.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/automation-schedule-pHDdbmqk.js
var automation_schedule_pHDdbmqk_exports = /* @__PURE__ */ __exportAll({
	n: () => computeNextRun,
	t: () => automation_schedule_exports
});
var automation_schedule_exports = /* @__PURE__ */ __exportAll$1({
	computeNextRun: () => computeNextRun,
	dueAutomations: () => dueAutomations,
	ensureAutomationSchedule: () => ensureAutomationSchedule
});
/** Next run timestamp after `from` for a schedule + HH:mm local time. */
function computeNextRun(schedule, time, from = Date.now(), afterLast) {
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
function ensureAutomationSchedule(a, now = Date.now()) {
	if (!a.enabled) return {
		...a,
		nextRun: void 0
	};
	if (a.nextRun && a.nextRun > now) return a;
	return {
		...a,
		nextRun: computeNextRun(a.schedule, a.time, now, a.lastRun)
	};
}
function dueAutomations(list, now = Date.now()) {
	return list.filter((a) => {
		if (!a.enabled || a.nextRun == null || a.nextRun > now) return false;
		if (a.schedule === "once") return a.runCount === 0;
		return true;
	});
}
//#endregion
export { computeNextRun as n, automation_schedule_pHDdbmqk_exports as t };
