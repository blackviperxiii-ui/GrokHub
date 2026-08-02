import { p as getModesWithCatalog } from "./version-xnMxJHGr.mjs";
import { P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dgqzm_EC.mjs";
import { G as Cable, J as ArrowRight, M as HardDrive, X as Activity, k as Image, l as Sparkles, o as TimerReset, t as Zap } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { n as UsageMeterPanel } from "./UsageMeter-dfHqEUzV.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/CommandView-Baq5m5aK.js
var import_jsx_runtime = require_jsx_runtime();
function CommandView() {
	const connectors = useGrokHub((s) => s.connectors);
	const skills = useGrokHub((s) => s.skills);
	const automations = useGrokHub((s) => s.automations);
	const activity = useGrokHub((s) => s.activity);
	const agents = useGrokHub((s) => s.agents);
	const mode = useGrokHub((s) => s.mode);
	const setMode = useGrokHub((s) => s.setMode);
	const modelCatalog = useGrokHub((s) => s.modelCatalog);
	const setNav = useGrokHub((s) => s.setNav);
	const runAutomation = useGrokHub((s) => s.runAutomation);
	const sendChat = useGrokHub((s) => s.sendChat);
	const connected = connectors.filter((c) => c.status === "connected").length;
	const enabledSkills = skills.filter((s) => s.enabled).length;
	const activeAutos = automations.filter((a) => a.enabled).length;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-6",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "grid gap-3 sm:grid-cols-2 xl:grid-cols-4",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatCard, {
						label: "Connectors",
						value: `${connected}/${connectors.length}`,
						hint: "OAuth tools live",
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Cable, { className: "h-4 w-4" }),
						onClick: () => setNav("connectors")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatCard, {
						label: "Skills",
						value: `${enabledSkills} on`,
						hint: `${skills.length} total`,
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-4 w-4" }),
						onClick: () => setNav("skills")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatCard, {
						label: "Automations",
						value: String(activeAutos),
						hint: "scheduled + triggers",
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(TimerReset, { className: "h-4 w-4" }),
						onClick: () => setNav("automations")
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(StatCard, {
						label: "Agents",
						value: String(agents.filter((a) => a.status !== "offline").length),
						hint: "online roster",
						icon: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Zap, { className: "h-4 w-4" }),
						onClick: () => setNav("agents")
					})
				]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterPanel, { compact: true }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("section", {
				className: "grid gap-4 lg:grid-cols-[1.35fr_1fr]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Grok modes" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Baked in exactly like the web picker — Auto, Fast, Expert, Heavy, Build. Costs scale by mode." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
					className: "grid gap-2 sm:grid-cols-2",
					children: getModesWithCatalog(modelCatalog).map((m) => {
						const selected = m.id === mode;
						const costHint = m.id === "heavy" ? "8u" : m.id === "expert" ? "4u" : m.id === "build" ? "2u" : m.id === "auto" ? "1.5u" : "1u";
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							onClick: () => setMode(m.id),
							className: selected ? "rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-3 text-left" : "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 text-left hover:border-[var(--color-border-strong)]",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center gap-2 text-sm font-medium",
								children: [
									m.label,
									m.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
										className: "text-[10px]",
										children: "Beta"
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "ml-auto font-mono text-[10px] text-[var(--color-subtle)]",
										children: costHint
									})
								]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mt-0.5 text-xs text-[var(--color-muted)]",
								children: m.subtitle
							})]
						}, m.id);
					})
				})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, { children: "Quick actions" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Agent, host desktop, Imagine, automations." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
					className: "grid gap-2",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							className: "h-auto justify-start px-4 py-3 text-left",
							onClick: () => setNav("desktop"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "h-4 w-4 text-[var(--color-muted)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: "Desktop host"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-xs text-[var(--color-subtle)]",
									children: "Unsandboxed CLI · files · apps"
								})] })]
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							className: "h-auto justify-start px-4 py-3 text-left",
							onClick: () => {
								setNav("chat");
								sendChat("/morning");
							},
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium text-[var(--color-fg)]",
								children: "Run morning brief"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs text-[var(--color-subtle)]",
								children: "Uses active mode routing"
							})] })
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							className: "h-auto justify-start px-4 py-3 text-left",
							onClick: () => setNav("imagine"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Image, { className: "h-4 w-4 text-[var(--color-muted)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: "Open Imagine"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-xs text-[var(--color-subtle)]",
									children: "5 units per render"
								})] })]
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							className: "h-auto justify-start px-4 py-3 text-left",
							onClick: () => {
								const id = automations.find((a) => a.enabled)?.id;
								if (id) runAutomation(id);
							},
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium text-[var(--color-fg)]",
								children: "Run top automation"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs text-[var(--color-subtle)]",
								children: "First enabled job"
							})] })
						})
					]
				})] })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "flex-row items-center justify-between gap-3 space-y-0",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Activity, { className: "h-4 w-4" }), "Activity"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Runs across modes, host CLI, Imagine, skills, and chat." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					variant: "ghost",
					size: "sm",
					onClick: () => setNav("chat"),
					children: ["Open agent", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ArrowRight, { className: "h-3.5 w-3.5" })]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
				className: "divide-y divide-[var(--color-border)]",
				children: activity.slice(0, 8).map((item) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", {
					className: "flex items-start justify-between gap-3 py-3 first:pt-0 last:pb-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex flex-wrap items-center gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-sm font-medium",
								children: item.title
							}), item.status && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: item.status === "success" ? "success" : item.status === "running" ? "info" : item.status === "failed" ? "danger" : "default",
								children: item.status
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
							className: "mt-0.5 truncate text-sm text-[var(--color-muted)]",
							children: item.detail
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, {
						ts: item.ts,
						className: "shrink-0 tabular text-xs text-[var(--color-subtle)]"
					})]
				}, item.id))
			}) })] })
		]
	});
}
function StatCard({ label, value, hint, icon, onClick }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		onClick,
		className: "rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-panel)] p-4 text-left shadow-[var(--shadow-soft)] transition-colors hover:border-[var(--color-border-strong)]",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mb-3 flex items-center justify-between text-[var(--color-muted)]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-xs font-medium uppercase tracking-wide",
					children: label
				}), icon]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "text-2xl font-semibold tracking-tight tabular",
				children: value
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-1 text-xs text-[var(--color-subtle)]",
				children: hint
			})
		]
	});
}
//#endregion
export { CommandView };
