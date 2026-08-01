import { P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dx8p0izU.mjs";
import { i as Users } from "../_libs/lucide-react.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/AgentsView-CFZbU5El.js
var import_jsx_runtime = require_jsx_runtime();
function AgentsView() {
	const agents = useGrokHub((s) => s.agents);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Users, { className: "h-4 w-4" }), "Multi-agent roster"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "OpenClaw-style named agents with a lean xAI model router. Primary stays cheap; build model for code; escalate only when needed." })] }) }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-3 sm:grid-cols-2",
				children: agents.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center gap-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-sm font-semibold",
							style: { color: a.color },
							children: a.name.slice(0, 1)
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
							className: "text-sm",
							children: a.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: a.role })] })]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						variant: a.status === "working" ? "info" : a.status === "idle" ? "success" : "default",
						children: a.status
					})]
				}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
					className: "space-y-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-between rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: "Model"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-mono text-xs text-[var(--color-fg)]",
							children: a.model
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-center justify-between rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: "Active tasks"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "tabular text-xs text-[var(--color-fg)]",
							children: a.tasks
						})]
					})]
				})] }, a.id))
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "How this maps to Grok web"
			}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "grid gap-3 text-sm text-[var(--color-muted)] md:grid-cols-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-1 font-medium text-[var(--color-fg)]",
							children: "Connectors"
						}), "I/O layer — Gmail, Drive, GitHub, Notion, custom MCP. Same idea as grok.com/connectors."]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-1 font-medium text-[var(--color-fg)]",
							children: "Skills"
						}), "Procedure layer — teach once, slash forever. Office docs + custom workflows."]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-1 font-medium text-[var(--color-fg)]",
							children: "Automations"
						}), "Schedule or email-trigger jobs that attach connectors + skills and leave a run log."]
					})
				]
			})] })
		]
	});
}
//#endregion
export { AgentsView };
