import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-Dgqzm_EC.mjs";
import { E as LoaderCircle, G as Cable, O as Link2Off, W as Check } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Input } from "./input-bxgymBz8.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/ConnectorsView-Cj8fIlHQ.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function ConnectorsView() {
	const connectors = useGrokHub((s) => s.connectors);
	const connectConnector = useGrokHub((s) => s.connectConnector);
	const oauth = useGrokHub((s) => s.oauth);
	const [q, setQ] = (0, import_react.useState)("");
	const [busyId, setBusyId] = (0, import_react.useState)(null);
	const filtered = (0, import_react.useMemo)(() => {
		const needle = q.trim().toLowerCase();
		if (!needle) return connectors;
		return connectors.filter((c) => c.name.toLowerCase().includes(needle) || c.category.toLowerCase().includes(needle) || c.description.toLowerCase().includes(needle));
	}, [connectors, q]);
	const connected = connectors.filter((c) => c.status === "connected").length;
	async function onToggle(id) {
		setBusyId(id);
		try {
			await connectConnector(id);
		} finally {
			setBusyId(null);
		}
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
			className: "gap-3 sm:flex-row sm:items-end sm:justify-between",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Cable, { className: "h-4 w-4" }), "Grok Connectors"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Live: Grok, Desktop Host, GitHub. Other rows are planned integrations (open vendor site only — not marked connected).",
				` ${connected} live.`,
				oauth?.email ? ` · Grok as ${oauth.email}` : ""
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: q,
				onChange: (e) => setQ(e.target.value),
				placeholder: "Search connectors",
				className: "sm:max-w-xs"
			})]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "grid gap-3 md:grid-cols-2 xl:grid-cols-3",
			children: filtered.map((c) => {
				const live = c.id === "grok-xai" || c.id === "desktop-host" || c.id === "github";
				const on = c.status === "connected";
				const err = c.status === "error";
				const busy = busyId === c.id;
				return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "flex flex-col",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
							className: "text-sm",
							children: c.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
							className: "mt-1",
							children: c.category
						})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: on ? "success" : err ? "danger" : live ? "default" : "info",
							children: on ? "connected" : err ? "error" : live ? "offline" : "coming soon"
						})]
					}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "mt-auto flex flex-1 flex-col gap-4",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm text-[var(--color-muted)]",
								children: c.description
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "flex flex-wrap gap-1.5",
								children: c.tools.slice(0, 4).map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-subtle)]",
									children: t
								}, t))
							}),
							c.lastUsed && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "text-xs text-[var(--color-subtle)]",
								children: ["Last used ", /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: c.lastUsed })]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: on ? "secondary" : "default",
								size: "sm",
								className: "mt-auto w-full",
								disabled: busy,
								onClick: () => void onToggle(c.id),
								children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin" }), "Working…"] }) : on ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link2Off, { className: "h-3.5 w-3.5" }), "Disconnect"] }) : live ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "h-3.5 w-3.5" }), "Connect"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "h-3.5 w-3.5" }), "Open vendor"] })
							})
						]
					})]
				}, c.id);
			})
		})]
	});
}
//#endregion
export { ConnectorsView };
