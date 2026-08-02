import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { t as automationTimes } from "./automation-schedule-fGYqzfa1.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dgqzm_EC.mjs";
import { _ as Plus, j as HeartPulse, n as X, o as TimerReset, v as Play } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { t as Textarea } from "./textarea-aSaPOSKs.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/AutomationsView-B-7LoRXy.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var SCHEDULES = [
	"once",
	"daily",
	"weekdays",
	"weekly",
	"monthly",
	"heartbeat"
];
function parseTimesInput(raw) {
	return raw.split(/[,\s]+/).map((t) => t.trim()).filter(Boolean);
}
function AutomationsView() {
	const automations = useGrokHub((s) => s.automations);
	const connectors = useGrokHub((s) => s.connectors);
	const toggleAutomation = useGrokHub((s) => s.toggleAutomation);
	const runAutomation = useGrokHub((s) => s.runAutomation);
	const addAutomation = useGrokHub((s) => s.addAutomation);
	const running = useGrokHub((s) => s.running);
	const heartbeatAt = useGrokHub((s) => s.heartbeatAt);
	const [name, setName] = (0, import_react.useState)("");
	const [instructions, setInstructions] = (0, import_react.useState)("");
	const [schedule, setSchedule] = (0, import_react.useState)("daily");
	const [times, setTimes] = (0, import_react.useState)(["09:00"]);
	const [timeDraft, setTimeDraft] = (0, import_react.useState)("");
	const [heartbeatEveryMin, setHeartbeatEveryMin] = (0, import_react.useState)(5);
	const isHeartbeat = schedule === "heartbeat";
	function addTimeSlot() {
		const parsed = parseTimesInput(timeDraft);
		if (!parsed.length) return;
		setTimes((prev) => Array.from(/* @__PURE__ */ new Set([...prev, ...parsed])).sort());
		setTimeDraft("");
	}
	function onCreate() {
		if (!name.trim() || !instructions.trim()) return;
		const slots = times.length ? times : ["09:00"];
		addAutomation({
			name: name.trim(),
			instructions: instructions.trim(),
			schedule,
			time: slots[0],
			times: slots,
			heartbeatEveryMin: isHeartbeat ? heartbeatEveryMin : void 0
		});
		setName("");
		setInstructions("");
		setSchedule("daily");
		setTimes(["09:00"]);
		setHeartbeatEveryMin(5);
	}
	const sorted = (0, import_react.useMemo)(() => [...automations].sort((a, b) => {
		if (a.enabled !== b.enabled) return a.enabled ? -1 : 1;
		return (a.nextRun || 0) - (b.nextRun || 0);
	}), [automations]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "flex flex-wrap items-center gap-3 py-3 text-xs text-[var(--color-muted)]",
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HeartPulse, { className: "h-4 w-4 text-[var(--color-info)]" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
					"Heartbeat ",
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: heartbeatAt }),
					" · runs",
					" ",
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "font-medium text-[var(--color-fg)]",
						children: "heartbeat"
					}),
					" automations and the clock scheduler (every 30s while open)."
				] }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
					variant: "info",
					children: [automations.filter((a) => a.enabled && a.schedule === "heartbeat").length, " on heartbeat"]
				})
			]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid gap-4 lg:grid-cols-[1fr_360px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "space-y-3",
				children: [sorted.map((a) => {
					const slots = automationTimes(a);
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
						className: "gap-3 sm:flex-row sm:items-start sm:justify-between",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
								className: "flex flex-wrap items-center gap-2 text-sm",
								children: [
									a.schedule === "heartbeat" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HeartPulse, { className: "h-4 w-4 text-[var(--color-info)]" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TimerReset, { className: "h-4 w-4 text-[var(--color-muted)]" }),
									a.name,
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
										variant: a.enabled ? "success" : "default",
										children: a.enabled ? "enabled" : "paused"
									}),
									a.schedule === "heartbeat" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
										variant: "info",
										children: "heartbeat"
									})
								]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, {
								className: "mt-1",
								children: [
									a.schedule === "heartbeat" ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
										"every ",
										a.heartbeatEveryMin || 5,
										"m on heartbeat · ",
										a.runCount,
										" runs"
									] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
										a.schedule,
										" · ",
										slots.join(", "),
										" · ",
										a.runCount,
										" runs"
									] }),
									a.lastRun ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
										" ",
										"· last ",
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: a.lastRun })
									] }) : null,
									a.nextRun && a.enabled ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
										" ",
										"· next ",
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: a.nextRun })
									] }) : null
								]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex shrink-0 gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								size: "sm",
								variant: "secondary",
								onClick: () => toggleAutomation(a.id),
								children: a.enabled ? "Pause" : "Enable"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
								size: "sm",
								disabled: !a.enabled || running,
								onClick: () => void runAutomation(a.id),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "h-3.5 w-3.5" }), "Run now"]
							})]
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "space-y-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "text-sm text-[var(--color-muted)]",
							children: a.instructions
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex flex-wrap gap-1.5",
							children: [
								a.connectorIds.map((id) => {
									const c = connectors.find((x) => x.id === id);
									return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
										variant: c?.status === "connected" ? "info" : "default",
										children: c?.name ?? id
									}, id);
								}),
								a.skillIds.map((id) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: id }, id)),
								a.schedule !== "heartbeat" && slots.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									className: "font-mono",
									children: t
								}, t))
							]
						})]
					})] }, a.id);
				}), sorted.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
					className: "py-10 text-center text-sm text-[var(--color-muted)]",
					children: "No automations yet. Create one with multiple times or attach it to the heartbeat."
				}) })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "h-fit lg:sticky lg:top-20",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "h-4 w-4" }), "New automation"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Clock schedules support several HH:mm times. Heartbeat runs with the app pulse (configurable interval)." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
					className: "space-y-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Name"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: name,
								onChange: (e) => setName(e.target.value),
								placeholder: "Evening cash check"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Schedule"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("select", {
								value: schedule,
								onChange: (e) => setSchedule(e.target.value),
								className: "flex h-10 w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 text-sm text-[var(--color-fg)]",
								children: SCHEDULES.map((s) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("option", {
									value: s,
									children: s
								}, s))
							})]
						}),
						isHeartbeat ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Min minutes between heartbeat runs"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								type: "number",
								min: 1,
								max: 1440,
								value: heartbeatEveryMin,
								onChange: (e) => setHeartbeatEveryMin(Number(e.target.value) || 5)
							})]
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
									className: "text-xs font-medium text-[var(--color-muted)]",
									children: "Run times (multiple)"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "flex flex-wrap gap-1.5",
									children: times.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
										className: "inline-flex items-center gap-1 rounded-full border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-0.5 font-mono text-xs",
										children: [t, /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
											type: "button",
											className: "text-[var(--color-muted)] hover:text-[var(--color-fg)]",
											onClick: () => setTimes((prev) => prev.filter((x) => x !== t)),
											"aria-label": `Remove ${t}`,
											children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-3 w-3" })
										})]
									}, t))
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "flex gap-2",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
										value: timeDraft,
										onChange: (e) => setTimeDraft(e.target.value),
										placeholder: "09:00 or 09:00, 12:30, 18:00",
										onKeyDown: (e) => {
											if (e.key === "Enter") {
												e.preventDefault();
												addTimeSlot();
											}
										}
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
										type: "button",
										variant: "secondary",
										onClick: addTimeSlot,
										children: "Add"
									})]
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Instructions"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Textarea, {
								value: instructions,
								onChange: (e) => setInstructions(e.target.value),
								placeholder: "@Gmail summarize unpaid bills and draft replies…"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							className: "w-full",
							onClick: onCreate,
							disabled: !name.trim() || !instructions.trim() || !isHeartbeat && !times.length,
							children: "Create automation"
						})
					]
				})]
			})]
		})]
	});
}
//#endregion
export { AutomationsView };
