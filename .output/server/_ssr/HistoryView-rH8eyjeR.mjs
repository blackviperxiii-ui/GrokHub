import { o as __toESM } from "../_runtime.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, f as useGrokHub, i as CardDescription, n as Card, o as CardTitle, r as CardContent, t as Badge } from "./card-CfSUHW0g.mjs";
import { A as History, a as Trash2, w as MessageSquarePlus, y as Pencil } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/HistoryView-rH8eyjeR.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
function HistoryView() {
	const threads = useGrokHub((s) => s.threads);
	const activeThreadId = useGrokHub((s) => s.activeThreadId);
	const selectThread = useGrokHub((s) => s.selectThread);
	const deleteThread = useGrokHub((s) => s.deleteThread);
	const renameThread = useGrokHub((s) => s.renameThread);
	const newThread = useGrokHub((s) => s.newThread);
	const [renamingId, setRenamingId] = (0, import_react.useState)(null);
	const [draft, setDraft] = (0, import_react.useState)("");
	const inputRef = (0, import_react.useRef)(null);
	const sorted = [...threads].sort((a, b) => b.updatedAt - a.updatedAt);
	(0, import_react.useEffect)(() => {
		if (renamingId) {
			inputRef.current?.focus();
			inputRef.current?.select();
		}
	}, [renamingId]);
	function startRename(id, title) {
		setRenamingId(id);
		setDraft(title);
	}
	function commitRename() {
		if (!renamingId) return;
		const next = draft.trim();
		if (next) renameThread(renamingId, next);
		setRenamingId(null);
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "content-readable mx-auto space-y-4",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
			className: "flex flex-row items-start justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(History, { className: "h-4 w-4" }), "History"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Past chats — select to resume, rename, or delete." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
				size: "sm",
				onClick: () => newThread(),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquarePlus, { className: "h-4 w-4" }), "New chat"]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "space-y-1.5 p-3 pt-0",
			children: [sorted.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
				className: "px-2 py-8 text-center text-sm text-[var(--color-muted)]",
				children: "No chats yet. Start one from Agent."
			}), sorted.map((t) => {
				const active = t.id === activeThreadId;
				const preview = [...t.messages].reverse().find((m) => m.role === "user" || m.role === "assistant")?.content || "Empty chat";
				const isRenaming = renamingId === t.id;
				return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("group flex items-start gap-2 rounded-[var(--radius-md)] border px-3 py-2.5 transition-colors", active ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]" : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"),
					children: [isRenaming ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "min-w-0 flex-1",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
							ref: inputRef,
							value: draft,
							onChange: (e) => setDraft(e.target.value),
							onBlur: commitRename,
							onKeyDown: (e) => {
								if (e.key === "Enter") {
									e.preventDefault();
									commitRename();
								}
								if (e.key === "Escape") setRenamingId(null);
							},
							className: "w-full rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] bg-[var(--color-bg)] px-2 py-1.5 text-sm outline-none focus:ring-1 focus:ring-[var(--color-accent)]",
							"aria-label": "Rename chat"
						})
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						className: "min-w-0 flex-1 text-left",
						onClick: () => selectThread(t.id),
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex flex-wrap items-center gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate text-sm font-medium",
									children: t.title
								}), active && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									className: "text-[10px]",
									variant: "info",
									children: "Open"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "mt-0.5 line-clamp-2 text-xs text-[var(--color-muted)]",
								children: preview.replace(/\s+/g, " ").slice(0, 140)
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 text-[10px] text-[var(--color-subtle)]",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: t.updatedAt }), t.mode ? ` · ${t.mode}` : ""]
							})
						]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex shrink-0 gap-0.5 opacity-0 transition-opacity group-hover:opacity-100",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "rounded p-1.5 text-[var(--color-subtle)] hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]",
							"aria-label": "Rename chat",
							onClick: () => startRename(t.id, t.title),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Pencil, { className: "h-3.5 w-3.5" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							className: "rounded p-1.5 text-[var(--color-subtle)] hover:bg-[var(--color-surface)] hover:text-[var(--color-danger)]",
							"aria-label": "Delete chat",
							onClick: () => deleteThread(t.id),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "h-3.5 w-3.5" })
						})]
					})]
				}, t.id);
			})]
		})] })
	});
}
//#endregion
export { HistoryView };
