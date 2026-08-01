import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dx8p0izU.mjs";
import { _ as Plus, o as TimerReset, v as Play } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
import { t as Textarea } from "./textarea-aSaPOSKs.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/AutomationsView-D8Vmf_7P.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var SCHEDULES = [
	"once",
	"daily",
	"weekdays",
	"weekly",
	"monthly"
];
function AutomationsView() {
	const automations = useGrokHub((s) => s.automations);
	const connectors = useGrokHub((s) => s.connectors);
	const toggleAutomation = useGrokHub((s) => s.toggleAutomation);
	const runAutomation = useGrokHub((s) => s.runAutomation);
	const addAutomation = useGrokHub((s) => s.addAutomation);
	const running = useGrokHub((s) => s.running);
	const [name, setName] = (0, import_react.useState)("");
	const [instructions, setInstructions] = (0, import_react.useState)("");
	const [schedule, setSchedule] = (0, import_react.useState)("daily");
	const [time, setTime] = (0, import_react.useState)("09:00");
	function onCreate() {
		if (!name.trim() || !instructions.trim()) return;
		addAutomation({
			name: name.trim(),
			instructions: instructions.trim(),
			schedule,
			time: time.trim() || "09:00"
		});
		setName("");
		setInstructions("");
		setSchedule("daily");
		setTime("09:00");
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "space-y-5",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid gap-4 lg:grid-cols-[1fr_340px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "space-y-3",
				children: automations.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
					className: "gap-3 sm:flex-row sm:items-start sm:justify-between",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
							className: "flex flex-wrap items-center gap-2 text-sm",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(TimerReset, { className: "h-4 w-4 text-[var(--color-muted)]" }),
								a.name,
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: a.enabled ? "success" : "default",
									children: a.enabled ? "enabled" : "paused"
								})
							]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, {
							className: "mt-1",
							children: [
								a.schedule,
								" · ",
								a.time,
								" · ",
								a.runCount,
								" runs",
								a.lastRun ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
									" ",
									"· last ",
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: a.lastRun })
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
						children: [a.connectorIds.map((id) => {
							const c = connectors.find((x) => x.id === id);
							return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: c?.status === "connected" ? "info" : "default",
								children: c?.name ?? id
							}, id);
						}), a.skillIds.map((id) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, { children: id }, id))]
					})]
				})] }, a.id))
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "h-fit lg:sticky lg:top-20",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "h-4 w-4" }), "New automation"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Local scheduler (every 30s while the app is open). Due jobs run through the agent with live Grok when connected." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
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
							className: "grid grid-cols-2 gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
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
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "space-y-1.5",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
									className: "text-xs font-medium text-[var(--color-muted)]",
									children: "Time"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
									value: time,
									onChange: (e) => setTime(e.target.value),
									placeholder: "18:00"
								})]
							})]
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
							disabled: !name.trim() || !instructions.trim(),
							children: "Create automation"
						})
					]
				})]
			})]
		})
	});
}
//#endregion
export { AutomationsView };
