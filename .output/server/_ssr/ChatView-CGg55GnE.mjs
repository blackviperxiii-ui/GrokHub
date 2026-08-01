import { o as __toESM } from "../_runtime.mjs";
import { C as usagePercent, f as getMode, n as PLAN_LIMITS, u as formatUnits } from "./version-BE4o4tL_.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { r as cn, t as Button } from "./GrokLogo-eJjdnOC_.mjs";
import { a as CardHeader, i as CardDescription, n as Card, o as CardTitle, r as CardContent, s as useGrokHub, t as Badge } from "./card-J2wZc2w_.mjs";
import { I as Compass, S as LoaderCircle, b as MessageSquarePlus, c as Sparkles, f as Send, k as Gauge, o as Terminal, s as Square, u as ShieldAlert } from "../_libs/lucide-react.mjs";
import { t as RelativeTime } from "./RelativeTime-CDg-rQ4W.mjs";
import { t as Textarea } from "./textarea-aSaPOSKs.mjs";
import { t as HostGatewayBanner } from "./HostGatewayBanner-CUCDDmX3.mjs";
import { t as Markdown } from "../_libs/react-markdown+[...].mjs";
import { t as remarkGfm } from "../_libs/remark-gfm.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/ChatView-CGg55GnE.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var MAX_DEFAULT = 8;
var MAX_HARD = 10;
function recentUserMessages(chat, n = 12) {
	return chat.filter((m) => m.role === "user").slice(-n).map((m) => m.content.trim()).filter(Boolean).reverse();
}
function uniqByValue(chips) {
	const seen = /* @__PURE__ */ new Set();
	const out = [];
	for (const c of chips) {
		const key = c.value.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		out.push(c);
	}
	return out;
}
function shorten(s, n = 36) {
	const t = s.replace(/\s+/g, " ").trim();
	if (t.length <= n) return t;
	return t.slice(0, n - 1) + "…";
}
/** Infer follow-ups from a prior user line. */
function followUpsFrom(msg) {
	const lower = msg.toLowerCase();
	const out = [];
	if (/download/i.test(lower)) {
		out.push({
			id: "fu-dl",
			label: "List Downloads again",
			value: "List what's in my Downloads folder",
			kind: "chat",
			score: 70
		});
		out.push({
			id: "fu-dl-shell",
			label: "$ ls Downloads",
			value: "$ ls -la \"$HOME/Downloads\" | head -40",
			kind: "shell",
			score: 65
		});
	}
	if (/desktop|host|uname|shell|cli/i.test(lower) || lower.startsWith("$")) {
		out.push({
			id: "fu-disk",
			label: "Disk free",
			value: "$ df -h | head -12",
			kind: "shell",
			score: 55
		});
		out.push({
			id: "fu-ps",
			label: "Top processes",
			value: "$ ps aux --sort=-%mem | head -12",
			kind: "shell",
			score: 50
		});
	}
	if (/code|bug|error|implement|refactor|build/i.test(lower)) {
		out.push({
			id: "fu-build",
			label: "Switch to Build",
			value: "__mode:build",
			kind: "mode",
			score: 60,
			hint: "mode"
		});
		out.push({
			id: "fu-explain",
			label: "Explain the approach",
			value: "Explain the approach step by step",
			kind: "chat",
			score: 52
		});
	}
	if (/imagine|image|draw|picture|logo/i.test(lower)) out.push({
		id: "fu-imagine",
		label: "Open Imagine",
		value: "__nav:imagine",
		kind: "nav",
		score: 68
	});
	if (/usage|quota|limit|plan/i.test(lower)) out.push({
		id: "fu-usage",
		label: "Usage details",
		value: "What's my SuperGrok usage right now?",
		kind: "chat",
		score: 48
	});
	return out;
}
/**
* Build ranked quick-assistant chips.
* Always returns ≤ max (default 8, hard cap 10).
*/
function buildQuickChips(input) {
	const max = Math.min(input.max ?? MAX_DEFAULT, MAX_HARD);
	const chips = [];
	const users = recentUserMessages(input.chat);
	const lastUser = users[0] || "";
	const pct = Math.round(usagePercent(input.usage));
	const plan = PLAN_LIMITS[input.usage.plan];
	const recentActivity = input.activity.slice(0, 12);
	const draft = (input.draft || "").trim().toLowerCase();
	if (!input.grokConnected) chips.push({
		id: "ctx-connect",
		label: "Connect Grok",
		value: "__nav:settings",
		kind: "nav",
		score: 100,
		hint: "oauth"
	});
	if (input.hostOnline === false) chips.push({
		id: "ctx-host",
		label: "Connect desktop host",
		value: "__nav:settings",
		kind: "nav",
		score: 95
	});
	else if (input.hostOnline) chips.push({
		id: "ctx-uname",
		label: "$ uname -a",
		value: "$ uname -a && whoami && pwd",
		kind: "shell",
		score: 40
	});
	if (pct >= 80) chips.push({
		id: "ctx-quota",
		label: `Usage ${pct}%`,
		value: "What's my usage and how can I save units?",
		kind: "chat",
		score: 85
	});
	for (let i = 0; i < Math.min(users.length, 5); i++) {
		const msg = users[i];
		const ageBoost = 30 - i * 5;
		if (msg.length < 80 && i > 0) chips.push({
			id: `recent-${i}`,
			label: shorten(msg, 28),
			value: msg,
			kind: msg.startsWith("$") ? "shell" : "chat",
			score: 45 + ageBoost,
			hint: "recent"
		});
		chips.push(...followUpsFrom(msg).map((c) => ({
			...c,
			score: c.score + ageBoost * .3
		})));
	}
	for (const a of recentActivity) {
		if (a.kind === "desktop") chips.push({
			id: `act-host-${a.id}`,
			label: "Host status",
			value: "$ uname -a && df -h | head -8",
			kind: "shell",
			score: 42
		});
		if (a.kind === "imagine") chips.push({
			id: `act-img-${a.id}`,
			label: "Imagine again",
			value: "__nav:imagine",
			kind: "nav",
			score: 50
		});
		if (a.kind === "chat" && a.status === "failed") chips.push({
			id: `act-retry-${a.id}`,
			label: "Retry last ask",
			value: lastUser || "Try that again",
			kind: "chat",
			score: 72
		});
		if (a.kind === "system" && /update/i.test(a.title + a.detail)) chips.push({
			id: `act-upd-${a.id}`,
			label: "Check updates",
			value: "__nav:settings",
			kind: "nav",
			score: 35
		});
		if (a.kind === "usage") chips.push({
			id: `act-usage-${a.id}`,
			label: "Usage",
			value: "What's my usage right now?",
			kind: "chat",
			score: 44
		});
	}
	const otherThreads = input.threads.filter((t) => t.messages.some((m) => m.role === "user")).slice(0, 3);
	for (const th of otherThreads) {
		const first = th.messages.find((m) => m.role === "user");
		if (!first) continue;
		chips.push({
			id: `thread-${th.id}`,
			label: shorten(th.title || first.content, 26),
			value: `Continue: ${shorten(first.content, 120)}`,
			kind: "chat",
			score: 28,
			hint: "history"
		});
	}
	if (input.connectors.filter((c) => c.status === "connected").some((c) => c.id === "desktop-host")) chips.push({
		id: "conn-files",
		label: "Browse home",
		value: "$ ls -la \"$HOME\" | head -30",
		kind: "shell",
		score: 38
	});
	const defaults = [
		{
			id: "def-help",
			label: "What can you help with?",
			value: "What can you help me with in GrokHub?",
			kind: "chat",
			score: 20
		},
		{
			id: "def-modes",
			label: "Explain my modes",
			value: "Explain Auto / Fast / Expert / Heavy / Build and when to use each",
			kind: "chat",
			score: 18
		},
		{
			id: "def-usage",
			label: "My usage",
			value: `What's my usage? (${plan.label})`,
			kind: "chat",
			score: 16
		},
		{
			id: "def-imagine",
			label: "Imagine",
			value: "__nav:imagine",
			kind: "nav",
			score: 15
		},
		{
			id: "def-auto",
			label: input.mode === "auto" ? "How Auto routes" : "Use Auto mode",
			value: input.mode === "auto" ? "How does Auto choose models for my prompts?" : "__mode:auto",
			kind: input.mode === "auto" ? "chat" : "mode",
			score: 14
		}
	];
	chips.push(...defaults);
	let ranked = uniqByValue(chips).filter((c) => c.value.trim().toLowerCase() !== draft).sort((a, b) => b.score - a.score);
	if (draft.length >= 1) {
		const pred = ranked.map((c) => {
			const hay = `${c.label} ${c.value}`.toLowerCase();
			let boost = 0;
			if (hay.startsWith(draft)) boost += 40;
			else if (hay.includes(draft)) boost += 25;
			for (const tok of draft.split(/\s+/)) if (tok.length > 2 && hay.includes(tok)) boost += 8;
			if (draft.startsWith("$") && c.kind === "shell") boost += 30;
			if (/imagine|draw|image/.test(draft) && c.kind === "nav" && c.value.includes("imagine")) boost += 35;
			return {
				...c,
				score: c.score + boost
			};
		}).filter((c) => {
			const hay = `${c.label} ${c.value}`.toLowerCase();
			if (draft.length < 2) return true;
			return c.score >= 80 || hay.includes(draft) || draft.split(/\s+/).some((tok) => tok.length > 2 && hay.includes(tok));
		}).sort((a, b) => b.score - a.score);
		ranked = pred.length ? pred : ranked;
	}
	const picked = [];
	const kindCount = {};
	for (const c of ranked) {
		if (picked.length >= max) break;
		const k = c.kind;
		const n = kindCount[k] || 0;
		if (k === "shell" && n >= 3) continue;
		if (k === "nav" && n >= 2) continue;
		if (k === "mode" && n >= 1) continue;
		picked.push(c);
		kindCount[k] = n + 1;
	}
	if (picked.length < Math.min(4, max)) for (const c of ranked) {
		if (picked.length >= max) break;
		if (picked.some((p) => p.id === c.id)) continue;
		picked.push(c);
	}
	return picked.slice(0, max);
}
var components = {
	a: ({ href, children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("a", {
		href,
		target: "_blank",
		rel: "noopener noreferrer",
		className: "text-[var(--color-info)] underline-offset-2 hover:underline",
		children
	}),
	code: ({ className, children, ...props }) => {
		if (Boolean(className?.includes("language-")) || String(children).includes("\n")) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
			className: cn("block overflow-x-auto rounded-[var(--radius-sm)] bg-[var(--color-elevated)] p-3 font-mono text-[12px] leading-relaxed text-[var(--color-fg)]", className),
			...props,
			children
		});
		return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("code", {
			className: "rounded bg-[var(--color-elevated)] px-1 py-0.5 font-mono text-[12px]",
			...props,
			children
		});
	},
	pre: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
		className: "my-2 overflow-x-auto",
		children
	}),
	ul: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
		className: "my-1.5 list-disc space-y-0.5 pl-5",
		children
	}),
	ol: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("ol", {
		className: "my-1.5 list-decimal space-y-0.5 pl-5",
		children
	}),
	li: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", {
		className: "leading-relaxed",
		children
	}),
	p: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: "my-1.5 leading-relaxed first:mt-0 last:mb-0",
		children
	}),
	h1: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h1", {
		className: "mb-2 mt-3 text-base font-semibold",
		children
	}),
	h2: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h2", {
		className: "mb-1.5 mt-3 text-sm font-semibold",
		children
	}),
	h3: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
		className: "mb-1 mt-2 text-sm font-medium",
		children
	}),
	blockquote: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("blockquote", {
		className: "my-2 border-l-2 border-[var(--color-border-strong)] pl-3 text-[var(--color-muted)]",
		children
	}),
	table: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "my-2 overflow-x-auto",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("table", {
			className: "w-full border-collapse text-xs",
			children
		})
	}),
	th: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("th", {
		className: "border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1 text-left font-medium",
		children
	}),
	td: ({ children }) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("td", {
		className: "border border-[var(--color-border)] px-2 py-1 align-top",
		children
	}),
	hr: () => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("hr", { className: "my-3 border-[var(--color-border)]" })
};
/** Render assistant/system markdown safely (no raw HTML). */
function MarkdownBody({ content, className }) {
	if (!content) return null;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("markdown-body min-w-0 break-words", className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Markdown, {
			remarkPlugins: [remarkGfm],
			components,
			children: content
		})
	});
}
function chipIcon(kind) {
	if (kind === "shell") return Terminal;
	if (kind === "nav") return Compass;
	if (kind === "mode") return Gauge;
	return Sparkles;
}
function ChatView() {
	const chat = useGrokHub((s) => s.chat);
	const sendChat = useGrokHub((s) => s.sendChat);
	const stopChat = useGrokHub((s) => s.stopChat);
	const running = useGrokHub((s) => s.running);
	const streamStatus = useGrokHub((s) => s.streamStatus);
	const mode = useGrokHub((s) => s.mode);
	const setMode = useGrokHub((s) => s.setMode);
	const setNav = useGrokHub((s) => s.setNav);
	const pushActivity = useGrokHub((s) => s.pushActivity);
	const recordUsage = useGrokHub((s) => s.recordUsage);
	const usage = useGrokHub((s) => s.usage);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const newThread = useGrokHub((s) => s.newThread);
	const activity = useGrokHub((s) => s.activity);
	const threads = useGrokHub((s) => s.threads);
	const connectors = useGrokHub((s) => s.connectors);
	const pendingHostConfirm = useGrokHub((s) => s.pendingHostConfirm);
	const resolveHostConfirm = useGrokHub((s) => s.resolveHostConfirm);
	const [text, setText] = (0, import_react.useState)("");
	const [localRunning, setLocalRunning] = (0, import_react.useState)(false);
	const [hostOnline, setHostOnline] = (0, import_react.useState)(void 0);
	const endRef = (0, import_react.useRef)(null);
	const inputRef = (0, import_react.useRef)(null);
	const modeMeta = getMode(mode);
	const busy = running || localRunning;
	const plan = PLAN_LIMITS[usage.plan];
	const pct = Math.round(usagePercent(usage));
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		(async () => {
			try {
				const { hostInfo } = await import("./host-client-D2zxhrZc.mjs");
				const i = await hostInfo();
				if (!cancelled) setHostOnline(i.bridge !== "none" && Boolean(i.unsandboxed));
			} catch {
				if (!cancelled) setHostOnline(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, []);
	const chips = (0, import_react.useMemo)(() => buildQuickChips({
		chat,
		activity,
		threads,
		connectors,
		mode,
		grokConnected,
		usage,
		draft: text,
		hostOnline,
		max: text.trim().length > 0 ? 10 : Math.min(10, Math.max(4, 4 + Math.min(chat.length, 4)))
	}), [
		chat,
		activity,
		threads,
		connectors,
		mode,
		grokConnected,
		usage,
		text,
		hostOnline
	]);
	(0, import_react.useEffect)(() => {
		endRef.current?.scrollIntoView({ behavior: "smooth" });
	}, [
		chat,
		busy,
		streamStatus
	]);
	async function runShell(command) {
		setLocalRunning(true);
		const userLine = command.startsWith("$") ? command : `$ ${command}`;
		useGrokHub.setState((s) => {
			const chat = [...s.chat, {
				id: `u_${Date.now()}`,
				role: "user",
				content: userLine,
				ts: Date.now(),
				mode
			}];
			return {
				chat,
				threads: s.threads.map((t) => t.id === s.activeThreadId ? {
					...t,
					messages: chat,
					updatedAt: Date.now()
				} : t),
				running: true,
				streamStatus: "Host running…"
			};
		});
		try {
			const bill = recordUsage("host");
			if (!bill.ok) {
				useGrokHub.setState((s) => ({ chat: [...s.chat, {
					id: `a_${Date.now()}`,
					role: "system",
					content: `Host blocked: ${plan.label} unit quota exhausted.`,
					ts: Date.now()
				}] }));
				return;
			}
			const cmd = command.replace(/^\$\s*/, "").replace(/^\/sh\s+/, "").trim();
			const { hostExec } = await import("./host-client-D2zxhrZc.mjs");
			const r = await hostExec(cmd);
			const body = [
				`[Desktop host · ${r.ok ? "ok" : "fail"} · exit ${r.code ?? "?"} · ${bill.cost}u]`,
				`cwd: ${r.cwd}`,
				"",
				r.stdout || "(no stdout)",
				r.stderr ? `\n[stderr]\n${r.stderr}` : ""
			].filter(Boolean).join("\n");
			useGrokHub.setState((s) => {
				const chat = [...s.chat, {
					id: `a_${Date.now()}`,
					role: "assistant",
					content: body,
					ts: Date.now(),
					mode
				}];
				return {
					chat,
					threads: s.threads.map((t) => t.id === s.activeThreadId ? {
						...t,
						messages: chat,
						updatedAt: Date.now()
					} : t)
				};
			});
			pushActivity({
				kind: "desktop",
				title: r.ok ? "Host command ok" : "Host command failed",
				detail: `${cmd.slice(0, 100)} · ${bill.cost}u`,
				status: r.ok ? "success" : "failed"
			});
		} catch (e) {
			const msg = e instanceof Error ? e.message : "host failed";
			useGrokHub.setState((s) => ({ chat: [...s.chat, {
				id: `a_${Date.now()}`,
				role: "system",
				content: `Desktop gateway offline: ${msg}\n\nConnect the host in Settings → Desktop host gateway so I can run shell/files/apps on your machine.`,
				ts: Date.now()
			}] }));
			setNav("settings");
		} finally {
			setLocalRunning(false);
			useGrokHub.setState({
				running: false,
				streamStatus: null
			});
		}
	}
	async function onChip(chip) {
		if (busy) return;
		if (chip.kind === "nav" && chip.value.startsWith("__nav:")) {
			const nav = chip.value.slice(6);
			setNav(nav);
			return;
		}
		if (chip.kind === "mode" && chip.value.startsWith("__mode:")) {
			const m = chip.value.slice(7);
			setMode(m);
			return;
		}
		if (chip.kind === "shell" || chip.value.startsWith("$") || chip.value.startsWith("/sh ")) {
			setText("");
			await runShell(chip.value);
			return;
		}
		setText("");
		await sendChat(chip.value);
	}
	async function onSend(value) {
		if (busy) return;
		const payload = (value ?? text).trim();
		if (!payload) return;
		if (payload.toLowerCase().includes("imagine") && !payload.startsWith("/") && !payload.startsWith("$")) setNav("imagine");
		setText("");
		if (payload.startsWith("$") || payload.startsWith("/sh ")) {
			await runShell(payload);
			return;
		}
		await sendChat(payload);
	}
	function onStop() {
		if (localRunning) {
			setLocalRunning(false);
			useGrokHub.setState({
				running: false,
				streamStatus: null
			});
			return;
		}
		stopChat();
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "chat-stage mx-auto flex h-full min-h-0 w-full flex-col gap-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
			className: "flex min-h-0 flex-1 flex-col overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, {
				className: "shrink-0 border-b border-[var(--color-border)] px-4 py-3 md:px-6 3xl:px-8",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Agent"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
						"Live Grok · ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-mono",
							children: "$"
						}),
						" host shell · History in the sidebar"
					] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-col items-end gap-1",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex gap-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
									size: "sm",
									variant: "secondary",
									onClick: () => newThread(),
									disabled: busy,
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquarePlus, { className: "h-3.5 w-3.5" }), "New"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									className: "font-mono",
									children: modeMeta.label
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: grokConnected ? "success" : "default",
								className: "text-[10px]",
								children: grokConnected ? "Grok live" : "Connect in Settings"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "text-[10px] tabular text-[var(--color-subtle)]",
								children: [
									formatUnits(usage.usedUnits),
									"/",
									formatUnits(plan.units),
									" · ",
									pct,
									"%"
								]
							})
						]
					})]
				})
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-0",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 px-4 pt-3 md:px-6 3xl:px-8",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HostGatewayBanner, { variant: "compact" })
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-6 3xl:px-10 uw:px-16",
						children: [
							chat.map((m) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: cn("flex", m.role === "user" ? "justify-end" : "justify-start"),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: cn("chat-bubble rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed", m.role === "user" ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : m.role === "system" ? "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]" : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)]", m.streaming && "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]"),
									children: [
										/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
											className: "mb-1 flex flex-wrap items-center gap-2 text-[10px] uppercase tracking-wide text-[var(--color-subtle)]",
											children: [
												/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
													m.role,
													" · ",
													/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: m.ts })
												] }),
												m.mode && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
													className: "rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case",
													children: getMode(m.mode).label
												}),
												m.streaming && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
													className: "inline-flex items-center gap-1 rounded border border-[color-mix(in_oklab,var(--color-info)_40%,transparent)] px-1.5 py-px font-mono normal-case text-[var(--color-info)]",
													children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-2.5 w-2.5 animate-spin" }), "streaming"]
												}),
												m.stopped && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
													className: "rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case text-[var(--color-warn)]",
													children: "stopped"
												})
											]
										}),
										m.content ? m.role === "user" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
											className: "whitespace-pre-wrap",
											children: m.content
										}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(MarkdownBody, { content: m.content }) : m.streaming ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "inline-flex items-center gap-1.5 text-[var(--color-subtle)]",
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-info)]" }), "…"]
										}) : "",
										m.streaming && m.content ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-[var(--color-fg)] align-middle opacity-70" }) : null
									]
								})
							}, m.id)),
							busy && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center gap-2 text-xs text-[var(--color-subtle)]",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin text-[var(--color-info)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "shimmer rounded px-1",
									children: streamStatus || (localRunning ? "Host running…" : `${modeMeta.label} · working…`)
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { ref: endRef })
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "shrink-0 space-y-2 border-t border-[var(--color-border)] p-3 md:p-4 3xl:px-8 uw:px-12",
						children: [
							!busy && chips.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mx-auto w-full max-w-[min(56rem,100%)] 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "mb-1 flex items-center justify-center gap-2 px-0.5",
									children: [
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]",
											children: "Quick assist"
										}),
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
											className: "text-[10px] text-[var(--color-subtle)]",
											children: "·"
										}),
										/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "text-[10px] text-[var(--color-subtle)]",
											children: [
												chips.length,
												" suggestion",
												chips.length === 1 ? "" : "s"
											]
										})
									]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "flex flex-wrap items-center justify-center gap-1.5",
									role: "listbox",
									"aria-label": "Quick assistant suggestions",
									children: chips.map((c) => {
										const Icon = chipIcon(c.kind);
										return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
											type: "button",
											role: "option",
											disabled: busy,
											title: c.value.startsWith("__") ? c.label : c.value,
											onClick: () => void onChip(c),
											className: cn("inline-flex max-w-[14rem] items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-colors", "border-[var(--color-border)] text-[var(--color-muted)]", "hover:border-[var(--color-border-strong)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]", "disabled:opacity-50", c.kind === "shell" && "font-mono", c.score >= 80 && "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]"),
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-3 w-3 shrink-0 opacity-70" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
												className: "truncate",
												children: c.label
											})]
										}, c.id);
									})
								})]
							}),
							pendingHostConfirm && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mx-auto w-full max-w-[min(56rem,100%)] rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,var(--color-surface))] p-3 3xl:max-w-[min(64rem,100%)]",
								children: [
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "mb-2 flex items-center gap-2 text-sm font-medium text-[var(--color-fg)]",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-4 w-4 text-[var(--color-warn)]" }), "Allow host commands?"]
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("ul", {
										className: "mb-3 space-y-1 font-mono text-xs text-[var(--color-muted)]",
										children: pendingHostConfirm.cmds.map((c, i) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", {
											className: "break-all",
											children: [
												/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
													className: "text-[var(--color-subtle)]",
													children: [
														"[",
														pendingHostConfirm.risks[i] || "run",
														"]"
													]
												}),
												" ",
												"$ ",
												c
											]
										}, c + i))
									}),
									/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
										className: "flex gap-2",
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
											size: "sm",
											onClick: () => resolveHostConfirm(true),
											children: "Run on this machine"
										}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
											size: "sm",
											variant: "secondary",
											onClick: () => resolveHostConfirm(false),
											children: "Cancel"
										})]
									})
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
								className: "mx-auto flex w-full max-w-[min(56rem,100%)] gap-2 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]",
								onSubmit: (e) => {
									e.preventDefault();
									if (busy) {
										onStop();
										return;
									}
									onSend();
								},
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Textarea, {
									ref: inputRef,
									value: text,
									onChange: (e) => setText(e.target.value),
									placeholder: busy ? "Agent running — press Stop to interrupt…" : "Message Grok…  Enter to send · Shift+Enter for newline · $ shell",
									rows: 1,
									className: "max-h-40 min-h-[2.5rem] flex-1 resize-y",
									onKeyDown: (e) => {
										if (e.key === "Escape" && busy) {
											e.preventDefault();
											onStop();
											return;
										}
										if (e.key === "Enter" && !e.shiftKey && !busy) {
											e.preventDefault();
											onSend();
										}
									}
								}), busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									type: "button",
									variant: "secondary",
									size: "icon",
									onClick: onStop,
									"aria-label": "Stop",
									title: "Stop (Esc)",
									className: "border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] text-[var(--color-danger)]",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Square, { className: "h-3.5 w-3.5 fill-current" })
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									type: "submit",
									disabled: !text.trim(),
									size: "icon",
									"aria-label": "Send",
									children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Send, { className: "h-4 w-4" })
								})]
							}),
							busy && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mx-auto flex w-full max-w-[min(56rem,100%)] items-center justify-between text-[10px] text-[var(--color-subtle)] 3xl:max-w-[min(64rem,100%)]",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "inline-flex items-center gap-1.5",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "pulse-live inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-info)]" }), streamStatus || "Running"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
									type: "button",
									className: "font-medium text-[var(--color-danger)] hover:underline",
									onClick: onStop,
									children: "Stop generating"
								})]
							})
						]
					})
				]
			})]
		})
	});
}
//#endregion
export { ChatView };
