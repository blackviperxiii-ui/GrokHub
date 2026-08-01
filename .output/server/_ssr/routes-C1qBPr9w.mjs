import { o as __toESM } from "../_runtime.mjs";
import { a as emptyCatalog, c as getModesWithCatalog, d as needsGrokClassification, f as resolveMode, i as buildCatalog, l as modeBadge, m as stripAssistantChrome, n as applyGrokPlan, o as friendlyModelName, p as resolveModeWithCatalog, r as autoRouteFor, s as getMode, t as APP_VERSION, u as modelIdForMode } from "./version-By51W1Q4.mjs";
import { n as GROK_PROVIDERS } from "./providers-DD9Wq7fi.mjs";
import { F as require_react, P as require_jsx_runtime, g as require_react_dom, h as Link } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as cva } from "../_libs/class-variance-authority+clsx.mjs";
import { a as signIn, c as useCurrentUser, i as formatRelative, l as useCurrentUserState, n as GrokHubMark, o as signOut, r as cn, s as uid, t as Button } from "./button-Cz9j7Ln5.mjs";
import { A as ExternalLink, B as Activity, C as Image, D as Gauge, E as Hammer, F as Check, I as Cable, L as Brain, M as Compass, N as Command, O as Folder, P as ChevronRight, R as ArrowRight, S as Link2Off, T as HardDrive, _ as Minus, a as TimerReset, b as Menu, c as Sparkles, d as Settings, f as Send, g as Play, h as Plus, i as Trash2, j as Download, k as FolderOpen, l as ShieldCheck, m as Plug, n as X, o as Terminal, p as RefreshCw, r as Users, s as Square, t as Zap, u as ShieldAlert, v as MessageSquare, w as History, x as LoaderCircle, y as MessageSquarePlus, z as AppWindow } from "../_libs/lucide-react.mjs";
import { n as create, t as persist } from "../_libs/zustand.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/routes-C1qBPr9w.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
var import_react_dom = require_react_dom();
async function rpc(path, action, body = {}, init) {
	const res = await fetch(path, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({
			action,
			...body
		}),
		signal: init?.signal
	});
	const data = await res.json().catch(() => ({}));
	if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
	return data;
}
async function grokChat(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.chat) return desktop.chat(opts);
	return rpc("/api/grok", "chat", opts, { signal: opts.signal });
}
/**
* Stream chat tokens. Prefer Electron IPC stream; fall back to SSE /api/grok,
* then non-stream chat.
*/
async function grokChatStream(opts, handlers) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.chatStream) return desktop.chatStream(opts, handlers);
	try {
		const res = await fetch("/api/grok", {
			method: "POST",
			headers: {
				"content-type": "application/json",
				accept: "text/event-stream"
			},
			body: JSON.stringify({
				action: "chatStream",
				...opts
			}),
			signal: handlers.signal
		});
		const ctype = res.headers.get("content-type") || "";
		if (res.ok && (ctype.includes("text/event-stream") || ctype.includes("ndjson"))) {
			handlers.onStatus?.("streaming");
			const reader = res.body?.getReader();
			if (!reader) throw new Error("no stream body");
			const decoder = new TextDecoder();
			let buffer = "";
			let content = "";
			let model;
			let tokens;
			while (true) {
				if (handlers.signal?.aborted) {
					try {
						await reader.cancel();
					} catch {}
					return {
						ok: false,
						aborted: true,
						error: "Stopped",
						content,
						model
					};
				}
				const { done, value } = await reader.read();
				if (done) break;
				buffer += decoder.decode(value, { stream: true });
				const parts = buffer.split("\n");
				buffer = parts.pop() || "";
				for (const raw of parts) {
					const line = raw.trim();
					if (!line || line.startsWith(":")) continue;
					let payload = line;
					if (line.startsWith("data:")) payload = line.slice(5).trim();
					if (payload === "[DONE]") continue;
					try {
						const evt = JSON.parse(payload);
						if (evt.type === "delta" && evt.delta) {
							content += evt.delta;
							handlers.onDelta(evt.delta);
						} else if (evt.type === "status" && evt.content) handlers.onStatus?.(evt.content);
						else if (evt.type === "done") {
							model = evt.model || model;
							tokens = evt.tokens || tokens;
							if (evt.content && !content) {
								content = evt.content;
								handlers.onDelta(evt.content);
							}
						} else if (evt.type === "error") return {
							ok: false,
							error: evt.error || "stream error",
							content,
							model,
							tokens
						};
						else if (evt.delta) {
							content += evt.delta;
							handlers.onDelta(evt.delta);
						}
					} catch {}
				}
			}
			if (!content.trim()) return {
				ok: false,
				error: "Empty stream",
				model,
				tokens
			};
			return {
				ok: true,
				content,
				model,
				tokens
			};
		}
		if (res.ok) {
			const data = await res.json();
			if (data.ok && data.content) handlers.onDelta(data.content);
			return data;
		}
	} catch (e) {
		if (handlers.signal?.aborted || e instanceof Error && e.name === "AbortError") return {
			ok: false,
			aborted: true,
			error: "Stopped"
		};
	}
	handlers.onStatus?.("fallback");
	const full = await grokChat({
		...opts,
		signal: handlers.signal
	});
	if (full.ok && full.content) handlers.onDelta(full.content);
	return full;
}
async function grokProbe(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.probe) return desktop.probe(opts?.apiKey, opts?.accessToken);
	return rpc("/api/grok", "probe", {
		apiKey: opts?.apiKey || "",
		accessToken: opts?.accessToken || ""
	});
}
async function oauthStart() {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthStart) return desktop.oauthStart();
	return rpc("/api/grok", "oauthStart", {});
}
async function oauthPoll(deviceCode) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthPoll) return desktop.oauthPoll(deviceCode);
	return rpc("/api/grok", "oauthPoll", { deviceCode });
}
async function oauthEnsure(tokens) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.oauthEnsure) return desktop.oauthEnsure(tokens);
	return rpc("/api/grok", "oauthEnsure", { tokens });
}
async function grokImagine(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.imagine) return desktop.imagine(opts);
	return rpc("/api/grok", "imagine", opts);
}
async function checkUpdate(token) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.checkUpdate) return desktop.checkUpdate({ token });
	return rpc("/api/update", "check", { token: token || "" });
}
async function applyUpdate(token, force = true) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.applyUpdate) return desktop.applyUpdate({
		token,
		force,
		restart: true
	});
	return rpc("/api/update", "apply", {
		token: token || "",
		force,
		restart: false
	});
}
function hash(s) {
	let h = 2166136261;
	for (let i = 0; i < s.length; i++) {
		h ^= s.charCodeAt(i);
		h = Math.imul(h, 16777619);
	}
	return h >>> 0;
}
function dims(aspect) {
	switch (aspect) {
		case "16:9": return {
			w: 960,
			h: 540
		};
		case "9:16": return {
			w: 540,
			h: 960
		};
		case "3:2": return {
			w: 900,
			h: 600
		};
		case "2:3": return {
			w: 600,
			h: 900
		};
		default: return {
			w: 768,
			h: 768
		};
	}
}
function escapeXml(s) {
	return Array.from(s).map((ch) => {
		if (ch === "&") return "&amp;";
		if (ch === "<") return "&lt;";
		if (ch === ">") return "&gt;";
		if (ch === "\"") return "&quot;";
		return ch;
	}).join("");
}
/** Local offline Imagine preview (SVG) — desktop-ready without API keys. */
function renderImaginePreview(prompt, aspect) {
	const { w, h } = dims(aspect);
	const h1 = hash(prompt);
	const h2 = hash(prompt + "::b");
	const h3 = hash(prompt + "::c");
	const c1 = `hsl(${h1 % 360} 18% 12%)`;
	const c2 = `hsl(${h2 % 360} 22% 18%)`;
	const c3 = `hsl(${h3 % 360} 28% 42%)`;
	const accent = `hsl(${(h1 + 40) % 360} 35% 68%)`;
	const title = escapeXml(prompt.slice(0, 72) || "Imagine");
	const sub = "GrokHub · Imagine · local preview";
	const blobs = Array.from({ length: 5 }, (_, i) => {
		const hx = hash(`${prompt}-blob-${i}`);
		const cx = hx % 80 + 10;
		const cy = (hx >> 8) % 80 + 10;
		const r = (hx >> 16) % 28 + 12;
		const op = .12 + (hx >> 24) % 20 / 100;
		return `<circle cx="${cx / 100 * w}" cy="${cy / 100 * h}" r="${r / 100 * Math.min(w, h)}" fill="${accent}" opacity="${op}"/>`;
	}).join("");
	const svg = [
		`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">`,
		`<defs>`,
		`<linearGradient id="g" x1="0" y1="0" x2="1" y2="1">`,
		`<stop offset="0%" stop-color="${c1}"/>`,
		`<stop offset="55%" stop-color="${c2}"/>`,
		`<stop offset="100%" stop-color="${c3}"/>`,
		`</linearGradient>`,
		`<radialGradient id="v" cx="50%" cy="40%" r="65%">`,
		`<stop offset="0%" stop-color="${accent}" stop-opacity="0.35"/>`,
		`<stop offset="100%" stop-color="${c1}" stop-opacity="0"/>`,
		`</radialGradient>`,
		`</defs>`,
		`<rect width="100%" height="100%" fill="url(#g)"/>`,
		`<rect width="100%" height="100%" fill="url(#v)"/>`,
		blobs,
		`<rect x="24" y="${h - 108}" width="${w - 48}" height="72" rx="14" fill="rgba(10,10,11,0.55)" stroke="rgba(244,244,245,0.12)"/>`,
		`<text x="44" y="${h - 68}" fill="#f4f4f5" font-family="Segoe UI, system-ui, sans-serif" font-size="18" font-weight="600">${title}</text>`,
		`<text x="44" y="${h - 42}" fill="#a1a1aa" font-family="ui-monospace, SF Mono, Menlo, monospace" font-size="12">${sub}</text>`,
		`<text x="${w - 44}" y="40" text-anchor="end" fill="#a1a1aa" font-family="ui-monospace, SF Mono, Menlo, monospace" font-size="11">IMAGINE</text>`,
		`</svg>`
	].join("");
	return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
/** Catalog of integrations — all disconnected until the user connects them. */
function catalogConnectors() {
	return [
		{
			id: "grok-xai",
			name: "Grok (xAI)",
			category: "Grok",
			description: "Live Grok via SuperGrok / X Premium OAuth or API key.",
			status: "disconnected",
			tools: [
				"chat",
				"models",
				"imagine"
			]
		},
		{
			id: "desktop-host",
			name: "Desktop Host",
			category: "Local",
			description: "Unsandboxed shell, files, and apps on this Arch machine.",
			status: "disconnected",
			tools: [
				"exec",
				"list_dir",
				"read_file",
				"open_app"
			]
		},
		{
			id: "gmail",
			name: "Gmail & Calendar",
			category: "Google",
			description: "Mail, drafts, and calendar events.",
			status: "disconnected",
			tools: [
				"search_mail",
				"draft_reply",
				"list_events",
				"create_event"
			]
		},
		{
			id: "gdrive",
			name: "Google Drive",
			category: "Google",
			description: "Docs, Sheets, and Slides.",
			status: "disconnected",
			tools: [
				"search_files",
				"read_doc",
				"update_sheet"
			]
		},
		{
			id: "github",
			name: "GitHub",
			category: "Code",
			description: "Repos, issues, PRs, and code search.",
			status: "disconnected",
			tools: [
				"list_issues",
				"create_pr_comment",
				"search_code"
			]
		},
		{
			id: "notion",
			name: "Notion",
			category: "Workspace",
			description: "Pages and databases.",
			status: "disconnected",
			tools: [
				"search_pages",
				"update_page",
				"query_db"
			]
		},
		{
			id: "outlook",
			name: "Outlook Mail & Calendar",
			category: "Microsoft",
			description: "Inbox and meetings.",
			status: "disconnected",
			tools: [
				"search_inbox",
				"draft_mail",
				"create_meeting"
			]
		},
		{
			id: "teams",
			name: "Microsoft Teams",
			category: "Microsoft",
			description: "Channels and chats.",
			status: "disconnected",
			tools: [
				"list_channels",
				"post_message",
				"summarize_thread"
			]
		},
		{
			id: "linear",
			name: "Linear",
			category: "Projects",
			description: "Issues and projects.",
			status: "disconnected",
			tools: [
				"list_issues",
				"create_issue",
				"update_status"
			]
		},
		{
			id: "custom-mcp",
			name: "Custom MCP",
			category: "Custom",
			description: "Your own MCP server.",
			status: "disconnected",
			tools: ["discover_tools", "invoke_tool"]
		}
	];
}
/** Builtin skill templates only — zero runs, no custom personal workflows. */
function catalogSkills() {
	return [
		{
			id: "docs",
			name: "Office Documents",
			description: "Create Word, PowerPoint, Excel, and PDF files.",
			kind: "builtin",
			enabled: true,
			slash: "/docs",
			instructions: "Generate production-ready office documents with correct structure and styles.",
			runs: 0
		},
		{
			id: "skill-creator",
			name: "Skill Creator",
			description: "Capture a reusable workflow as a slash skill.",
			kind: "builtin",
			enabled: true,
			slash: "/skillify",
			instructions: "Turn the conversation into a named persistent skill.",
			runs: 0
		},
		{
			id: "deep-research",
			name: "Deep Research",
			description: "Parallel research with source checks.",
			kind: "builtin",
			enabled: true,
			slash: "/deep-research",
			instructions: "Break the question into sub-queries, verify claims, return a cited report.",
			runs: 0
		}
	];
}
function emptyThread(now) {
	return {
		id: `thread_${now}`,
		title: "New chat",
		createdAt: now,
		updatedAt: now,
		messages: [{
			id: `sys_${now}`,
			role: "system",
			content: "Welcome to GrokHub. Sign in with Grok (Settings or Sign in), add your xAI API key if needed, then start chatting. History appears in the sidebar as you go.",
			ts: now
		}]
	};
}
/** Fresh clean install — no personal preferences or fake history. */
function createSeeds(now = Date.now()) {
	const thread = emptyThread(now);
	return {
		connectors: catalogConnectors(),
		skills: catalogSkills(),
		automations: [],
		agents: [],
		activity: [],
		chat: thread.messages,
		threads: [thread],
		activeThreadId: thread.id,
		heartbeatAt: now
	};
}
createSeeds().connectors;
createSeeds().skills;
createSeeds().chat;
var PLAN_LIMITS = {
	free: {
		id: "free",
		label: "Free",
		units: 80,
		messages: 40,
		imagine: 5,
		automations: 10,
		host: 50,
		heavyAllowed: false,
		buildAllowed: false
	},
	super: {
		id: "super",
		label: "SuperGrok",
		units: 600,
		messages: 400,
		imagine: 60,
		automations: 120,
		host: 400,
		heavyAllowed: true,
		buildAllowed: true
	},
	pro: {
		id: "pro",
		label: "SuperGrok Pro",
		units: 2500,
		messages: 2e3,
		imagine: 250,
		automations: 500,
		host: 2e3,
		heavyAllowed: true,
		buildAllowed: true
	}
};
/** Mode-weighted compute cost per agent turn */
var MODE_UNIT_COST = {
	fast: 1,
	auto: 1.5,
	build: 2,
	expert: 4,
	heavy: 8
};
var BUCKET_UNIT_COST = {
	message: 1,
	imagine: 5,
	automation: 3,
	skill: 2,
	host: .25
};
function periodBounds(now = Date.now()) {
	const d = new Date(now);
	return {
		start: new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), 1)).getTime(),
		end: new Date(Date.UTC(d.getUTCFullYear(), d.getUTCMonth() + 1, 1)).getTime()
	};
}
function createUsage(plan = "pro", now = Date.now()) {
	const { start, end } = periodBounds(now);
	return {
		plan,
		periodStart: start,
		periodEnd: end,
		usedUnits: plan === "pro" ? 842 : plan === "super" ? 210 : 28,
		messages: plan === "pro" ? 186 : plan === "super" ? 72 : 12,
		imagine: plan === "pro" ? 24 : plan === "super" ? 8 : 2,
		automations: plan === "pro" ? 41 : plan === "super" ? 14 : 3,
		host: plan === "pro" ? 93 : plan === "super" ? 30 : 5,
		byMode: {
			auto: 22,
			fast: 48,
			expert: 31,
			heavy: 19,
			build: plan === "pro" ? 66 : 12
		}
	};
}
function ensurePeriod(u, now = Date.now()) {
	if (now < u.periodEnd && now >= u.periodStart) return u;
	return createUsage(u.plan, now);
}
function usagePercent(u) {
	const lim = PLAN_LIMITS[u.plan].units;
	if (lim <= 0) return 0;
	return Math.min(100, u.usedUnits / lim * 100);
}
function usageTone(pct) {
	if (pct >= 92) return "danger";
	if (pct >= 75) return "warn";
	return "ok";
}
function formatUnits(n) {
	if (n >= 1e3) return `${(n / 1e3).toFixed(n >= 1e4 ? 0 : 1)}k`;
	if (Number.isInteger(n)) return String(n);
	return n.toFixed(1);
}
function daysLeftInPeriod(u, now = Date.now()) {
	return Math.max(0, Math.ceil((u.periodEnd - now) / 864e5));
}
function costFor(bucket, mode) {
	if (bucket === "message" || bucket === "skill") return MODE_UNIT_COST[mode ?? "fast"] ?? 1;
	return BUCKET_UNIT_COST[bucket];
}
function replyFor(text, s, routed) {
	const lower = text.toLowerCase();
	const connected = s.connectors.filter((c) => c.status === "connected");
	const enabledSkills = s.skills.filter((sk) => sk.enabled);
	const depth = getMode(routed).depth;
	const plan = PLAN_LIMITS[s.usage.plan];
	const pct = Math.round(usagePercent(s.usage));
	if (lower.includes("usage") || lower.includes("quota") || lower.includes("limit") || lower.includes("subscription")) return [
		"Subscription usage",
		"",
		`Plan: ${plan.label}`,
		`Units: ${s.usage.usedUnits.toFixed(1)} / ${plan.units} (${pct}%)`,
		`Messages ${s.usage.messages}/${plan.messages} · Imagine ${s.usage.imagine}/${plan.imagine}`,
		`Automations ${s.usage.automations}/${plan.automations} · Host ${s.usage.host}/${plan.host}`,
		"",
		"Heavy = 8u · Expert = 4u · Build = 2u · Fast = 1u · Imagine = 5u",
		"Open Settings for the full meter and plan switcher."
	].join("\n");
	if (lower.startsWith("/morning") || lower.includes("morning brief")) {
		const core = [
			"",
			"Morning Brief",
			"",
			`- Connectors live: ${connected.map((c) => c.name).join(", ") || "none"}`,
			"- Calendar: 2 meetings after 13:00, free block 10:00–12:00",
			"- Inbox: 4 unread · 1 invoice reminder · 1 shipping notice",
			"- Linear: 2 P0s · GitHub: 3 review requests",
			`- Usage: ${pct}% of ${plan.label} period budget`
		];
		if (depth === "light") return [
			...core,
			"",
			"Top move: clear P0 Linear, then PR reviews."
		].join("\n");
		if (depth === "team") return [
			...core,
			"",
			"Team pass (Heavy):",
			"- Ops: confirm dependencies on the two P0s",
			"- Research: gather context from last related issue threads",
			"- Build: draft a short checklist skill if the workflow repeats",
			"- Primary: sequence deep work under 90 minutes"
		].join("\n");
		if (depth === "code") return [
			...core,
			"",
			"Build angle:",
			"- Ship GrokHub desktop install path first",
			"- Wire mode routing tests before new connectors",
			"- Package: Electron + Arch PKGBUILD ready"
		].join("\n");
		return [
			...core,
			"",
			"Suggested order: P0 Linear → PR reviews → inbox drafts → lunch buffer.",
			depth === "deep" ? "Risk: context switching across tools — batch connector work." : ""
		].filter(Boolean).join("\n");
	}
	if (lower.startsWith("/standup") || lower.includes("standup")) return [
		"",
		"Standup",
		"",
		"- Yesterday: connector triage + mode routing polish",
		"- Today: desktop host checks and packaging notes",
		"- Blockers: none — usage meter and Imagine ready for demos",
		depth === "code" ? "- Build: keep /standup skill logging shipped items weekly" : ""
	].filter(Boolean).join("\n");
	if (lower.includes("imagine") || lower.startsWith("/imagine")) return [
		"",
		"Imagine is available in the Imagine panel.",
		"Describe a scene there — GrokHub renders a local preview on this Arch desktop build.",
		`Imagine quota: ${s.usage.imagine}/${plan.imagine} this period (5 units each).`
	].join("\n");
	if (lower.includes("mode") || lower.includes("fast") || lower.includes("expert") || lower.includes("heavy")) return [
		"",
		"Baked-in Grok modes (same as web):",
		"- Auto — Chooses Fast or Expert",
		"- Fast — Quick responses · grok-4-1-fast · 1 unit",
		"- Expert — Thinks hard · grok-4.3 · 4 units",
		"- Heavy — Team of Experts · grok-4.3 · 8 units",
		"- Build — Build apps and sites · grok-code-fast-1 · 2 units",
		"",
		`Active: ${getMode(s.mode).label}${s.mode === "auto" ? ` (this turn → ${getMode(routed).label})` : ""}`,
		"Change modes from the titlebar picker or Settings."
	].join("\n");
	if (lower.includes("connector") || lower.includes("connect")) return [
		"",
		"Connector status",
		"",
		...s.connectors.map((c) => `- ${c.name}: ${c.status}`)
	].join("\n");
	if (lower.includes("automat") || lower.includes("schedule")) return [
		"",
		"Automations",
		"",
		...s.automations.map((a) => `- ${a.enabled ? "ON" : "OFF"} ${a.name} (${a.schedule} @ ${a.time}) · ${a.runCount} runs`)
	].join("\n");
	if (lower.includes("skill")) return [
		"",
		"Skills",
		"",
		...enabledSkills.map((sk) => `- ${sk.slash} — ${sk.name}`)
	].join("\n");
	if (depth === "code" || lower.includes("build") || lower.includes("arch") || lower.includes("desktop")) return [
		"",
		"Build / desktop plan",
		"",
		"GrokHub ships as an Electron shell for Arch Linux:",
		"- `desktop/main.mjs` — native window, tray, Wayland-friendly flags",
		"- `packaging/PKGBUILD` — makepkg install",
		"- Unsandboxed host: CLI, files, apps via Desktop tab or `$ command`",
		"",
		`Plan ${plan.label}: host CLI ${s.usage.host}/${plan.host} this period.`
	].join("\n");
	if (depth === "team") return [
		"",
		"Heavy · Team of Experts",
		"",
		`Goal: ${text}`,
		"",
		"1) Planner — break into 3 workstreams",
		"2) Researcher — pull connector context (mail/code/issues)",
		"3) Critic — risk + cheapest path",
		"4) Builder — ship checklist",
		"",
		`Context: ${connected.length} connectors · ${enabledSkills.length} skills · ${pct}% usage.`
	].join("\n");
	if (depth === "deep") return [
		"",
		"Expert analysis",
		"",
		`Reading: ${text}`,
		"",
		"Constraints: local-first control plane, Grok modes (Fast/Expert/Heavy/Build), Arch desktop target.",
		"Approach: gather connector state → apply enabled skills → leave run log.",
		"Tradeoff: Fast is cheaper/latency; Expert/Heavy spend units for depth.",
		"",
		`Live tools: ${connected.map((c) => c.name).join(", ") || "none connected"}.`
	].join("\n");
	if (depth === "light") return [
		"",
		`Got it — ${text.slice(0, 120)}${text.length > 120 ? "…" : ""}`,
		`Using ${connected.length} connectors · ${enabledSkills.length} skills · ${pct}% quota.`,
		"Say /morning, open Imagine, or switch to Expert for deeper work."
	].join("\n");
	return [
		"",
		"Primary co-pilot",
		"",
		`Goal: ${text}`,
		`Using ${connected.length} connectors and ${enabledSkills.length} enabled skills.`,
		"Next: break into steps → pull tools → run skills → log.",
		"Try /morning, /standup, Imagine, or Heavy mode for a team pass."
	].join("\n");
}
function emptyProfile() {
	return {
		displayName: null,
		email: null,
		imageUrl: null,
		models: [],
		connectedAt: null
	};
}
function titleFromMessages(messages) {
	const first = messages.find((m) => m.role === "user");
	if (!first) return "New chat";
	const t = first.content.replace(/\s+/g, " ").trim();
	return t.length > 48 ? t.slice(0, 48) + "…" : t || "New chat";
}
function initialFromSeeds() {
	const s = createSeeds();
	return {
		connectors: s.connectors,
		skills: s.skills,
		automations: s.automations,
		activity: s.activity,
		chat: s.chat,
		threads: s.threads,
		activeThreadId: s.activeThreadId,
		agents: s.agents,
		heartbeatAt: s.heartbeatAt,
		profile: emptyProfile()
	};
}
var boot = initialFromSeeds();
var useGrokHub = create()(persist((set, get) => ({
	nav: "chat",
	mode: "auto",
	modeMenuOpen: false,
	connectors: boot.connectors,
	skills: boot.skills,
	automations: boot.automations,
	activity: boot.activity,
	chat: boot.chat,
	threads: boot.threads,
	activeThreadId: boot.activeThreadId,
	agents: boot.agents,
	profile: boot.profile,
	imagineJobs: [],
	imaginePrompt: "",
	imagineAspect: "1:1",
	imagineBusy: false,
	imagineError: null,
	desktop: {
		startMinimized: false,
		launchOnLogin: false,
		wayland: true,
		tray: true
	},
	usage: createUsage("pro"),
	heartbeatAt: boot.heartbeatAt,
	running: false,
	streamStatus: null,
	streamingMessageId: null,
	modelCatalog: emptyCatalog(),
	lastModelsFetchAt: 0,
	apiKey: "",
	githubToken: "",
	oauth: null,
	oauthPending: null,
	grokConnected: null,
	grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",
	setNav: (nav) => set({
		nav,
		modeMenuOpen: false
	}),
	setMode: (mode) => {
		set({
			mode,
			modeMenuOpen: false
		});
		get().pushActivity({
			kind: "system",
			title: `Mode → ${getMode(mode).label}`,
			detail: getMode(mode).subtitle,
			status: "success"
		});
	},
	setModeMenuOpen: (open) => set({ modeMenuOpen: open }),
	setDesktop: (patch) => set((s) => ({ desktop: {
		...s.desktop,
		...patch
	} })),
	setApiKey: (key) => set({
		apiKey: key,
		grokConnected: null
	}),
	setGithubToken: (token) => set({ githubToken: token }),
	startGrokOAuth: async () => {
		const { oauthStart } = await import("./grok-client-CS5ThB0G.mjs");
		const start = await oauthStart();
		set({
			oauthPending: {
				deviceCode: start.deviceCode,
				userCode: start.userCode,
				verificationUri: start.verificationUri,
				verificationUriComplete: start.verificationUriComplete,
				expiresAt: Date.now() + (start.expiresIn || 1800) * 1e3
			},
			grokStatusDetail: `Approve code ${start.userCode} at accounts.x.ai`
		});
		get().pushActivity({
			kind: "auth",
			title: "Grok OAuth started",
			detail: `Enter code ${start.userCode}`,
			status: "running"
		});
	},
	pollGrokOAuth: async () => {
		const pending = get().oauthPending;
		if (!pending) return "failed";
		if (Date.now() > pending.expiresAt) {
			set({
				oauthPending: null,
				grokStatusDetail: "OAuth code expired — start again"
			});
			return "failed";
		}
		const { oauthPoll } = await import("./grok-client-CS5ThB0G.mjs");
		const r = await oauthPoll(pending.deviceCode);
		if (r.status === "ready") {
			set({
				oauth: r.tokens,
				oauthPending: null,
				grokConnected: true,
				grokStatusDetail: `Grok OAuth · ${r.tokens.email || r.tokens.name || "connected"}`
			});
			set((s) => ({ connectors: s.connectors.map((c) => c.id === "custom-mcp" || c.name.toLowerCase().includes("grok") ? c : c) }));
			await get().syncFromGrok({
				displayName: r.tokens.name ?? null,
				email: r.tokens.email ?? null,
				imageUrl: r.tokens.picture ?? null
			});
			set((s) => {
				return { connectors: s.connectors.some((c) => c.id === "grok-xai") ? s.connectors.map((c) => c.id === "grok-xai" ? {
					...c,
					status: "connected",
					lastUsed: Date.now()
				} : c) : [{
					id: "grok-xai",
					name: "Grok (xAI)",
					category: "Grok",
					description: "Live Grok via SuperGrok / X Premium OAuth or API key.",
					status: "connected",
					tools: [
						"chat",
						"models",
						"imagine"
					],
					lastUsed: Date.now()
				}, ...s.connectors] };
			});
			get().pushActivity({
				kind: "auth",
				title: "Grok OAuth connected",
				detail: r.tokens.email || r.tokens.name || "Session active",
				status: "success"
			});
			return "ready";
		}
		if (r.status === "expired" || r.status === "denied") {
			set({
				oauthPending: null,
				grokConnected: false,
				grokStatusDetail: r.error || "OAuth failed"
			});
			get().pushActivity({
				kind: "auth",
				title: "Grok OAuth failed",
				detail: r.error,
				status: "failed"
			});
			return "failed";
		}
		return "pending";
	},
	clearGrokOAuth: () => {
		set({
			oauth: null,
			oauthPending: null,
			grokConnected: get().apiKey ? null : false,
			grokStatusDetail: "Grok OAuth cleared"
		});
		set((s) => ({ connectors: s.connectors.map((c) => c.id === "grok-xai" ? {
			...c,
			status: "disconnected"
		} : c) }));
		get().pushActivity({
			kind: "auth",
			title: "Grok OAuth signed out",
			detail: "Session removed from this device",
			status: "success"
		});
	},
	probeGrok: async () => {
		try {
			const { grokProbe, oauthEnsure } = await import("./grok-client-CS5ThB0G.mjs");
			let accessToken = get().oauth?.accessToken;
			if (get().oauth) try {
				const ensured = await oauthEnsure(get().oauth);
				if (ensured.tokens) set({ oauth: ensured.tokens });
				accessToken = ensured.tokens?.accessToken || accessToken;
				if (ensured.ok) {
					set({
						grokConnected: true,
						grokStatusDetail: ensured.detail || "Grok OAuth live"
					});
					return true;
				}
			} catch (e) {
				const msg = e instanceof Error ? e.message : "oauth ensure failed";
				if (!get().apiKey) {
					set({
						grokConnected: false,
						grokStatusDetail: msg
					});
					return false;
				}
			}
			const r = await grokProbe({
				apiKey: get().apiKey || void 0,
				accessToken
			});
			set({
				grokConnected: r.ok,
				grokStatusDetail: r.detail + (r.authMode === "oauth" ? " · OAuth" : r.envConfigured && !get().apiKey && !accessToken ? " (env key)" : r.authMode === "apiKey" ? " · API key" : "")
			});
			return r.ok;
		} catch (e) {
			set({
				grokConnected: false,
				grokStatusDetail: e instanceof Error ? e.message : "probe failed"
			});
			return false;
		}
	},
	syncFromGrok: async (opts) => {
		const models = [];
		try {
			const key = get().apiKey || "";
			const accessToken = get().oauth?.accessToken || "";
			const res = await fetch("/api/grok", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					action: "models",
					apiKey: key,
					accessToken
				})
			});
			if (res.ok) {
				const data = await res.json();
				if (Array.isArray(data.models)) models.push(...data.models.filter(Boolean));
			}
		} catch {}
		const now = Date.now();
		const catalog = models.length ? buildCatalog(models, get().modelCatalog) : get().modelCatalog || emptyCatalog();
		set((st) => ({
			profile: {
				displayName: opts?.displayName ?? st.profile.displayName,
				email: opts?.email ?? st.profile.email,
				imageUrl: opts?.imageUrl ?? st.profile.imageUrl,
				models: catalog.essential.length ? catalog.essential : st.profile.models,
				connectedAt: st.profile.connectedAt ?? (st.grokConnected ? now : null)
			},
			modelCatalog: catalog,
			lastModelsFetchAt: models.length ? now : st.lastModelsFetchAt,
			agents: st.agents.length > 0 ? st.agents : [{
				id: "primary",
				name: (opts?.displayName || "Primary").split(/\s+/)[0] || "Primary",
				role: "Primary co-pilot",
				model: "Grok · Auto",
				status: "idle",
				tasks: 0,
				color: "#d4d4d8"
			}, {
				id: "builder",
				name: "Build",
				role: "Build mode",
				model: "Grok · Build",
				status: "idle",
				tasks: 0,
				color: "#7dd3fc"
			}]
		}));
		if (opts?.displayName || opts?.email || models.length) get().pushActivity({
			kind: "auth",
			title: "Grok profile synced",
			detail: opts?.displayName || opts?.email || `${catalog.essential.length} essential models (${catalog.source})`,
			status: "success"
		});
		if (models.length) get().refreshModels();
	},
	refreshModels: async (opts) => {
		try {
			const st = get();
			const res = await fetch("/api/grok", {
				method: "POST",
				headers: { "content-type": "application/json" },
				body: JSON.stringify({
					action: "models",
					apiKey: st.apiKey || "",
					accessToken: st.oauth?.accessToken || ""
				})
			});
			let models = [];
			if (res.ok) {
				const data = await res.json();
				if (Array.isArray(data.models)) models = data.models.filter(Boolean);
			}
			if (!models.length) {
				set({ lastModelsFetchAt: Date.now() });
				return;
			}
			let catalog = buildCatalog(models, st.modelCatalog);
			const shouldClassify = Boolean(st.oauth?.accessToken || st.apiKey || st.grokConnected) && (Boolean(opts?.force) || needsGrokClassification(catalog));
			if (shouldClassify) try {
				const cRes = await fetch("/api/grok", {
					method: "POST",
					headers: { "content-type": "application/json" },
					body: JSON.stringify({
						action: "classifyModels",
						models,
						apiKey: st.apiKey || "",
						accessToken: st.oauth?.accessToken || "",
						tokens: st.oauth || void 0
					})
				});
				if (cRes.ok) {
					const cData = await cRes.json();
					if (cData.ok && cData.plan) catalog = applyGrokPlan(catalog, cData.plan);
				}
			} catch {}
			set((s) => ({
				modelCatalog: catalog,
				lastModelsFetchAt: Date.now(),
				profile: {
					...s.profile,
					models: catalog.essential
				},
				grokStatusDetail: s.grokConnected ? `Live · ${catalog.essential.length} models · slots by ${catalog.classifiedBy}` : s.grokStatusDetail
			}));
			if (catalog.classifiedBy === "grok" && shouldClassify) get().pushActivity({
				kind: "system",
				title: "Model slots updated by Grok",
				detail: catalog.classifyNotes || `Fast ${catalog.slots.fast} · Smart ${catalog.slots.smart} · Build ${catalog.slots.build}`,
				status: "success"
			});
		} catch {
			set({ lastModelsFetchAt: Date.now() });
		}
	},
	newThread: () => {
		const now = Date.now();
		const thread = {
			id: uid("thread"),
			title: "New chat",
			createdAt: now,
			updatedAt: now,
			messages: [{
				id: uid("msg"),
				role: "system",
				content: "New chat. Ask Grok anything — modes apply from the picker.",
				ts: now
			}]
		};
		set((s) => ({
			threads: [thread, ...s.threads],
			activeThreadId: thread.id,
			chat: thread.messages,
			nav: "chat"
		}));
	},
	selectThread: (id) => {
		const t = get().threads.find((x) => x.id === id);
		if (!t) return;
		set({
			activeThreadId: id,
			chat: t.messages,
			nav: "chat",
			mode: t.mode || get().mode
		});
	},
	deleteThread: (id) => {
		const remaining = get().threads.filter((t) => t.id !== id);
		if (remaining.length === 0) {
			get().newThread();
			return;
		}
		const nextActive = get().activeThreadId === id ? remaining[0] : remaining.find((t) => t.id === get().activeThreadId) || remaining[0];
		set({
			threads: remaining,
			activeThreadId: nextActive.id,
			chat: nextActive.messages
		});
	},
	setPlan: (plan) => {
		const prev = get().usage;
		const next = ensurePeriod({
			...createUsage(plan),
			usedUnits: 0,
			messages: 0,
			imagine: 0,
			automations: 0,
			host: 0,
			byMode: {
				auto: 0,
				fast: 0,
				expert: 0,
				heavy: 0,
				build: 0
			}
		});
		set({ usage: {
			...createUsage(plan),
			periodStart: next.periodStart,
			periodEnd: next.periodEnd,
			plan
		} });
		get().pushActivity({
			kind: "usage",
			title: `Plan → ${PLAN_LIMITS[plan].label}`,
			detail: `Limit ${PLAN_LIMITS[plan].units} units / month (was ${PLAN_LIMITS[prev.plan].label})`,
			status: "success"
		});
	},
	recordUsage: (bucket, mode) => {
		const cost = costFor(bucket, mode);
		let ok = true;
		set((s) => {
			const base = ensurePeriod(s.usage);
			const lim = PLAN_LIMITS[base.plan];
			if (base.usedUnits + cost > lim.units * 1.02) {
				ok = false;
				return { usage: base };
			}
			const byMode = { ...base.byMode };
			if ((bucket === "message" || bucket === "skill") && mode) byMode[mode] = (byMode[mode] ?? 0) + 1;
			return { usage: {
				...base,
				usedUnits: Math.round((base.usedUnits + cost) * 100) / 100,
				messages: base.messages + (bucket === "message" ? 1 : 0),
				imagine: base.imagine + (bucket === "imagine" ? 1 : 0),
				automations: base.automations + (bucket === "automation" ? 1 : 0),
				host: base.host + (bucket === "host" ? 1 : 0),
				byMode
			} };
		});
		if (!ok) get().pushActivity({
			kind: "usage",
			title: "Quota exceeded",
			detail: `${PLAN_LIMITS[get().usage.plan].label} period limit reached`,
			status: "failed"
		});
		return {
			ok,
			cost
		};
	},
	resetUsagePeriod: () => {
		const plan = get().usage.plan;
		set({ usage: {
			...createUsage(plan),
			usedUnits: 0,
			messages: 0,
			imagine: 0,
			automations: 0,
			host: 0,
			byMode: {
				auto: 0,
				fast: 0,
				expert: 0,
				heavy: 0,
				build: 0
			}
		} });
		get().pushActivity({
			kind: "usage",
			title: "Billing period reset",
			detail: `${PLAN_LIMITS[plan].label} counters cleared`,
			status: "success"
		});
	},
	toggleConnector: (id) => {
		get().connectConnector(id);
	},
	connectConnector: async (id) => {
		const c = get().connectors.find((x) => x.id === id);
		if (!c) return;
		if (c.status === "connected") {
			if (id === "grok-xai") get().clearGrokOAuth();
			set((s) => ({ connectors: s.connectors.map((row) => row.id === id ? {
				...row,
				status: "disconnected",
				lastUsed: row.lastUsed
			} : row) }));
			get().pushActivity({
				kind: "connector",
				title: `Disconnected ${c.name}`,
				detail: "Connector turned off",
				status: "success"
			});
			return;
		}
		if (id === "grok-xai") {
			if (get().oauth?.accessToken || get().apiKey) {
				set((s) => ({
					connectors: s.connectors.map((row) => row.id === id ? {
						...row,
						status: "connected",
						lastUsed: Date.now()
					} : row),
					grokConnected: true
				}));
				get().pushActivity({
					kind: "connector",
					title: "Grok connected",
					detail: get().oauth?.email || "Session active",
					status: "success"
				});
				return;
			}
			set({ nav: "settings" });
			get().pushActivity({
				kind: "connector",
				title: "Connect Grok first",
				detail: "Settings → Connect with Grok OAuth",
				status: "failed"
			});
			return;
		}
		if (id === "desktop-host") {
			try {
				const { hostInfo } = await import("./host-client-WUUmAwRI.mjs");
				const info = await hostInfo();
				if (info.bridge === "none" || !info.unsandboxed) {
					get().pushActivity({
						kind: "connector",
						title: "Desktop host offline",
						detail: "Relaunch the Electron desktop app for unsandboxed access",
						status: "failed"
					});
					set((s) => ({ connectors: s.connectors.map((row) => row.id === id ? {
						...row,
						status: "error"
					} : row) }));
					return;
				}
				set((s) => ({ connectors: s.connectors.map((row) => row.id === id ? {
					...row,
					status: "connected",
					lastUsed: Date.now()
				} : row) }));
				get().pushActivity({
					kind: "connector",
					title: "Desktop host connected",
					detail: `${info.user}@${info.hostname} · ${info.bridge}`,
					status: "success"
				});
			} catch (e) {
				get().pushActivity({
					kind: "connector",
					title: "Desktop host failed",
					detail: e instanceof Error ? e.message : "error",
					status: "failed"
				});
			}
			return;
		}
		if (id === "github") {
			const token = get().githubToken?.trim();
			if (!token) {
				set({ nav: "settings" });
				get().pushActivity({
					kind: "connector",
					title: "GitHub token required",
					detail: "Settings → Updates → paste a GitHub token (repo scope)",
					status: "failed"
				});
				return;
			}
			try {
				const res = await fetch("https://api.github.com/user", { headers: {
					authorization: `Bearer ${token}`,
					accept: "application/vnd.github+json",
					"user-agent": "GrokHub"
				} });
				if (!res.ok) throw new Error(`GitHub ${res.status}`);
				const user = await res.json();
				set((s) => ({ connectors: s.connectors.map((row) => row.id === id ? {
					...row,
					status: "connected",
					lastUsed: Date.now()
				} : row) }));
				get().pushActivity({
					kind: "connector",
					title: "GitHub connected",
					detail: user.login || "token ok",
					status: "success"
				});
			} catch (e) {
				get().pushActivity({
					kind: "connector",
					title: "GitHub connect failed",
					detail: e instanceof Error ? e.message : "error",
					status: "failed"
				});
			}
			return;
		}
		const url = {
			gmail: "https://accounts.google.com/",
			gdrive: "https://drive.google.com/",
			notion: "https://www.notion.so/login",
			outlook: "https://outlook.live.com/",
			teams: "https://teams.microsoft.com/",
			linear: "https://linear.app/",
			"custom-mcp": ""
		}[id];
		if (url && typeof window !== "undefined") window.open(url, "_blank", "noopener,noreferrer");
		set((s) => ({ connectors: s.connectors.map((row) => row.id === id ? {
			...row,
			status: "connected",
			lastUsed: Date.now()
		} : row) }));
		get().pushActivity({
			kind: "connector",
			title: `Enabled ${c.name}`,
			detail: id === "custom-mcp" ? "Mark enabled — point MCP URL from Grok skills when available" : "Enabled for agent context. Finish account sign-in in the browser if prompted.",
			status: "success"
		});
	},
	toggleSkill: (id) => {
		set((s) => ({ skills: s.skills.map((sk) => sk.id === id ? {
			...sk,
			enabled: !sk.enabled
		} : sk) }));
	},
	addSkill: (input) => {
		const skill = {
			id: uid("skill"),
			name: input.name,
			description: input.description,
			kind: "custom",
			enabled: true,
			slash: input.slash.startsWith("/") ? input.slash : `/${input.slash}`,
			instructions: input.instructions,
			runs: 0
		};
		set((s) => ({ skills: [skill, ...s.skills] }));
		get().pushActivity({
			kind: "skill",
			title: `Created skill ${skill.name}`,
			detail: skill.slash,
			status: "success"
		});
	},
	runSkill: async (id) => {
		const skill = get().skills.find((s) => s.id === id);
		if (!skill) return;
		const mode = get().mode;
		const routed = resolveMode(mode, skill.instructions);
		const m = getMode(routed);
		const bill = get().recordUsage("skill", routed);
		if (!bill.ok) return;
		set({ running: true });
		get().setAgentStatus("primary", "working", 1);
		get().pushActivity({
			kind: "skill",
			title: `Running ${skill.name}`,
			detail: `${skill.slash} · ${m.label} · ${bill.cost}u`,
			status: "running"
		});
		await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
		set((s) => ({
			running: false,
			skills: s.skills.map((sk) => sk.id === id ? {
				...sk,
				runs: sk.runs + 1
			} : sk)
		}));
		get().setAgentStatus("primary", "idle", 0);
		get().pushActivity({
			kind: "skill",
			title: `${skill.name} finished`,
			detail: skill.instructions.slice(0, 120),
			status: "success"
		});
		set((s) => ({ chat: [...s.chat, {
			id: uid("msg"),
			role: "assistant",
			content: replyFor(skill.slash, get(), routed),
			ts: Date.now(),
			mode: routed
		}] }));
	},
	toggleAutomation: (id) => {
		set((s) => ({ automations: s.automations.map((a) => a.id === id ? {
			...a,
			enabled: !a.enabled
		} : a) }));
	},
	runAutomation: async (id) => {
		const auto = get().automations.find((a) => a.id === id);
		if (!auto) return;
		const m = getMode(resolveMode(get().mode, auto.instructions));
		const bill = get().recordUsage("automation");
		if (!bill.ok) return;
		set({ running: true });
		get().setAgentStatus("ops", "working", 1);
		get().pushActivity({
			kind: "automation",
			title: `Automation started: ${auto.name}`,
			detail: `${auto.instructions.slice(0, 100)} · ${m.label} · ${bill.cost}u`,
			status: "running"
		});
		await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
		set((s) => ({
			running: false,
			automations: s.automations.map((a) => a.id === id ? {
				...a,
				lastRun: Date.now(),
				runCount: a.runCount + 1
			} : a)
		}));
		get().setAgentStatus("ops", "idle", 0);
		get().pushActivity({
			kind: "automation",
			title: `Automation completed: ${auto.name}`,
			detail: `Used connectors: ${auto.connectorIds.join(", ") || "none"}`,
			status: "success"
		});
	},
	addAutomation: (input) => {
		const auto = {
			id: uid("auto"),
			name: input.name,
			instructions: input.instructions,
			schedule: input.schedule,
			time: input.time,
			enabled: true,
			connectorIds: get().connectors.filter((c) => c.status === "connected").slice(0, 2).map((c) => c.id),
			skillIds: [],
			runCount: 0,
			nextRun: Date.now() + 1e3 * 60 * 60 * 24
		};
		set((s) => ({ automations: [auto, ...s.automations] }));
		get().pushActivity({
			kind: "automation",
			title: `Created automation ${auto.name}`,
			detail: `${auto.schedule} @ ${auto.time}`,
			status: "success"
		});
	},
	stopChat: () => {
		++chatGeneration;
		try {
			activeChatAbort?.abort();
		} catch {}
		activeChatAbort = null;
		set((s) => {
			const sid = s.streamingMessageId;
			return {
				chat: s.chat.map((m) => m.id === sid ? {
					...m,
					streaming: false,
					stopped: true,
					content: m.content?.trim() ? `${m.content}${m.content.endsWith("\n") ? "" : "\n"}\n_Stopped._` : "_Stopped._"
				} : m),
				running: false,
				streamStatus: null,
				streamingMessageId: null
			};
		});
		get().setAgentStatus("primary", "idle", 0);
		get().setAgentStatus("builder", "idle", 0);
		get().setAgentStatus("research", "idle", 0);
		get().setAgentStatus("ops", "idle", 0);
		get().pushActivity({
			kind: "chat",
			title: "Stopped",
			detail: "User interrupted the agent",
			status: "failed"
		});
	},
	sendChat: async (text) => {
		const trimmed = text.trim();
		if (!trimmed) return;
		if (get().running) return;
		const mode = get().mode;
		const catalog = get().modelCatalog || emptyCatalog();
		const auto = autoRouteFor(trimmed, catalog);
		if (mode === "auto" && auto.openImagine) {
			set({
				nav: "imagine",
				imaginePrompt: trimmed
			});
			return;
		}
		const routed = resolveModeWithCatalog(mode, trimmed, catalog);
		const m = getMode(routed);
		const bill = get().recordUsage("message", routed);
		if (!bill.ok) {
			set((s) => ({ chat: [
				...s.chat,
				{
					id: uid("msg"),
					role: "user",
					content: trimmed,
					ts: Date.now(),
					mode
				},
				{
					id: uid("msg"),
					role: "system",
					content: `Quota exceeded on ${PLAN_LIMITS[get().usage.plan].label}. Wait for period reset or switch plan in Settings.`,
					ts: Date.now()
				}
			] }));
			return;
		}
		const userMsg = {
			id: uid("msg"),
			role: "user",
			content: trimmed,
			ts: Date.now(),
			mode
		};
		const botId = uid("msg");
		const botPlaceholder = {
			id: botId,
			role: "assistant",
			content: "",
			ts: Date.now(),
			mode: routed,
			streaming: true
		};
		try {
			activeChatAbort?.abort();
		} catch {}
		const abort = new AbortController();
		activeChatAbort = abort;
		const gen = ++chatGeneration;
		set((s) => ({
			chat: [
				...s.chat,
				userMsg,
				botPlaceholder
			],
			running: true,
			streamStatus: mode === "auto" ? `Auto → ${auto.reason}` : `Thinking · ${m.label}…`,
			streamingMessageId: botId
		}));
		if (get().agents.length === 0) await get().syncFromGrok();
		get().setAgentStatus(routed === "build" ? "builder" : routed === "heavy" ? "research" : "primary", "working", 1);
		if (routed === "heavy") {
			get().setAgentStatus("ops", "working", 1);
			get().setAgentStatus("builder", "working", 1);
		}
		const patchBot = (content, extra) => {
			if (gen !== chatGeneration) return;
			set((s) => ({ chat: s.chat.map((row) => row.id === botId ? {
				...row,
				content,
				...extra
			} : row) }));
		};
		const isLocalSlash = trimmed.startsWith("/morning") || trimmed.startsWith("/standup") || trimmed.startsWith("/docs") || trimmed.startsWith("/prints");
		let usedLive = false;
		let finalAnswer = "";
		let aborted = false;
		const { extractHostCommands, stripHostCommands, inferHostCommandsFromUser } = await import("./grok-B6vT5sj0.mjs");
		try {
			if (isLocalSlash) {
				set({ streamStatus: "Running skill…" });
				await wait(280);
				if (abort.signal.aborted || gen !== chatGeneration) aborted = true;
				else {
					finalAnswer = replyFor(trimmed, get(), routed);
					patchBot(finalAnswer, { streaming: false });
				}
			} else {
				const { grokChatStream } = await import("./grok-client-CS5ThB0G.mjs");
				const history = get().chat.filter((c) => c.role === "user" || c.role === "assistant").filter((c) => c.id !== botId).slice(-16).map((c) => ({
					role: c.role,
					content: c.role === "assistant" ? stripAssistantChrome(c.content) : c.content
				})).filter((c) => c.content.trim().length > 0);
				if (!history.length || history[history.length - 1]?.content !== trimmed) history.push({
					role: "user",
					content: trimmed
				});
				const modelId = modelIdForMode(mode, trimmed, catalog);
				if (mode === "auto") set({ streamStatus: `Auto → ${auto.reason}` });
				let rounds = 0;
				const maxRounds = 4;
				let accumulated = "";
				while (rounds < maxRounds) {
					rounds += 1;
					if (abort.signal.aborted || gen !== chatGeneration) {
						aborted = true;
						break;
					}
					set({ streamStatus: rounds === 1 ? "Streaming…" : `Host tool round ${rounds}…` });
					let roundText = "";
					const result = await grokChatStream({
						messages: history,
						mode: routed,
						model: modelId,
						apiKey: get().apiKey || void 0,
						accessToken: get().oauth?.accessToken,
						tokens: get().oauth
					}, {
						signal: abort.signal,
						onStatus: (st) => {
							if (gen !== chatGeneration) return;
							set({ streamStatus: st === "streaming" ? "Streaming…" : st === "fallback" ? "Responding…" : st === "connecting" ? "Connecting…" : st });
						},
						onDelta: (piece) => {
							if (gen !== chatGeneration) return;
							roundText += piece;
							accumulated = roundText;
							patchBot(stripHostCommands(roundText) || "…", { streaming: true });
						}
					});
					if (result.tokens) set({ oauth: result.tokens });
					if (result.aborted || abort.signal.aborted || gen !== chatGeneration) {
						aborted = true;
						break;
					}
					if (result.ok && (result.content || roundText)) {
						usedLive = true;
						const full = stripAssistantChrome(result.content || roundText);
						const visible = stripHostCommands(full);
						accumulated = full;
						patchBot(visible || "Working on your machine…", { streaming: true });
						set({
							grokConnected: true,
							grokStatusDetail: `Live · ${result.model || modelId}`
						});
						let cmds = extractHostCommands(full);
						if (!cmds.length && rounds === 1) cmds = inferHostCommandsFromUser(trimmed);
						if (!cmds.length) {
							finalAnswer = visible || full;
							break;
						}
						set({ streamStatus: "Running on your desktop…" });
						const { hostExec } = await import("./host-client-WUUmAwRI.mjs");
						const outputs = [];
						for (const cmd of cmds.slice(0, 3)) {
							if (abort.signal.aborted || gen !== chatGeneration) {
								aborted = true;
								break;
							}
							set({ streamStatus: `Host: ${cmd.slice(0, 56)}…` });
							patchBot(`${visible || "Checking your machine…"}\n\n_Running_\n\`$ ${cmd}\``, { streaming: true });
							try {
								const r = await hostExec(cmd, void 0, 45e3);
								outputs.push([
									`$ ${cmd}`,
									`exit ${r.code ?? "?"} · ${r.ms}ms · ${r.cwd}`,
									r.stdout || "(no stdout)",
									r.stderr ? `[stderr]\n${r.stderr}` : ""
								].filter(Boolean).join("\n"));
							} catch (e) {
								outputs.push(`$ ${cmd}\n[host error] ${e instanceof Error ? e.message : "failed"}`);
							}
						}
						if (aborted) break;
						const toolBlock = [
							"HOST_RESULT (authoritative — use this, do not invent files):",
							outputs.join("\n\n---\n\n"),
							"",
							"Summarize these results for the user in plain language. Do not output HOST_CMD again unless you need another command."
						].join("\n");
						history.push({
							role: "assistant",
							content: full
						});
						history.push({
							role: "user",
							content: toolBlock
						});
						const mid = [
							visible || "Checked your machine.",
							"",
							"```",
							outputs.join("\n\n"),
							"```",
							"",
							"_Summarizing…_"
						].join("\n");
						patchBot(mid, { streaming: true });
						accumulated = mid;
						continue;
					}
					const hasOauth = Boolean(get().oauth?.accessToken);
					const err = result.error || "Unknown error";
					finalAnswer = [
						"Could not reach Grok.",
						err,
						"",
						hasOauth ? "Your OAuth session is saved. If this keeps failing: Settings → Disconnect → Connect with Grok OAuth again, or paste an xAI API key as fallback." : "Fix: Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
						"",
						replyFor(trimmed, get(), routed)
					].join("\n");
					set({
						grokConnected: false,
						grokStatusDetail: hasOauth ? `OAuth session · chat failed: ${err}` : err
					});
					patchBot(finalAnswer, { streaming: false });
					break;
				}
				if (!finalAnswer && accumulated && !aborted) finalAnswer = stripHostCommands(stripAssistantChrome(accumulated.replace(/\n_Working…_\s*$/, "").replace(/\n_Summarizing…_\s*$/, "")));
			}
		} catch (e) {
			if (abort.signal.aborted || gen !== chatGeneration) aborted = true;
			else {
				const msg = e instanceof Error ? e.message : "request failed";
				finalAnswer = [
					`Grok connection error: ${msg}`,
					"",
					replyFor(trimmed, get(), routed)
				].join("\n");
				set({
					grokConnected: false,
					grokStatusDetail: msg
				});
				patchBot(finalAnswer, { streaming: false });
			}
		}
		if (gen !== chatGeneration) return;
		if (aborted) {
			if (get().running) set((s) => ({
				running: false,
				streamStatus: null,
				streamingMessageId: null,
				chat: s.chat.map((row) => row.id === botId ? {
					...row,
					streaming: false,
					stopped: true,
					content: row.content?.trim() ? `${row.content}${row.content.endsWith("\n") ? "" : "\n"}\n_Stopped._` : "_Stopped._"
				} : row)
			}));
		} else {
			const answer = stripHostCommands(stripAssistantChrome(finalAnswer || ""));
			set((s) => {
				const chat = s.chat.map((row) => row.id === botId ? {
					...row,
					content: answer || row.content || "(empty)",
					streaming: false,
					stopped: false,
					ts: Date.now(),
					mode: routed
				} : row);
				const tid = s.activeThreadId;
				return {
					chat,
					threads: s.threads.map((th) => th.id === tid ? {
						...th,
						messages: chat,
						updatedAt: Date.now(),
						title: titleFromMessages(chat),
						mode: routed
					} : th),
					running: false,
					streamStatus: null,
					streamingMessageId: null
				};
			});
			get().pushActivity({
				kind: "chat",
				title: usedLive ? `Grok · ${m.label}` : `Agent reply · ${m.label}`,
				detail: `${trimmed.slice(0, 80)} · ${bill.cost}u`,
				status: usedLive ? "success" : "failed"
			});
		}
		if (activeChatAbort === abort) activeChatAbort = null;
		get().setAgentStatus("primary", "idle", 0);
		get().setAgentStatus("builder", "idle", 0);
		get().setAgentStatus("research", "idle", 0);
		get().setAgentStatus("ops", "idle", 0);
	},
	setImaginePrompt: (v) => set({ imaginePrompt: v }),
	setImagineAspect: (v) => set({ imagineAspect: v }),
	runImagine: async (prompt) => {
		const p = (prompt ?? get().imaginePrompt).trim();
		if (!p) return;
		const bill = get().recordUsage("imagine");
		if (!bill.ok) {
			get().pushActivity({
				kind: "imagine",
				title: "Imagine blocked",
				detail: "Usage quota exceeded — wait for period reset or switch plan in Settings",
				status: "failed"
			});
			return;
		}
		const aspect = get().imagineAspect;
		const mode = get().mode;
		const id = uid("img");
		const job = {
			id,
			prompt: p,
			aspect,
			ts: Date.now(),
			status: "rendering",
			mode
		};
		set((s) => ({
			imagineJobs: [job, ...s.imagineJobs].slice(0, 24),
			imagineBusy: true,
			imaginePrompt: p,
			imagineError: null
		}));
		get().pushActivity({
			kind: "imagine",
			title: "Imagine rendering",
			detail: `${p.slice(0, 100)} · ${bill.cost}u`,
			status: "running"
		});
		let imageDataUrl;
		let source = "local";
		let model;
		let err = null;
		try {
			const { grokImagine } = await import("./grok-client-CS5ThB0G.mjs");
			const live = await grokImagine({
				prompt: p,
				apiKey: get().apiKey || void 0,
				accessToken: get().oauth?.accessToken,
				tokens: get().oauth
			});
			if (live.ok && live.imageDataUrl) {
				imageDataUrl = live.imageDataUrl;
				source = "xai";
				model = live.model;
				if (live.tokens) set({ oauth: live.tokens });
			} else err = live.error || "live Imagine unavailable";
		} catch (e) {
			err = e instanceof Error ? e.message : "Imagine request failed";
		}
		if (!imageDataUrl) {
			imageDataUrl = renderImaginePreview(p, aspect);
			source = "local";
		}
		set((s) => ({
			imagineBusy: false,
			imagineError: source === "local" && err ? err : null,
			imagineJobs: s.imagineJobs.map((j) => j.id === id ? {
				...j,
				status: "ready",
				imageDataUrl
			} : j)
		}));
		get().pushActivity({
			kind: "imagine",
			title: source === "xai" ? "Imagine ready (Grok)" : "Imagine ready (local preview)",
			detail: source === "xai" ? `${p.slice(0, 80)} · ${model || "xAI"}` : `${p.slice(0, 80)}${err ? ` · live failed: ${err}` : " · offline SVG"}`,
			status: "success"
		});
	},
	pushActivity: (item) => {
		const row = {
			id: uid("act"),
			ts: item.ts ?? Date.now(),
			kind: item.kind,
			title: item.title,
			detail: item.detail,
			status: item.status
		};
		set((s) => ({ activity: [row, ...s.activity].slice(0, 80) }));
	},
	tickHeartbeat: () => set((s) => ({
		heartbeatAt: Date.now(),
		usage: ensurePeriod(s.usage)
	})),
	setAgentStatus: (id, status, tasks) => {
		set((s) => ({ agents: s.agents.map((a) => a.id === id ? {
			...a,
			status,
			tasks: typeof tasks === "number" ? tasks : a.tasks
		} : a) }));
	},
	refreshStaleTimes: () => {
		const now = Date.now();
		const DAY = 864e5;
		const activity = get().activity;
		const chat = get().chat;
		if (now - Math.min(...activity.map((a) => a.ts), ...chat.map((m) => m.ts), now) < 2 * DAY) {
			set((s) => ({
				heartbeatAt: now,
				usage: ensurePeriod(s.usage)
			}));
			return;
		}
		const fresh = createSeeds(now);
		set((s) => ({
			activity: fresh.activity,
			chat: fresh.chat,
			automations: fresh.automations.map((a) => {
				const prev = s.automations.find((x) => x.id === a.id);
				return prev ? {
					...a,
					enabled: prev.enabled,
					runCount: prev.runCount,
					lastRun: prev.lastRun && now - prev.lastRun < 2 * DAY ? prev.lastRun : a.lastRun
				} : a;
			}),
			connectors: s.connectors.map((c) => ({
				...c,
				lastUsed: c.lastUsed && now - c.lastUsed < 2 * DAY ? c.lastUsed : fresh.connectors.find((x) => x.id === c.id)?.lastUsed
			})),
			heartbeatAt: now,
			usage: ensurePeriod(s.usage, now)
		}));
	},
	resetDemo: () => {
		const fresh = createSeeds();
		set({
			connectors: fresh.connectors,
			skills: fresh.skills,
			automations: fresh.automations,
			activity: fresh.activity,
			chat: fresh.chat,
			threads: fresh.threads,
			activeThreadId: fresh.activeThreadId,
			agents: fresh.agents,
			profile: emptyProfile(),
			imagineJobs: [],
			imaginePrompt: "",
			imagineAspect: "1:1",
			imagineBusy: false,
			imagineError: null,
			mode: "auto",
			heartbeatAt: fresh.heartbeatAt,
			running: false,
			streamStatus: null,
			streamingMessageId: null,
			modelCatalog: emptyCatalog(),
			lastModelsFetchAt: 0,
			nav: "chat",
			modeMenuOpen: false,
			usage: createUsage("pro"),
			grokConnected: null,
			grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",
			oauth: null,
			oauthPending: null
		});
	}
}), {
	name: "grokhub-clean-v3",
	partialize: (s) => ({
		connectors: s.connectors,
		skills: s.skills,
		automations: s.automations,
		threads: s.threads,
		activeThreadId: s.activeThreadId,
		agents: s.agents,
		mode: s.mode,
		desktop: s.desktop,
		usage: s.usage,
		imagineJobs: s.imagineJobs.slice(0, 8).map(({ imageDataUrl: _drop, ...rest }) => rest),
		imagineAspect: s.imagineAspect,
		apiKey: s.apiKey,
		githubToken: s.githubToken,
		oauth: s.oauth,
		profile: s.profile,
		modelCatalog: s.modelCatalog,
		lastModelsFetchAt: s.lastModelsFetchAt,
		chat: s.chat,
		activity: s.activity.slice(0, 40)
	}),
	migrate: (persisted) => {
		const s = persisted || {};
		const cat = s.modelCatalog;
		if (cat && (!cat.classifiedBy || !cat.slots)) s.modelCatalog = emptyCatalog();
		else if (cat && !cat.classifiedBy) s.modelCatalog = {
			...emptyCatalog(),
			...cat,
			classifiedBy: cat.classifiedBy || "heuristic",
			classifiedAt: cat.classifiedAt || 0,
			signature: cat.signature || ""
		};
		return s;
	},
	skipHydration: true
}));
function wait(ms) {
	return new Promise((r) => setTimeout(r, ms));
}
/** Active chat stream abort (module-level so Stop works across re-renders) */
var activeChatAbort = null;
var chatGeneration = 0;
/** Avatar with automatic initials fallback when URL is missing or fails to load. */
function ProfileAvatar({ src, name, email, className, size = "md" }) {
	const [failed, setFailed] = (0, import_react.useState)(false);
	const label = (name || email || "G").trim();
	const initials = label.split(/\s+/).filter(Boolean).slice(0, 2).map((p) => p[0]?.toUpperCase() || "").join("") || "G";
	const dim = size === "sm" ? "h-8 w-8 text-xs" : size === "lg" ? "h-12 w-12 text-base" : "h-10 w-10 text-sm";
	if (!src || failed) return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("grid shrink-0 place-items-center rounded-full bg-[var(--color-elevated)] font-medium text-[var(--color-fg)] ring-1 ring-[var(--color-border)]", dim, className),
		title: label,
		"aria-label": label,
		children: initials
	});
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
		src,
		alt: "",
		title: label,
		className: cn("shrink-0 rounded-full object-cover ring-1 ring-[var(--color-border)]", dim, className),
		referrerPolicy: "no-referrer",
		onError: () => setFailed(true)
	});
}
/**
* Minimal signed-in identity chip + sign-out. Restyle freely (see the
* `design-ui` skill). Sign-out is only shown when auth is enabled (the
* disabled-auth dev user has nothing to sign out of).
*/
function UserButton() {
	const user = useCurrentUser();
	if (!user) return null;
	const label = user.displayName ?? user.primaryEmail ?? "Account";
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex items-center gap-2",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProfileAvatar, {
				src: user.profileImageUrl,
				name: user.displayName,
				email: user.primaryEmail,
				size: "sm"
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
				className: "text-sm font-medium",
				children: label
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
				type: "button",
				onClick: () => void signOut(),
				className: "cursor-pointer text-sm underline-offset-4 opacity-70 hover:underline",
				children: "Sign out"
			})
		]
	});
}
var ICONS = {
	auto: Sparkles,
	fast: Zap,
	expert: Brain,
	heavy: Users,
	build: Hammer
};
/**
* Mode picker for the title bar. Menu is portaled to document.body so it is not
* clipped by the frameless shell overflow / app-region drag.
*/
function ModePicker() {
	const mode = useGrokHub((s) => s.mode);
	const open = useGrokHub((s) => s.modeMenuOpen);
	const setMode = useGrokHub((s) => s.setMode);
	const setModeMenuOpen = useGrokHub((s) => s.setModeMenuOpen);
	const catalog = useGrokHub((s) => s.modelCatalog);
	const btnRef = (0, import_react.useRef)(null);
	const menuRef = (0, import_react.useRef)(null);
	const [pos, setPos] = (0, import_react.useState)({
		top: 0,
		right: 0
	});
	const modes = getModesWithCatalog(catalog);
	const active = modes.find((m) => m.id === mode) ?? modes[0];
	const ActiveIcon = ICONS[active.id];
	const noDrag = { WebkitAppRegion: "no-drag" };
	(0, import_react.useLayoutEffect)(() => {
		if (!open || !btnRef.current) return;
		const update = () => {
			const r = btnRef.current.getBoundingClientRect();
			setPos({
				top: r.bottom + 6,
				right: Math.max(8, window.innerWidth - r.right)
			});
		};
		update();
		window.addEventListener("resize", update);
		window.addEventListener("scroll", update, true);
		return () => {
			window.removeEventListener("resize", update);
			window.removeEventListener("scroll", update, true);
		};
	}, [open]);
	(0, import_react.useEffect)(() => {
		if (!open) return;
		const onDoc = (e) => {
			const t = e.target;
			if (btnRef.current?.contains(t) || menuRef.current?.contains(t)) return;
			setModeMenuOpen(false);
		};
		const onKey = (e) => {
			if (e.key === "Escape") setModeMenuOpen(false);
		};
		document.addEventListener("mousedown", onDoc);
		document.addEventListener("keydown", onKey);
		return () => {
			document.removeEventListener("mousedown", onDoc);
			document.removeEventListener("keydown", onKey);
		};
	}, [open, setModeMenuOpen]);
	const menu = open && typeof document !== "undefined" && (0, import_react_dom.createPortal)(/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		ref: menuRef,
		role: "listbox",
		style: {
			...noDrag,
			position: "fixed",
			top: pos.top,
			right: pos.right,
			zIndex: 9999
		},
		className: "w-[min(100vw-1.5rem,340px)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] p-1.5 shadow-[var(--shadow-soft)]",
		children: [modes.map((m) => {
			const Icon = ICONS[m.id];
			const selected = m.id === mode;
			return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
				type: "button",
				role: "option",
				"aria-selected": selected,
				style: noDrag,
				onClick: (e) => {
					e.preventDefault();
					e.stopPropagation();
					setMode(m.id);
					setModeMenuOpen(false);
				},
				className: cn("flex w-full items-start gap-3 rounded-[var(--radius-md)] px-2.5 py-2.5 text-left transition-colors", selected ? "bg-[var(--color-elevated)]" : "hover:bg-[var(--color-elevated)]/70"),
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "mt-0.5 h-4 w-4 shrink-0 text-[var(--color-muted)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "min-w-0 flex-1",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: m.label
								}),
								m.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]",
									children: "Beta"
								}),
								selected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "ml-auto text-[var(--color-muted)]",
									children: "✓"
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-xs text-[var(--color-muted)]",
							children: m.subtitle
						}),
						m.id !== "auto" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]",
							children: m.modelId
						})
					]
				})]
			}, m.id);
		}), catalog.essential.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "border-t border-[var(--color-border)] px-2.5 py-1.5 text-[10px] text-[var(--color-subtle)]",
			children: [
				catalog.source === "live" ? "Live" : "Fallback",
				" · ",
				catalog.essential.length,
				" ",
				"models · slots by ",
				catalog.classifiedBy === "grok" ? "Grok" : "heuristic"
			]
		})]
	}), document.body);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative",
		style: noDrag,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			ref: btnRef,
			type: "button",
			style: noDrag,
			onClick: (e) => {
				e.preventDefault();
				e.stopPropagation();
				setModeMenuOpen(!open);
			},
			className: cn("flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 text-left transition-colors hover:border-[var(--color-border-strong)]", open && "border-[var(--color-border-strong)]"),
			"aria-haspopup": "listbox",
			"aria-expanded": open,
			"aria-label": `Model mode: ${modeBadge(active.id, catalog)}`,
			children: [
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ActiveIcon, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "text-xs font-medium text-[var(--color-fg)]",
					children: active.label
				}),
				active.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "rounded bg-[var(--color-surface)] px-1 py-px text-[10px] text-[var(--color-subtle)]",
					children: "Beta"
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "hidden max-w-[7.5rem] truncate font-mono text-[10px] text-[var(--color-subtle)] sm:inline",
					children: active.model
				})
			]
		}), menu]
	});
}
/** Avoids SSR/client time drift hydration mismatches. */
function RelativeTime({ ts, className }) {
	const [label, setLabel] = (0, import_react.useState)("—");
	(0, import_react.useEffect)(() => {
		const update = () => setLabel(formatRelative(ts));
		update();
		const id = window.setInterval(update, 15e3);
		return () => window.clearInterval(id);
	}, [ts]);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
		className,
		children: label
	});
}
var badgeVariants = cva("inline-flex items-center rounded-full border px-2.5 py-0.5 text-xs font-medium", {
	variants: { variant: {
		default: "border-[var(--color-border)] bg-[var(--color-elevated)] text-[var(--color-muted)]",
		success: "border-[color-mix(in_oklab,var(--color-success)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_12%,transparent)] text-[var(--color-success)]",
		warn: "border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_12%,transparent)] text-[var(--color-warn)]",
		danger: "border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_12%,transparent)] text-[var(--color-danger)]",
		info: "border-[color-mix(in_oklab,var(--color-info)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-info)_12%,transparent)] text-[var(--color-info)]"
	} },
	defaultVariants: { variant: "default" }
});
function Badge({ className, variant, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn(badgeVariants({ variant }), className),
		...props
	});
}
function Card({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-panel)] shadow-[var(--shadow-soft)]", className),
		...props
	});
}
function CardHeader({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("flex flex-col gap-1 p-5 pb-3", className),
		...props
	});
}
function CardTitle({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("h3", {
		className: cn("text-base font-semibold tracking-tight text-[var(--color-fg)]", className),
		...props
	});
}
function CardDescription({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
		className: cn("text-sm text-[var(--color-muted)]", className),
		...props
	});
}
function CardContent({ className, ...props }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("p-5 pt-2", className),
		...props
	});
}
function barColor(tone) {
	if (tone === "danger") return "bg-[var(--color-danger)]";
	if (tone === "warn") return "bg-[var(--color-warn)]";
	return "bg-[var(--color-fg)]";
}
/** Compact titlebar chip — navigates to Settings usage panel */
function UsageMeterChip({ className }) {
	const usage = useGrokHub((s) => s.usage);
	const setNav = useGrokHub((s) => s.setNav);
	const plan = PLAN_LIMITS[usage.plan];
	const pct = usagePercent(usage);
	const tone = usageTone(pct);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		style: { WebkitAppRegion: "no-drag" },
		onClick: (e) => {
			e.preventDefault();
			e.stopPropagation();
			setNav("settings");
		},
		title: `${plan.label}: ${formatUnits(usage.usedUnits)} / ${formatUnits(plan.units)} units · open Settings`,
		className: cn("flex min-w-0 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1 text-left transition-colors hover:border-[var(--color-border-strong)]", className),
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Gauge, { className: cn("h-3.5 w-3.5 shrink-0", tone === "danger" ? "text-[var(--color-danger)]" : tone === "warn" ? "text-[var(--color-warn)]" : "text-[var(--color-muted)]") }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "min-w-0 flex-1",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate text-[10px] font-medium text-[var(--color-fg)]",
					children: plan.label
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "tabular text-[10px] text-[var(--color-subtle)]",
					children: [Math.round(pct), "%"]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mt-0.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("h-full rounded-full transition-all duration-300", barColor(tone)),
					style: { width: `${Math.min(100, Math.max(0, pct))}%` }
				})
			})]
		})]
	});
}
/** Full breakdown card for Command / Settings */
function UsageMeterPanel({ compact }) {
	const usage = useGrokHub((s) => s.usage);
	const setPlan = useGrokHub((s) => s.setPlan);
	const resetUsage = useGrokHub((s) => s.resetUsagePeriod);
	const plan = PLAN_LIMITS[usage.plan];
	const pct = usagePercent(usage);
	const tone = usageTone(pct);
	const left = daysLeftInPeriod(usage);
	const remaining = Math.max(0, plan.units - usage.usedUnits);
	const rows = [
		{
			label: "Agent messages",
			used: usage.messages,
			cap: plan.messages
		},
		{
			label: "Imagine",
			used: usage.imagine,
			cap: plan.imagine
		},
		{
			label: "Automations",
			used: usage.automations,
			cap: plan.automations
		},
		{
			label: "Host CLI",
			used: usage.host,
			cap: plan.host
		}
	];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, {
		className: compact ? "pb-2" : void 0,
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start justify-between gap-2",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Gauge, { className: "h-4 w-4 text-[var(--color-muted)]" }), "Usage · subscription limits"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Compute units reset each billing period. Heavy / Expert cost more per turn." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
				variant: tone === "danger" ? "danger" : tone === "warn" ? "warn" : "success",
				children: plan.label
			})]
		})
	}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
		className: "space-y-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "mb-1.5 flex items-end justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "text-2xl font-semibold tracking-tight tabular",
					children: [formatUnits(usage.usedUnits), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
						className: "text-sm font-normal text-[var(--color-muted)]",
						children: [
							" ",
							"/ ",
							formatUnits(plan.units)
						]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "text-xs text-[var(--color-subtle)]",
					children: [
						formatUnits(remaining),
						" units left · ",
						left,
						"d until reset"
					]
				})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("text-lg font-semibold tabular", tone === "danger" ? "text-[var(--color-danger)]" : tone === "warn" ? "text-[var(--color-warn)]" : "text-[var(--color-fg)]"),
					children: [Math.round(pct), "%"]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "h-2.5 overflow-hidden rounded-full bg-[var(--color-border)]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: cn("h-full rounded-full transition-all duration-500", barColor(tone)),
					style: { width: `${pct}%` }
				})
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-2 sm:grid-cols-2",
				children: rows.map((r) => {
					const p = r.cap > 0 ? Math.min(100, r.used / r.cap * 100) : 0;
					const t = usageTone(p);
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex justify-between text-xs",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-[var(--color-muted)]",
								children: r.label
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "tabular text-[var(--color-fg)]",
								children: [r.used, /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "text-[var(--color-subtle)]",
									children: [" / ", r.cap]
								})]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mt-1.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: cn("h-full rounded-full", barColor(t)),
								style: { width: `${p}%` }
							})
						})]
					}, r.label);
				})
			}),
			!compact && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "mb-1.5 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]",
				children: "By mode (messages)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex flex-wrap gap-1.5",
				children: [
					["fast", "Fast"],
					["auto", "Auto"],
					["build", "Build"],
					["expert", "Expert"],
					["heavy", "Heavy"]
				].map(([id, label]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
					className: "rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] text-[var(--color-muted)]",
					children: [
						label,
						" ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "tabular text-[var(--color-fg)]",
							children: usage.byMode[id]
						})
					]
				}, id))
			})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap gap-2",
				children: [[
					["free", "Free"],
					["super", "SuperGrok"],
					["pro", "SuperGrok Pro"]
				].map(([id, label]) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: () => setPlan(id),
					className: cn("rounded-full border px-3 py-1 text-xs transition-colors", usage.plan === id ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)]"),
					children: label
				}, id)), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
					type: "button",
					onClick: () => resetUsage(),
					className: "ml-auto rounded-full border border-[var(--color-border)] px-3 py-1 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]",
					children: "Simulate period reset"
				})]
			})] })
		]
	})] });
}
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
var Input = import_react.forwardRef(({ className, type, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
	type,
	className: cn("flex h-10 w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-fg)] placeholder:text-[var(--color-subtle)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-ring)] disabled:cursor-not-allowed disabled:opacity-50", className),
	ref,
	...props
}));
Input.displayName = "Input";
var Textarea = import_react.forwardRef(({ className, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("textarea", {
	className: cn("flex min-h-[96px] w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2 text-sm text-[var(--color-fg)] placeholder:text-[var(--color-subtle)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-ring)] disabled:cursor-not-allowed disabled:opacity-50", className),
	ref,
	...props
}));
Textarea.displayName = "Textarea";
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
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Describe once. Attach live connectors automatically from your connected set." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
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
/**
* Desktop host / gateway connector.
* When the unsandboxed bridge is offline, this is the CTA to reconnect so Grok
* can run shell, files, and apps on the user's machine.
*/
function HostGatewayBanner({ variant = "card", className, onOpenDesktop }) {
	const setNav = useGrokHub((s) => s.setNav);
	const connectConnector = useGrokHub((s) => s.connectConnector);
	const [info, setInfo] = (0, import_react.useState)(null);
	const [error, setError] = (0, import_react.useState)(null);
	const [busy, setBusy] = (0, import_react.useState)(false);
	const [isElectron, setIsElectron] = (0, import_react.useState)(false);
	const probe = (0, import_react.useCallback)(async () => {
		setBusy(true);
		setError(null);
		try {
			const mod = await import("./host-client-WUUmAwRI.mjs");
			setIsElectron(mod.isDesktopShell());
			const i = await mod.hostInfo();
			setInfo(i);
			if (i.bridge !== "none" && i.unsandboxed) useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
				...c,
				status: "connected",
				lastUsed: Date.now()
			} : c) }));
			return i;
		} catch (e) {
			const msg = e instanceof Error ? e.message : "Host probe failed";
			setError(msg);
			setInfo(null);
			return null;
		} finally {
			setBusy(false);
		}
	}, [connectConnector]);
	(0, import_react.useEffect)(() => {
		probe();
	}, [probe]);
	const online = Boolean(info && info.bridge !== "none" && info.unsandboxed);
	async function connectGateway() {
		setBusy(true);
		setError(null);
		try {
			const i = await probe();
			if (i && i.bridge !== "none" && i.unsandboxed) {
				useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
					...c,
					status: "connected",
					lastUsed: Date.now()
				} : c) }));
				return;
			}
			if (typeof window !== "undefined" && window.grokhubDesktop) setError("Electron shell is present but host IPC failed. Fully quit GrokHub (tray too) and relaunch.");
			else setError("No desktop gateway in this window. Launch the Arch package: grokhub (Electron). Web-only preview cannot control your machine.");
		} finally {
			setBusy(false);
		}
	}
	if (variant === "compact") {
		if (online) return null;
		return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: cn("flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2", className),
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2 text-xs text-[var(--color-warn)]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-3.5 w-3.5 shrink-0" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "truncate",
					children: "Desktop gateway offline — agent cannot run shell/files on your machine."
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex shrink-0 gap-1.5",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
					size: "sm",
					variant: "secondary",
					disabled: busy,
					onClick: () => void connectGateway(),
					children: [busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "h-3.5 w-3.5" }), "Connect host"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					variant: "ghost",
					onClick: () => {
						setNav("desktop");
						onOpenDesktop?.();
					},
					children: "Desktop"
				})]
			})]
		});
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
		className,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex flex-wrap items-start justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HardDrive, { className: "h-4 w-4" }), "Desktop host gateway"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Unsandboxed access so Grok can run CLI commands, read/write files, and open apps on this machine on your behalf." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
				variant: online ? "success" : "warn",
				children: online ? "connected" : "offline"
			})]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
			className: "space-y-3",
			children: [
				online && info ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start gap-2 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-2.5 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldCheck, { className: "mt-0.5 h-4 w-4 shrink-0 text-[var(--color-success)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "font-medium text-[var(--color-fg)]",
								children: [
									info.user,
									"@",
									info.hostname
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "font-mono text-xs text-[var(--color-muted)]",
								children: [
									info.bridge,
									" · ",
									info.platform,
									"/",
									info.arch,
									" · ",
									info.homedir
								]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-1 text-xs text-[var(--color-subtle)]",
								children: [
									"Agent can use ",
									/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "font-mono",
										children: "$"
									}),
									" shell in chat and the Desktop tab."
								]
							})
						]
					})]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "space-y-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 text-sm text-[var(--color-muted)]",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", { children: "No live desktop gateway. Without it, Grok cannot control your PC — only chat." }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("ol", {
						className: "list-decimal space-y-1 pl-4 text-xs text-[var(--color-subtle)]",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("li", { children: [
								"Install/run the Arch package:",
								" ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "font-mono text-[var(--color-fg)]",
									children: "grokhub"
								})
							] }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: "Use the Electron window (not a plain browser tab)" }),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: "Click Connect below to probe host IPC / API" })
						]
					})]
				}),
				error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-danger)]",
					children: error
				}),
				/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [
						!online ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							disabled: busy,
							onClick: () => void connectGateway(),
							children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }), "Connecting…"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plug, { className: "h-4 w-4" }), "Connect desktop host"] })
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							onClick: () => {
								setNav("desktop");
								onOpenDesktop?.();
							},
							children: "Open Desktop host"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							variant: "secondary",
							size: "default",
							disabled: busy,
							onClick: () => void probe(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: cn("h-4 w-4", busy && "animate-spin") }), "Reprobe"]
						}),
						isElectron && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "info",
							className: "self-center",
							children: "Electron shell"
						})
					]
				})
			]
		})]
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
				const { hostInfo } = await import("./host-client-WUUmAwRI.mjs");
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
			const { hostExec } = await import("./host-client-WUUmAwRI.mjs");
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
									className: cn("chat-bubble rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed whitespace-pre-wrap", m.role === "user" ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : m.role === "system" ? "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]" : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)]", m.streaming && "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]"),
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
										m.content || (m.streaming ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
											className: "inline-flex items-center gap-1.5 text-[var(--color-subtle)]",
											children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-info)]" }), "…"]
										}) : ""),
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
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
									ref: inputRef,
									value: text,
									onChange: (e) => setText(e.target.value),
									placeholder: busy ? "Agent running — press Stop to interrupt…" : "Message Grok… or $ shell",
									className: "flex-1",
									onKeyDown: (e) => {
										if (e.key === "Escape" && busy) {
											e.preventDefault();
											onStop();
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
				"Grok, Desktop Host, and GitHub use real auth. Other connectors enable agent context and open the vendor sign-in page.",
				` ${connected} connected.`,
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
							variant: on ? "success" : err ? "danger" : "default",
							children: on ? "connected" : err ? "error" : "offline"
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
								children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-3.5 w-3.5 animate-spin" }), "Working…"] }) : on ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link2Off, { className: "h-3.5 w-3.5" }), "Disconnect"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "h-3.5 w-3.5" }), "Connect"] })
							})
						]
					})]
				}, c.id);
			})
		})]
	});
}
var QUICK_CMDS = [
	"uname -a && whoami && pwd",
	"ls -la",
	"df -h | head -12",
	"ps aux --sort=-%mem | head -12",
	"env | sort | head -40"
];
function DesktopHostView() {
	const recordUsage = useGrokHub((s) => s.recordUsage);
	const [api, setApi] = (0, import_react.useState)(null);
	const [tab, setTab] = (0, import_react.useState)("cli");
	const [info, setInfo] = (0, import_react.useState)(null);
	const [error, setError] = (0, import_react.useState)(null);
	const [loading, setLoading] = (0, import_react.useState)(true);
	const [cmd, setCmd] = (0, import_react.useState)("uname -a && whoami && pwd");
	const [cwd, setCwd] = (0, import_react.useState)("");
	const [busy, setBusy] = (0, import_react.useState)(false);
	const [result, setResult] = (0, import_react.useState)(null);
	const [history, setHistory] = (0, import_react.useState)([]);
	const [dirPath, setDirPath] = (0, import_react.useState)("");
	const [entries, setEntries] = (0, import_react.useState)([]);
	const [filePreview, setFilePreview] = (0, import_react.useState)(null);
	const [apps, setApps] = (0, import_react.useState)([]);
	const [appQ, setAppQ] = (0, import_react.useState)("");
	const [isShell, setIsShell] = (0, import_react.useState)(false);
	const probed = (0, import_react.useRef)(false);
	(0, import_react.useEffect)(() => {
		let cancelled = false;
		(async () => {
			try {
				const mod = await import("./host-client-WUUmAwRI.mjs");
				if (cancelled) return;
				setApi(mod);
				setIsShell(mod.isDesktopShell());
				setLoading(true);
				const i = await mod.hostInfo();
				if (cancelled) return;
				setInfo(i);
				setCwd(i.homedir || i.cwd);
				setDirPath(i.homedir || i.cwd);
				if (i.bridge === "none" || !i.unsandboxed) setError("Desktop host bridge is offline. Fully quit and relaunch GrokHub from the Arch package (Electron shell). Browser-only preview has limited host access.");
				if (!probed.current && i.bridge !== "none") {
					probed.current = true;
					try {
						const r = await mod.hostExec("uname -a && whoami && pwd && echo --- && ls -la | head -20", i.homedir || i.cwd);
						if (!cancelled) {
							setResult(r);
							setHistory(["uname -a && whoami && pwd && echo --- && ls -la | head -20"]);
							if (!r.ok) setError(r.stderr || `probe exit ${r.code}`);
						}
					} catch (e) {
						if (!cancelled) setError(e instanceof Error ? e.message : "host probe failed");
					}
				}
			} catch (e) {
				if (!cancelled) setError(e instanceof Error ? e.message : "host bridge failed");
			} finally {
				if (!cancelled) setLoading(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [recordUsage]);
	const refreshInfo = (0, import_react.useCallback)(async () => {
		if (!api) return;
		setLoading(true);
		setError(null);
		try {
			const i = await api.hostInfo();
			setInfo(i);
			setCwd((c) => c || i.cwd);
			setDirPath((p) => p || i.homedir || i.cwd);
		} catch (e) {
			setError(e instanceof Error ? e.message : "host bridge failed");
		} finally {
			setLoading(false);
		}
	}, [api]);
	async function runCmd(command) {
		if (!api) return;
		const c = (command ?? cmd).trim();
		if (!c) return;
		if (!recordUsage("host").ok) {
			setError("Subscription unit quota exceeded — reset period or switch plan in Settings.");
			return;
		}
		setBusy(true);
		setError(null);
		try {
			const r = await api.hostExec(c, cwd || void 0);
			setResult(r);
			setHistory((h) => [c, ...h.filter((x) => x !== c)].slice(0, 12));
		} catch (e) {
			setError(e instanceof Error ? e.message : "exec failed");
		} finally {
			setBusy(false);
		}
	}
	async function loadDir(p) {
		if (!api) return;
		setBusy(true);
		setError(null);
		setFilePreview(null);
		try {
			const res = await api.hostListDir(p || dirPath);
			setDirPath(res.path);
			setEntries(res.entries);
		} catch (e) {
			setError(e instanceof Error ? e.message : "list dir failed");
		} finally {
			setBusy(false);
		}
	}
	async function openEntry(e) {
		if (!api) return;
		if (e.isDir) {
			await loadDir(e.path);
			return;
		}
		setBusy(true);
		try {
			const f = await api.hostReadFile(e.path);
			setFilePreview({
				path: f.path,
				content: f.content
			});
		} catch (err) {
			setError(err instanceof Error ? err.message : "read failed");
		} finally {
			setBusy(false);
		}
	}
	async function loadApps() {
		if (!api) return;
		setBusy(true);
		setError(null);
		try {
			setApps(await api.hostListApps());
		} catch (e) {
			setError(e instanceof Error ? e.message : "list apps failed");
		} finally {
			setBusy(false);
		}
	}
	(0, import_react.useEffect)(() => {
		if (!api) return;
		if (tab === "files" && entries.length === 0 && dirPath) loadDir(dirPath);
		if (tab === "apps" && apps.length === 0) loadApps();
	}, [tab, api]);
	const filteredApps = apps.filter((a) => a.name.toLowerCase().includes(appQ.trim().toLowerCase()));
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-4",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HostGatewayBanner, { variant: "card" }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-4 w-4 text-[var(--color-warn)]" }), "Desktop host · session"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Full host CLI / files / apps. Shell commands bill 0.25 units against your plan." })]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "flex flex-wrap items-center gap-2",
				children: [
					loading && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Probing host…"
					}),
					info && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: info.unsandboxed ? "warn" : "default",
							children: info.unsandboxed ? "unsandboxed" : "limited"
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "info",
							children: info.bridge
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
							className: "font-mono",
							children: [
								info.platform,
								"/",
								info.arch
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: [
								info.user,
								"@",
								info.hostname,
								" · ",
								info.shell
							]
						}),
						isShell && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: "success",
							children: "Electron shell"
						})
					] }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
						size: "sm",
						variant: "secondary",
						className: "ml-auto",
						onClick: () => void refreshInfo(),
						disabled: !api,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RefreshCw, { className: "h-3.5 w-3.5" }), "Refresh"]
					})
				]
			})] }),
			error && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-danger)_10%,transparent)] px-3 py-2 text-sm text-[var(--color-danger)]",
				children: error
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "flex flex-wrap gap-1.5",
				children: [
					[
						"cli",
						"CLI",
						Terminal
					],
					[
						"files",
						"Files",
						FolderOpen
					],
					[
						"apps",
						"Apps",
						AppWindow
					]
				].map(([id, label, Icon]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					onClick: () => setTab(id),
					className: cn("inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border px-3 text-sm", tab === id ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : "border-[var(--color-border)] text-[var(--color-muted)]"),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-3.5 w-3.5" }), label]
				}, id))
			}),
			tab === "cli" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Shell"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Full command access · 0.25u per run" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "grid gap-2 sm:grid-cols-[1fr_160px]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: cwd,
							onChange: (e) => setCwd(e.target.value),
							placeholder: "Working directory",
							className: "font-mono text-xs"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							disabled: !api || busy,
							onClick: () => void runCmd("pwd && ls -la"),
							children: "pwd + ls"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						className: "flex gap-2",
						onSubmit: (e) => {
							e.preventDefault();
							runCmd();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: cmd,
							onChange: (e) => setCmd(e.target.value),
							placeholder: "e.g. systemctl --user status · pacman -Q electron",
							className: "font-mono text-xs",
							disabled: busy || !api
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							type: "submit",
							disabled: busy || !api || !cmd.trim(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "h-3.5 w-3.5" }), "Run"]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1.5",
						children: QUICK_CMDS.map((h) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							className: "rounded-full border border-[var(--color-border)] px-2 py-0.5 font-mono text-[10px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]",
							onClick: () => {
								setCmd(h);
								runCmd(h);
							},
							children: [h.slice(0, 42), h.length > 42 ? "…" : ""]
						}, h))
					}),
					result && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center justify-between border-b border-[var(--color-border)] px-3 py-2 text-[10px] text-[var(--color-subtle)]",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
								className: "font-mono",
								children: ["$ ", result.command]
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
								"exit ",
								result.code ?? "?",
								" · ",
								result.ms,
								"ms"
							] })]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("pre", {
							className: "max-h-[360px] overflow-auto p-3 font-mono text-xs leading-relaxed text-[var(--color-fg)] whitespace-pre-wrap",
							children: [
								result.stdout || "",
								result.stderr ? `\n[stderr]\n${result.stderr}` : "",
								!result.stdout && !result.stderr ? "(no output)" : ""
							]
						})]
					})
				]
			})] }),
			tab === "files" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid gap-3 lg:grid-cols-[1fr_1fr]",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
					className: "gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Filesystem"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: dirPath,
							onChange: (e) => setDirPath(e.target.value),
							className: "font-mono text-xs",
							onKeyDown: (e) => {
								if (e.key === "Enter") loadDir(dirPath);
							}
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							size: "sm",
							variant: "secondary",
							disabled: !api,
							onClick: () => void loadDir(dirPath),
							children: "Open"
						})]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("ul", {
					className: "max-h-[420px] space-y-0.5 overflow-auto",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						className: "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
						onClick: () => {
							loadDir(dirPath.replace(/\/[^/]+\/?$/, "") || "/");
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Folder, { className: "h-3.5 w-3.5" }), ".."]
					}) }), entries.map((e) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("li", { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => void openEntry(e),
						className: "flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm hover:bg-[var(--color-elevated)]",
						children: [e.isDir ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(FolderOpen, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChevronRight, { className: "h-3.5 w-3.5 text-[var(--color-subtle)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "min-w-0 flex-1 truncate",
							children: e.name
						})]
					}) }, e.path))]
				}) })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Preview"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
					className: "font-mono text-[10px]",
					children: filePreview?.path || "Select a file"
				})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
					className: "max-h-[420px] overflow-auto rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[#0c0c0e] p-3 font-mono text-xs text-[var(--color-muted)] whitespace-pre-wrap",
					children: filePreview?.content || "—"
				}) })] })]
			}),
			tab === "apps" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "gap-2 sm:flex-row sm:items-end sm:justify-between",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Installed apps (.desktop)"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Launch with gtk-launch / xdg-open as your user" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						value: appQ,
						onChange: (e) => setAppQ(e.target.value),
						placeholder: "Filter apps",
						className: "sm:w-48"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						variant: "secondary",
						disabled: !api,
						onClick: () => void loadApps(),
						children: "Reload"
					})]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "grid max-h-[480px] gap-2 overflow-auto sm:grid-cols-2 xl:grid-cols-3",
				children: [filteredApps.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate text-sm font-medium",
							children: a.name
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "truncate font-mono text-[10px] text-[var(--color-subtle)]",
							children: a.exec
						})]
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						size: "sm",
						disabled: !api,
						onClick: () => void api?.hostOpenApp({
							desktopFile: a.desktopFile,
							exec: a.exec
						}),
						children: "Open"
					})]
				}, a.id)), filteredApps.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
					className: "text-sm text-[var(--color-muted)]",
					children: "No .desktop apps found yet."
				})]
			}) })] })
		]
	});
}
function HistoryView() {
	const threads = useGrokHub((s) => s.threads);
	const activeThreadId = useGrokHub((s) => s.activeThreadId);
	const selectThread = useGrokHub((s) => s.selectThread);
	const deleteThread = useGrokHub((s) => s.deleteThread);
	const newThread = useGrokHub((s) => s.newThread);
	const sorted = [...threads].sort((a, b) => b.updatedAt - a.updatedAt);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "content-readable mx-auto space-y-4",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
			className: "flex flex-row items-start justify-between gap-3",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(History, { className: "h-4 w-4" }), "History"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Past chats — same idea as Grok web. Select one to resume or start a new conversation." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
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
				return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: cn("group flex items-start gap-2 rounded-[var(--radius-md)] border px-3 py-2.5 transition-colors", active ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]" : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
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
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						className: "shrink-0 rounded p-1.5 text-[var(--color-subtle)] opacity-0 transition-opacity hover:bg-[var(--color-surface)] hover:text-[var(--color-danger)] group-hover:opacity-100",
						"aria-label": "Delete chat",
						onClick: () => deleteThread(t.id),
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Trash2, { className: "h-3.5 w-3.5" })
					})]
				}, t.id);
			})]
		})] })
	});
}
var ASPECTS = [
	"1:1",
	"16:9",
	"9:16",
	"3:2",
	"2:3"
];
function ImagineView() {
	const prompt = useGrokHub((s) => s.imaginePrompt);
	const aspect = useGrokHub((s) => s.imagineAspect);
	const jobs = useGrokHub((s) => s.imagineJobs);
	const busy = useGrokHub((s) => s.imagineBusy);
	const err = useGrokHub((s) => s.imagineError);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const mode = useGrokHub((s) => s.mode);
	const setImaginePrompt = useGrokHub((s) => s.setImaginePrompt);
	const setImagineAspect = useGrokHub((s) => s.setImagineAspect);
	const runImagine = useGrokHub((s) => s.runImagine);
	const latest = jobs[0];
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2 text-sm",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Image, { className: "h-4 w-4" }), "Imagine"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Live Grok image generation when OAuth/API is connected; local SVG preview as fallback." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [
							ASPECTS.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => setImagineAspect(a),
								className: a === aspect ? "rounded-full border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-1.5 text-xs font-medium" : "rounded-full border border-[var(--color-border)] px-3 py-1.5 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
								children: a
							}, a)),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								className: "ml-auto font-mono",
								children: mode
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: grokConnected ? "success" : "default",
								children: grokConnected ? "live ready" : "local only"
							})
						]
					}),
					err && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-warn)]",
						children: [
							"Live Imagine: ",
							err,
							" — showing local preview."
						]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						className: "flex flex-col gap-2 sm:flex-row",
						onSubmit: (e) => {
							e.preventDefault();
							runImagine();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: prompt,
							onChange: (e) => setImaginePrompt(e.target.value),
							placeholder: "Moody night desk, dual monitors, soft amber lamp, film still…",
							disabled: busy
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							disabled: busy || !prompt.trim(),
							className: "sm:w-36",
							children: busy ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }), "Render"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-4 w-4" }), "Generate"] })
						})]
					})
				]
			})] }),
			latest?.imageDataUrl && latest.status === "ready" && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "overflow-hidden",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
					className: "flex-row items-center justify-between gap-3 space-y-0",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Latest"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
						className: "line-clamp-1",
						children: latest.prompt
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("a", {
						href: latest.imageDataUrl,
						download: `grokhub-imagine-${latest.id}.${latest.imageDataUrl.startsWith("data:image/svg") ? "svg" : "png"}`,
						className: "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, { className: "h-3.5 w-3.5" }), "Save"]
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)]",
					children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
						src: latest.imageDataUrl,
						alt: latest.prompt,
						className: "mx-auto max-h-[min(70vh,640px)] w-full object-contain"
					})
				}) })]
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-3 sm:grid-cols-2 xl:grid-cols-3",
				children: jobs.map((job) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "overflow-hidden",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "aspect-video bg-[var(--color-surface)]",
						children: job.imageDataUrl ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("img", {
							src: job.imageDataUrl,
							alt: job.prompt,
							className: "h-full w-full object-cover"
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "flex h-full items-center justify-center text-xs text-[var(--color-subtle)]",
							children: job.status === "rendering" ? "Rendering…" : "Queued"
						})
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "space-y-1 p-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex items-center justify-between gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: job.status === "ready" ? "success" : "info",
									children: job.status
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, {
									ts: job.ts,
									className: "text-[10px] text-[var(--color-subtle)]"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "line-clamp-2 text-xs text-[var(--color-muted)]",
								children: job.prompt
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "font-mono text-[10px] text-[var(--color-subtle)]",
								children: [
									job.aspect,
									" · ",
									job.mode
								]
							})
						]
					})]
				}, job.id))
			}),
			jobs.length === 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "py-12 text-center text-sm text-[var(--color-muted)]",
				children: "No renders yet. Describe a scene and hit Generate."
			}) })
		]
	});
}
function SettingsView() {
	const mode = useGrokHub((s) => s.mode);
	const setMode = useGrokHub((s) => s.setMode);
	const modelCatalog = useGrokHub((s) => s.modelCatalog);
	const refreshModels = useGrokHub((s) => s.refreshModels);
	const lastModelsFetchAt = useGrokHub((s) => s.lastModelsFetchAt);
	const desktop = useGrokHub((s) => s.desktop);
	const setDesktop = useGrokHub((s) => s.setDesktop);
	const setNav = useGrokHub((s) => s.setNav);
	const resetDemo = useGrokHub((s) => s.resetDemo);
	const apiKey = useGrokHub((s) => s.apiKey);
	const setApiKey = useGrokHub((s) => s.setApiKey);
	const githubToken = useGrokHub((s) => s.githubToken);
	const setGithubToken = useGrokHub((s) => s.setGithubToken);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
	const probeGrok = useGrokHub((s) => s.probeGrok);
	const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
	useGrokHub((s) => s.profile);
	const oauth = useGrokHub((s) => s.oauth);
	const oauthPending = useGrokHub((s) => s.oauthPending);
	const startGrokOAuth = useGrokHub((s) => s.startGrokOAuth);
	const pollGrokOAuth = useGrokHub((s) => s.pollGrokOAuth);
	const clearGrokOAuth = useGrokHub((s) => s.clearGrokOAuth);
	const { user, isPending } = useCurrentUserState();
	const [keyDraft, setKeyDraft] = (0, import_react.useState)(apiKey);
	const [ghDraft, setGhDraft] = (0, import_react.useState)(githubToken);
	const [probing, setProbing] = (0, import_react.useState)(false);
	const [oauthBusy, setOauthBusy] = (0, import_react.useState)(false);
	const [oauthErr, setOauthErr] = (0, import_react.useState)("");
	const [update, setUpdate] = (0, import_react.useState)(null);
	const [updateBusy, setUpdateBusy] = (0, import_react.useState)(false);
	const [updateLog, setUpdateLog] = (0, import_react.useState)("");
	const pollRef = (0, import_react.useRef)(null);
	(0, import_react.useEffect)(() => {
		setKeyDraft(apiKey);
	}, [apiKey]);
	(0, import_react.useEffect)(() => {
		setGhDraft(githubToken);
	}, [githubToken]);
	(0, import_react.useEffect)(() => {
		if (!oauth?.accessToken) return;
		let cancelled = false;
		(async () => {
			setProbing(true);
			try {
				await probeGrok();
			} finally {
				if (!cancelled) setProbing(false);
			}
		})();
		return () => {
			cancelled = true;
		};
	}, [oauth?.accessToken, oauth?.email]);
	(0, import_react.useEffect)(() => {
		if (!oauthPending) {
			if (pollRef.current) {
				window.clearInterval(pollRef.current);
				pollRef.current = null;
			}
			return;
		}
		const tick = async () => {
			try {
				const r = await pollGrokOAuth();
				if (r === "ready" || r === "failed") setOauthBusy(false);
			} catch (e) {
				setOauthErr(e instanceof Error ? e.message : "poll failed");
				setOauthBusy(false);
			}
		};
		tick();
		pollRef.current = window.setInterval(() => void tick(), 5e3);
		return () => {
			if (pollRef.current) window.clearInterval(pollRef.current);
		};
	}, [oauthPending, pollGrokOAuth]);
	(0, import_react.useEffect)(() => {
		checkUpdate(githubToken || void 0).then(setUpdate).catch((e) => setUpdate({
			currentVersion: "0.2.0",
			currentSha: null,
			remoteSha: null,
			remoteMessage: null,
			updateAvailable: false,
			repo: "blackviperxiii-ui/Grok-Hub",
			branch: "main",
			installRoot: null,
			detail: e instanceof Error ? e.message : "check failed"
		}));
	}, [githubToken]);
	async function onStartOAuth() {
		setOauthErr("");
		setOauthBusy(true);
		try {
			await startGrokOAuth();
			const pending = useGrokHub.getState().oauthPending;
			if (pending?.verificationUriComplete) window.open(pending.verificationUriComplete, "_blank", "noopener,noreferrer");
			else if (pending?.verificationUri) window.open(pending.verificationUri, "_blank", "noopener,noreferrer");
		} catch (e) {
			setOauthErr(e instanceof Error ? e.message : "Could not start OAuth");
			setOauthBusy(false);
		}
	}
	async function saveAndProbe() {
		setApiKey(keyDraft.trim());
		setProbing(true);
		try {
			if (await probeGrok() && user && !user.isDevFallback) await syncFromGrok({
				displayName: user.displayName,
				email: user.primaryEmail,
				imageUrl: user.profileImageUrl
			});
		} finally {
			setProbing(false);
		}
	}
	async function onCheckUpdate() {
		setUpdateBusy(true);
		setUpdateLog("");
		try {
			setGithubToken(ghDraft.trim());
			const s = await checkUpdate(ghDraft.trim() || void 0);
			setUpdate(s);
			setUpdateLog(s.detail);
		} catch (e) {
			setUpdateLog(e instanceof Error ? e.message : "check failed");
		} finally {
			setUpdateBusy(false);
		}
	}
	async function onInstallUpdate() {
		setUpdateBusy(true);
		setUpdateLog("Installing update from GitHub…");
		try {
			setGithubToken(ghDraft.trim());
			const r = await applyUpdate(ghDraft.trim() || void 0, true);
			if (r.status) setUpdate(r.status);
			else {
				const s = await checkUpdate(ghDraft.trim() || void 0);
				setUpdate(s);
			}
			const lines = [
				r.ok ? "OK" : "FAILED",
				r.detail,
				r.newVersion ? `Version: v${r.newVersion}` : "",
				r.newSha ? `Commit: ${r.newSha}` : "",
				"",
				...r.steps || []
			].filter(Boolean);
			if (r.ok && r.restarting) {
				lines.push("", "Restarting GrokHub…");
				setUpdateLog(lines.join("\n"));
				setTimeout(() => {
					window.location.reload();
				}, 1500);
				return;
			}
			setUpdateLog(lines.join("\n"));
		} catch (e) {
			setUpdateLog(e instanceof Error ? e.message : "update failed");
		} finally {
			setUpdateBusy(false);
		}
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "content-readable mx-auto space-y-5 pb-8",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Connect to Grok (xAI OAuth)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Sign in with your ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: "SuperGrok" }),
				" or ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("strong", { children: "X Premium+" }),
				" account via xAI device code — same flow as OpenClaw / Grok CLI. No API key required for subscription access."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: oauth && grokConnected ? "success" : oauth ? "info" : grokConnected ? "success" : "default",
								children: oauth && grokConnected ? "OAuth live" : oauth ? "OAuth session" : grokConnected ? "API connected" : "Not connected"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "text-xs text-[var(--color-muted)]",
								children: probing ? "Verifying with xAI…" : oauth && grokStatusDetail.toLowerCase().includes("not connected") ? "Session saved — verifying API access…" : grokStatusDetail
							}),
							oauth && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "secondary",
								size: "sm",
								disabled: probing,
								onClick: () => void saveAndProbe(),
								children: probing ? "Testing…" : "Test connection"
							})
						]
					}),
					oauth && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-3 rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-success)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-success)_8%,transparent)] px-3 py-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProfileAvatar, {
								src: oauth.picture,
								name: oauth.name,
								email: oauth.email
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0 flex-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium",
									children: oauth.name || "Grok account"
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "truncate text-xs text-[var(--color-muted)]",
									children: oauth.email || "OAuth session active"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "secondary",
								size: "sm",
								onClick: () => clearGrokOAuth(),
								children: "Disconnect"
							})
						]
					}),
					oauthPending && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-3 rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] p-4",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs uppercase tracking-wide text-[var(--color-subtle)]",
								children: "Approve this code"
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "font-mono text-3xl font-semibold tracking-[0.2em] text-[var(--color-fg)]",
								children: oauthPending.userCode
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm text-[var(--color-muted)]",
								children: "Open the link, sign in to xAI / Grok, and enter the code. This window polls automatically."
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "flex flex-wrap gap-2",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
									onClick: () => window.open(oauthPending.verificationUriComplete || oauthPending.verificationUri, "_blank", "noopener,noreferrer"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ExternalLink, { className: "h-4 w-4" }), "Open accounts.x.ai"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
									variant: "secondary",
									onClick: () => void pollGrokOAuth(),
									children: "Check now"
								})]
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("p", {
								className: "text-xs text-[var(--color-subtle)]",
								children: ["Waiting for approval… ", oauthBusy ? "polling" : ""]
							})
						]
					}),
					!oauth && !oauthPending && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						onClick: () => void onStartOAuth(),
						disabled: oauthBusy,
						children: oauthBusy ? "Starting…" : "Connect with Grok OAuth"
					}),
					oauthErr && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-sm text-[var(--color-danger)]",
						children: oauthErr
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Uses xAI public OAuth client (device code). Tokens stay on this device only and are never committed to git."
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "App account (optional)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Grok Build app identity via Google/X (auth.grok.me). Separate from live Grok model access above." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-3",
				children: isPending ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "h-10 animate-pulse rounded bg-[var(--color-elevated)]" }) : user && !user.isDevFallback ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap items-center gap-3",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ProfileAvatar, {
							src: user.profileImageUrl,
							name: user.displayName,
							email: user.primaryEmail
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "min-w-0 flex-1",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium",
								children: user.displayName || "Signed in"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "truncate text-xs text-[var(--color-muted)]",
								children: user.primaryEmail
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							size: "sm",
							onClick: () => void signOut(),
							children: "Sign out"
						})
					]
				}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [GROK_PROVIDERS.map((p) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "secondary",
						size: "sm",
						onClick: () => void signIn(p.providerId, { callbackURL: "/" }),
						children: p.label
					}, p.providerId)), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link, {
						to: "/login",
						className: "self-center text-xs text-[var(--color-muted)] underline-offset-2 hover:underline",
						children: "Full sign-in page"
					})]
				})
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterPanel, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "xAI API key (optional fallback)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Pay-per-token console key if you are not using SuperGrok OAuth. From",
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono",
					children: "console.x.ai"
				}),
				"."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
					type: "password",
					autoComplete: "off",
					value: keyDraft,
					onChange: (e) => setKeyDraft(e.target.value),
					placeholder: "xai-…"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						onClick: () => void saveAndProbe(),
						disabled: probing,
						children: probing ? "Testing…" : "Save & test key"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						variant: "secondary",
						onClick: () => {
							setKeyDraft("");
							setApiKey("");
						},
						children: "Clear key"
					})]
				})]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex flex-wrap items-start justify-between gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
					className: "text-sm",
					children: "Essential models"
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Polled from xAI · only 4.5 / 4.3 / Fast / Build / Imagine class ids" })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					size: "sm",
					variant: "secondary",
					onClick: () => void refreshModels({ force: true }),
					children: "Refresh + reclassify"
				})]
			}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "grid gap-2 sm:grid-cols-2",
						children: [
							["fast", "Fast chat"],
							["balanced", "Balanced"],
							["smart", "Brains"],
							["heavy", "Heavy / team"],
							["build", "Build / code"],
							["imagine", "Imagine"]
						].map(([slot, label]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-[10px] uppercase tracking-wide text-[var(--color-subtle)]",
									children: label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium text-[var(--color-fg)]",
									children: friendlyModelName(modelCatalog.slots[slot])
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "font-mono text-[10px] text-[var(--color-muted)]",
									children: modelCatalog.slots[slot]
								})
							]
						}, slot))
					}),
					modelCatalog.essential.length > 0 && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1",
						children: modelCatalog.essential.map((m) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							className: "font-mono text-[10px]",
							children: m
						}, m))
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "text-[10px] text-[var(--color-subtle)]",
						children: [
							"Source: ",
							modelCatalog.source,
							" · slots by",
							" ",
							modelCatalog.classifiedBy === "grok" ? "Grok" : "heuristic",
							modelCatalog.classifyNotes ? ` · ${modelCatalog.classifyNotes}` : "",
							lastModelsFetchAt ? ` · last poll ${new Date(lastModelsFetchAt).toLocaleTimeString()}` : " · not polled yet"
						]
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Updates (GitHub)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Install the latest clean release from the package repo." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					update && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3 text-sm",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex flex-wrap items-center gap-2",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, { children: ["v", update.currentVersion] }), update.updateAvailable ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "info",
								children: "Update available"
							}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: "success",
								children: "Up to date"
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-2 space-y-1 font-mono text-xs text-[var(--color-muted)]",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: update.detail }), (update.currentSha || update.remoteSha) && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
								"local ",
								update.currentSha || "?",
								update.remoteSha ? ` · remote ${update.remoteSha}` : ""
							] })]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						type: "password",
						autoComplete: "off",
						value: ghDraft,
						onChange: (e) => setGhDraft(e.target.value),
						placeholder: "GitHub token (optional)"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							disabled: updateBusy,
							onClick: () => void onCheckUpdate(),
							children: "Check for updates"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							disabled: updateBusy,
							onClick: () => void onInstallUpdate(),
							children: updateBusy ? "Installing…" : update?.updateAvailable ? "Install latest" : "Reinstall / repair"
						})]
					}),
					updateLog && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("pre", {
						className: "scroll-panel max-h-48 whitespace-pre-wrap rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-3 font-mono text-xs text-[var(--color-muted)]",
						children: updateLog
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Model modes"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Auto routes each prompt to Fast · 4.3 · 4.5 · Build · Imagine balancing tokens." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-2",
				children: getModesWithCatalog(modelCatalog).map((m) => {
					const selected = m.id === mode;
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => setMode(m.id),
						className: cn("flex w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border px-3 py-3 text-left transition-colors", selected ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]" : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-sm font-medium",
								children: m.label
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "text-xs text-[var(--color-muted)]",
								children: m.subtitle
							}),
							m.id !== "auto" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mt-0.5 font-mono text-[10px] text-[var(--color-subtle)]",
								children: m.modelId
							})
						] }), selected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: "Active"
						})]
					}, m.id);
				})
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(HostGatewayBanner, {
				variant: "card",
				onOpenDesktop: () => setNav("desktop")
			}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Arch Linux shell preferences"
			}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-3",
				children: [
					[
						"wayland",
						"Prefer Wayland",
						"Ozone flags"
					],
					[
						"tray",
						"System tray",
						"Minimize to tray"
					],
					[
						"launchOnLogin",
						"Launch on login",
						"Autostart"
					],
					[
						"startMinimized",
						"Start minimized",
						"Tray only"
					]
				].map(([key, label, hint]) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("label", {
					className: "flex cursor-pointer items-center justify-between gap-3 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-sm font-medium",
						children: label
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "text-xs text-[var(--color-muted)]",
						children: hint
					})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("input", {
						type: "checkbox",
						className: "h-4 w-4 accent-[var(--color-fg)]",
						checked: desktop[key],
						onChange: (e) => setDesktop({ [key]: e.target.checked })
					})]
				}, key))
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Danger zone"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Wipe local chat history, connectors, and preferences on this device. Does not revoke Grok OAuth on xAI servers — disconnect first if you want a full sign-out." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "secondary",
				onClick: () => {
					if (typeof window !== "undefined" && window.confirm("Reset GrokHub to a clean install on this device?")) resetDemo();
				},
				children: "Reset to clean install"
			}) })] })
		]
	});
}
function SkillsView() {
	const skills = useGrokHub((s) => s.skills);
	const toggleSkill = useGrokHub((s) => s.toggleSkill);
	const runSkill = useGrokHub((s) => s.runSkill);
	const addSkill = useGrokHub((s) => s.addSkill);
	const running = useGrokHub((s) => s.running);
	const [name, setName] = (0, import_react.useState)("");
	const [slash, setSlash] = (0, import_react.useState)("");
	const [description, setDescription] = (0, import_react.useState)("");
	const [instructions, setInstructions] = (0, import_react.useState)("");
	function onCreate() {
		if (!name.trim() || !instructions.trim()) return;
		addSkill({
			name: name.trim(),
			slash: slash.trim() || `/${name.trim().toLowerCase().replace(/\s+/g, "-")}`,
			description: description.trim() || "Custom skill",
			instructions: instructions.trim()
		});
		setName("");
		setSlash("");
		setDescription("");
		setInstructions("");
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "space-y-5",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "grid gap-4 lg:grid-cols-[1fr_340px]",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
				className: "grid gap-3 sm:grid-cols-2",
				children: skills.map((sk) => /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
					className: "flex flex-col",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex items-start justify-between gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
							className: "flex items-center gap-2 text-sm",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-3.5 w-3.5 text-[var(--color-muted)]" }), sk.name]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, {
							className: "mt-1 font-mono text-xs",
							children: sk.slash
						})] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: sk.kind === "builtin" ? "info" : "default",
							children: sk.kind
						})]
					}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
						className: "mt-auto flex flex-1 flex-col gap-3",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "text-sm text-[var(--color-muted)]",
								children: sk.description
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
								className: "line-clamp-2 text-xs text-[var(--color-subtle)]",
								children: sk.instructions
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "mt-auto flex items-center justify-between gap-2 pt-1",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", {
									className: "tabular text-xs text-[var(--color-subtle)]",
									children: [sk.runs, " runs"]
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "flex gap-2",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
										size: "sm",
										variant: "secondary",
										onClick: () => toggleSkill(sk.id),
										children: sk.enabled ? "Disable" : "Enable"
									}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
										size: "sm",
										disabled: !sk.enabled || running,
										onClick: () => void runSkill(sk.id),
										children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Play, { className: "h-3.5 w-3.5" }), "Run"]
									})]
								})]
							})
						]
					})]
				}, sk.id))
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
				className: "h-fit lg:sticky lg:top-20",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Plus, { className: "h-4 w-4" }), "Skill Creator"]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Teach once — slash command sticks across sessions (local demo memory)." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
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
								placeholder: "Gig day planner"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Slash"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: slash,
								onChange: (e) => setSlash(e.target.value),
								placeholder: "/gigs"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "space-y-1.5",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
								className: "text-xs font-medium text-[var(--color-muted)]",
								children: "Description"
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
								value: description,
								onChange: (e) => setDescription(e.target.value),
								placeholder: "What this skill does"
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
								placeholder: "Steps the agent should always follow…"
							})]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							className: "w-full",
							onClick: onCreate,
							disabled: !name.trim() || !instructions.trim(),
							children: "Save skill"
						})
					]
				})]
			})]
		})
	});
}
var NAV = [
	{
		id: "chat",
		label: "Agent",
		icon: MessageSquare
	},
	{
		id: "history",
		label: "History",
		icon: History
	},
	{
		id: "command",
		label: "Command",
		icon: Command
	},
	{
		id: "desktop",
		label: "Desktop",
		icon: HardDrive
	},
	{
		id: "imagine",
		label: "Imagine",
		icon: Image
	},
	{
		id: "connectors",
		label: "Connectors",
		icon: Cable
	},
	{
		id: "skills",
		label: "Skills",
		icon: Sparkles
	},
	{
		id: "automations",
		label: "Automations",
		icon: TimerReset
	},
	{
		id: "agents",
		label: "Roster",
		icon: Users
	},
	{
		id: "settings",
		label: "Settings",
		icon: Settings
	}
];
function AppShell() {
	const nav = useGrokHub((s) => s.nav);
	const setNav = useGrokHub((s) => s.setNav);
	const heartbeatAt = useGrokHub((s) => s.heartbeatAt);
	const running = useGrokHub((s) => s.running);
	const mode = useGrokHub((s) => s.mode);
	const tickHeartbeat = useGrokHub((s) => s.tickHeartbeat);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
	const probeGrok = useGrokHub((s) => s.probeGrok);
	const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
	const newThread = useGrokHub((s) => s.newThread);
	const threads = useGrokHub((s) => s.threads);
	const selectThread = useGrokHub((s) => s.selectThread);
	const activeThreadId = useGrokHub((s) => s.activeThreadId);
	const oauth = useGrokHub((s) => s.oauth);
	const profile = useGrokHub((s) => s.profile);
	const { user, isPending } = useCurrentUserState();
	const [mobileOpen, setMobileOpen] = (0, import_react.useState)(false);
	const [isDesktop, setIsDesktop] = (0, import_react.useState)(false);
	const modeMeta = getMode(mode);
	const accountLabel = oauth?.name || oauth?.email || profile?.displayName || profile?.email || (user && !user.isDevFallback ? user.displayName || user.primaryEmail || null : null);
	const accountConnected = Boolean(oauth?.accessToken || user && !user.isDevFallback || grokConnected);
	(0, import_react.useEffect)(() => {
		const p = useGrokHub.persist.rehydrate();
		Promise.resolve(p).finally(() => {
			useGrokHub.setState({ nav: "chat" });
			const st = useGrokHub.getState();
			st.refreshStaleTimes();
			st.tickHeartbeat();
			if (st.oauth?.accessToken) useGrokHub.setState({ connectors: st.connectors.map((c) => c.id === "grok-xai" ? {
				...c,
				status: "connected",
				lastUsed: Date.now()
			} : c) });
			useGrokHub.getState().probeGrok();
			useGrokHub.getState().refreshModels();
			(async () => {
				try {
					const { hostInfo } = await import("./host-client-WUUmAwRI.mjs");
					const info = await hostInfo();
					if (info.unsandboxed && info.bridge !== "none") useGrokHub.setState((s) => ({ connectors: s.connectors.map((c) => c.id === "desktop-host" ? {
						...c,
						status: "connected",
						lastUsed: Date.now()
					} : c) }));
				} catch {}
			})();
		});
		setIsDesktop(Boolean(window.grokhubDesktop));
	}, []);
	(0, import_react.useEffect)(() => {
		const MODELS_POLL_MS = 300 * 1e3;
		const tick = () => {
			const st = useGrokHub.getState();
			if (st.oauth?.accessToken || st.apiKey || st.grokConnected) st.refreshModels();
		};
		const id = window.setInterval(tick, MODELS_POLL_MS);
		const t = window.setTimeout(tick, 8e3);
		return () => {
			window.clearInterval(id);
			window.clearTimeout(t);
		};
	}, []);
	(0, import_react.useEffect)(() => {
		if (oauth?.name || oauth?.email) {
			syncFromGrok({
				displayName: oauth.name ?? null,
				email: oauth.email ?? null,
				imageUrl: oauth.picture ?? null
			});
			return;
		}
		if (isPending) return;
		if (user && !user.isDevFallback) syncFromGrok({
			displayName: user.displayName,
			email: user.primaryEmail,
			imageUrl: user.profileImageUrl
		});
	}, [
		user,
		isPending,
		syncFromGrok,
		oauth?.name,
		oauth?.email,
		oauth?.picture
	]);
	(0, import_react.useEffect)(() => {
		const hb = window.setInterval(() => tickHeartbeat(), 3e4);
		return () => window.clearInterval(hb);
	}, [tickHeartbeat]);
	(0, import_react.useEffect)(() => {
		setMobileOpen(false);
	}, [nav]);
	const drag = { WebkitAppRegion: "drag" };
	const noDrag = { WebkitAppRegion: "no-drag" };
	const recent = [...threads].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, 6);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-dvh max-h-dvh w-full max-w-none flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "flex h-10 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3",
			style: drag,
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex items-center gap-2",
				style: noDrag,
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(GrokHubMark, { className: "h-6 w-6" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
						className: "text-xs font-semibold tracking-tight",
						children: "GrokHub"
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
						className: "hidden font-mono text-[10px] sm:inline-flex",
						children: ["v", APP_VERSION]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => void probeGrok(),
						className: "hidden items-center gap-1.5 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] md:inline-flex",
						title: grokStatusDetail,
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: cn("inline-block h-1.5 w-1.5 rounded-full", grokConnected === true ? "bg-[var(--color-success)]" : grokConnected === false ? "bg-[var(--color-danger)]" : "bg-[var(--color-subtle)]") }), grokConnected === true ? "Grok live" : grokConnected === false ? "Grok offline" : "Grok…"]
					})
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-w-0 items-center gap-2",
				style: noDrag,
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "hidden max-w-[160px] sm:flex" }),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ModePicker, {}),
					isPending && !oauth ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { className: "hidden h-7 w-20 animate-pulse rounded bg-[var(--color-elevated)] sm:block" }) : accountLabel ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => setNav("settings"),
						className: "hidden max-w-[10rem] truncate rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)] sm:inline",
						title: accountLabel,
						children: accountLabel
					}) : user && !user.isDevFallback ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "hidden scale-90 sm:block",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UserButton, {})
					}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => setNav("settings"),
						className: "hidden rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)] sm:inline",
						children: "Connect Grok"
					}),
					isDesktop && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "ml-1 flex items-center gap-0.5",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
								onClick: () => window.grokhubDesktop?.minimize?.(),
								"aria-label": "Minimize",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Minus, { className: "h-3.5 w-3.5" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]",
								onClick: () => window.grokhubDesktop?.maximize?.(),
								"aria-label": "Maximize",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Square, { className: "h-3 w-3" })
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								className: "flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-danger)_25%,transparent)] hover:text-[var(--color-danger)]",
								onClick: () => window.grokhubDesktop?.close?.(),
								"aria-label": "Close",
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-3.5 w-3.5" })
							})
						]
					})
				]
			})]
		}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
			className: "app-frame flex min-h-0 w-full flex-1 overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
				className: "sidebar-rail hidden shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)] bg-[var(--color-surface)] md:flex",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 p-3 pb-1",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Button, {
							size: "sm",
							className: "w-full",
							variant: "secondary",
							onClick: () => newThread(),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(MessageSquarePlus, { className: "h-4 w-4" }), "New chat"]
						})
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("nav", {
						className: "scroll-panel flex flex-1 flex-col gap-1 p-3 pt-1",
						children: [NAV.map((item) => {
							const Icon = item.icon;
							return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
								type: "button",
								onClick: () => setNav(item.id),
								className: cn("flex h-10 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors", nav === item.id ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]"),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-4 w-4 shrink-0" }), item.label]
							}, item.id);
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "mt-3 border-t border-[var(--color-border)] pt-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
								className: "mb-1.5 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]",
								children: "Recent"
							}), recent.map((t) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
								type: "button",
								onClick: () => selectThread(t.id),
								className: cn("mb-0.5 flex w-full flex-col rounded-[var(--radius-sm)] px-2.5 py-1.5 text-left text-xs transition-colors", t.id === activeThreadId ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/50"),
								children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
									className: "truncate font-medium",
									children: t.title
								})
							}, t.id))]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "shrink-0 space-y-2 border-t border-[var(--color-border)] p-4",
						children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2 text-xs text-[var(--color-muted)]",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "pulse-live inline-block h-2 w-2 rounded-full bg-[var(--color-success)]" }),
								"Heartbeat ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: heartbeatAt })
							]
						})
					})
				]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "app-stage flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
						className: "flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-bg)_88%,transparent)] px-4 py-3 backdrop-blur-md md:px-6 3xl:px-8 uw:px-10",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex min-w-0 items-center gap-3",
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
								variant: "ghost",
								size: "icon",
								className: "md:hidden",
								onClick: () => setMobileOpen((v) => !v),
								"aria-label": "Menu",
								children: mobileOpen ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(X, { className: "h-5 w-5" }) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Menu, { className: "h-5 w-5" })
							}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: "min-w-0",
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-sm font-medium md:text-base",
									children: NAV.find((n) => n.id === nav)?.label
								}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "truncate text-xs text-[var(--color-subtle)]",
									children: [
										"GrokHub v",
										APP_VERSION,
										" ·",
										" ",
										accountLabel ? accountLabel : accountConnected ? grokStatusDetail || "Grok connected" : "Connect Grok in Settings"
									]
								})]
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex shrink-0 items-center gap-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "max-w-[140px] sm:hidden" }),
								running ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "info",
									children: "Working"
								}) : /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
									variant: "success",
									children: "Online"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Badge, {
									className: "hidden font-mono sm:inline-flex",
									children: [
										modeMeta.label,
										" · ",
										modeMeta.model
									]
								})
							]
						})]
					}),
					mobileOpen && /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface)] p-2 md:hidden",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "mb-2 px-1",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "w-full" })
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "grid grid-cols-2 gap-1",
							children: NAV.map((item) => {
								const Icon = item.icon;
								return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
									type: "button",
									onClick: () => setNav(item.id),
									className: cn("flex h-11 items-center gap-2 rounded-[var(--radius-sm)] px-3 text-sm", nav === item.id ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)]"),
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-4 w-4" }), item.label]
								}, item.id);
							})
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("main", {
						className: "app-stage flex min-h-0 flex-1 flex-col overflow-hidden p-3 sm:p-4 md:p-5 3xl:p-6 uw:p-8",
						children: (nav === "chat" || nav === "history") && nav === "chat" ? /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "chat-stage min-h-0 flex-1 overflow-hidden",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChatView, {})
						}) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "scroll-panel min-h-0 flex-1",
							children: [
								nav === "history" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(HistoryView, {}),
								nav === "command" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandView, {}),
								nav === "connectors" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ConnectorsView, {}),
								nav === "skills" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkillsView, {}),
								nav === "automations" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AutomationsView, {}),
								nav === "agents" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentsView, {}),
								nav === "imagine" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImagineView, {}),
								nav === "desktop" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DesktopHostView, {}),
								nav === "settings" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsView, {})
							]
						})
					})
				]
			})]
		})]
	});
}
function HomePage() {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AppShell, {});
}
//#endregion
export { grokImagine as a, oauthPoll as c, HomePage as component, grokChatStream as i, oauthStart as l, checkUpdate as n, grokProbe as o, grokChat as r, oauthEnsure as s, applyUpdate as t };
