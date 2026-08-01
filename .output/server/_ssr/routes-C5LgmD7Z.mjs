import { r as __toESM } from "../_runtime.mjs";
import { M as require_react, j as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { A as Brain, C as Folder, D as ChevronRight, E as Command, M as AppWindow, N as Activity, O as Check, S as Gauge, T as Download, _ as LoaderCircle, a as Terminal, b as HardDrive, c as ShieldAlert, d as RefreshCw, f as Plus, g as Menu, h as MessageSquare, i as TimerReset, j as ArrowRight, k as Cable, l as Settings, m as Minus, n as X, o as Square, p as Play, r as Users, s as Sparkles, t as Zap, u as Send, v as Link2Off, w as FolderOpen, x as Hammer, y as Image } from "../_libs/lucide-react.mjs";
import { n as create, t as persist } from "../_libs/zustand.mjs";
import { n as clsx, t as cva } from "../_libs/class-variance-authority+clsx.mjs";
import { t as twMerge } from "../_libs/tailwind-merge.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/routes-C5LgmD7Z.js
var import_react = /* @__PURE__ */ __toESM(require_react());
var import_jsx_runtime = require_jsx_runtime();
async function rpc(path, action, body = {}) {
	const res = await fetch(path, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify({
			action,
			...body
		})
	});
	const data = await res.json().catch(() => ({}));
	if (!res.ok) throw new Error(data.error || `HTTP ${res.status}`);
	return data;
}
async function grokChat(opts) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.chat) return desktop.chat(opts);
	return rpc("/api/grok", "chat", opts);
}
async function grokProbe(apiKey) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.probe) return desktop.probe(apiKey);
	return rpc("/api/grok", "probe", { apiKey: apiKey || "" });
}
async function checkUpdate(token) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.checkUpdate) return desktop.checkUpdate({ token });
	return rpc("/api/update", "check", { token: token || "" });
}
async function applyUpdate(token) {
	const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (desktop?.applyUpdate) return desktop.applyUpdate({ token });
	return rpc("/api/update", "apply", { token: token || "" });
}
var GROK_MODES = [
	{
		id: "auto",
		label: "Auto",
		subtitle: "Chooses Fast or Expert",
		model: "Grok 4.5",
		icon: "auto",
		latencyMs: [400, 900],
		depth: "standard"
	},
	{
		id: "fast",
		label: "Fast",
		subtitle: "Quick responses · Grok 4.5",
		model: "Grok 4.5",
		icon: "fast",
		latencyMs: [250, 500],
		depth: "light"
	},
	{
		id: "expert",
		label: "Expert",
		subtitle: "Thinks hard · Grok 4.5",
		model: "Grok 4.5",
		icon: "expert",
		latencyMs: [900, 1600],
		depth: "deep"
	},
	{
		id: "heavy",
		label: "Heavy",
		subtitle: "Team of Experts · Grok 4.5",
		model: "Grok 4.5",
		icon: "heavy",
		latencyMs: [1400, 2400],
		depth: "team"
	},
	{
		id: "build",
		label: "Build",
		subtitle: "Build apps and sites · Grok 4.5",
		model: "Grok 4.5",
		icon: "build",
		latencyMs: [700, 1400],
		depth: "code"
	}
];
function getMode(id) {
	return GROK_MODES.find((m) => m.id === id) ?? GROK_MODES[0];
}
/** Auto routes simple prompts to Fast, deeper ones to Expert. */
function resolveMode(id, prompt) {
	if (id !== "auto") return id;
	const p = prompt.toLowerCase();
	return p.includes("architect") || p.includes("debug") || p.includes("why") || p.includes("compare") || p.includes("research") || p.includes("plan") || p.length > 160 || p.split(/\s+/).length > 28 ? "expert" : "fast";
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
var MINUTE = 6e4;
var HOUR = 60 * MINUTE;
/** Fresh relative timestamps so activity never freezes at a calendar date. */
function createSeeds(now = Date.now()) {
	return {
		connectors: [
			{
				id: "gmail",
				name: "Gmail & Calendar",
				category: "Google",
				description: "Read mail, draft replies, and manage calendar events.",
				status: "connected",
				tools: [
					"search_mail",
					"draft_reply",
					"list_events",
					"create_event"
				],
				lastUsed: now - 12 * MINUTE
			},
			{
				id: "gdrive",
				name: "Google Drive",
				category: "Google",
				description: "Search, read, and update Docs, Sheets, and Slides.",
				status: "connected",
				tools: [
					"search_files",
					"read_doc",
					"update_sheet"
				],
				lastUsed: now - 40 * MINUTE
			},
			{
				id: "github",
				name: "GitHub",
				category: "Code",
				description: "Repos, issues, PRs, and code search.",
				status: "connected",
				tools: [
					"list_issues",
					"create_pr_comment",
					"search_code"
				],
				lastUsed: now - 5 * MINUTE
			},
			{
				id: "notion",
				name: "Notion",
				category: "Workspace",
				description: "Pages, databases, and wikis across your workspace.",
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
				description: "Inbox triage, drafts, and meeting management.",
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
				description: "Channels, chats, and message summaries.",
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
				description: "Issues, projects, and roadmap updates.",
				status: "connected",
				tools: [
					"list_issues",
					"create_issue",
					"update_status"
				],
				lastUsed: now - 90 * MINUTE
			},
			{
				id: "onedrive",
				name: "OneDrive",
				category: "Microsoft",
				description: "Personal and shared files in OneDrive.",
				status: "disconnected",
				tools: ["search_files", "read_file"]
			},
			{
				id: "vercel",
				name: "Vercel",
				category: "Deploy",
				description: "Projects, deployments, and domain status.",
				status: "disconnected",
				tools: ["list_deploys", "get_status"]
			},
			{
				id: "custom-mcp",
				name: "Custom MCP",
				category: "Custom",
				description: "Bring your own MCP server for local or private tools.",
				status: "disconnected",
				tools: ["discover_tools", "invoke_tool"]
			}
		],
		skills: [
			{
				id: "docs",
				name: "Office Documents",
				description: "Create and edit polished Word, PowerPoint, Excel, and PDF files.",
				kind: "builtin",
				enabled: true,
				slash: "/docs",
				instructions: "Generate production-ready office documents with correct structure, styles, and tables.",
				runs: 48
			},
			{
				id: "skill-creator",
				name: "Skill Creator",
				description: "Teach Grok a reusable workflow once; apply it forever.",
				kind: "builtin",
				enabled: true,
				slash: "/skillify",
				instructions: "Capture a conversation workflow into a named persistent skill.",
				runs: 12
			},
			{
				id: "morning-brief",
				name: "Morning Brief",
				description: "Inbox highlights, calendar, open issues, and day plan.",
				kind: "custom",
				enabled: true,
				slash: "/morning",
				instructions: "Pull Gmail + Calendar + GitHub + Linear. Summarize top 5 actions. Keep under 12 bullets.",
				runs: 31
			},
			{
				id: "standup",
				name: "Standup Notes",
				description: "Turn recent activity into a short standup update.",
				kind: "custom",
				enabled: true,
				slash: "/standup",
				instructions: "Summarize yesterday's shipped work, today's plan, and blockers in three bullets.",
				runs: 14
			},
			{
				id: "print-queue",
				name: "Print Queue",
				description: "3D print job tracker and packaging checklist.",
				kind: "custom",
				enabled: false,
				slash: "/prints",
				instructions: "Track print jobs, material, failure notes, and packaging steps for finished parts.",
				runs: 4
			},
			{
				id: "deep-research",
				name: "Deep Research",
				description: "Fan research out across parallel agents with source checks.",
				kind: "builtin",
				enabled: true,
				slash: "/deep-research",
				instructions: "Break the question into sub-queries, verify claims, return a cited report.",
				runs: 17
			}
		],
		automations: [
			{
				id: "auto-morning",
				name: "08:00 Morning Brief",
				instructions: "Run /morning. Email me the brief and post open P0s from Linear + GitHub.",
				schedule: "weekdays",
				time: "08:00",
				enabled: true,
				connectorIds: [
					"gmail",
					"github",
					"linear"
				],
				skillIds: ["morning-brief", "docs"],
				lastRun: now - 14 * HOUR,
				nextRun: now + 10 * HOUR,
				runCount: 27
			},
			{
				id: "auto-inbox",
				name: "Inbox triage",
				instructions: "When mail arrives from bills or work, summarize and draft a reply if needed.",
				schedule: "daily",
				time: "on email",
				enabled: true,
				connectorIds: ["gmail"],
				skillIds: [],
				lastRun: now - 35 * MINUTE,
				runCount: 64
			},
			{
				id: "auto-weekly-review",
				name: "Weekly agent review",
				instructions: "Summarize all automation runs, skill usage, and connector health for the week.",
				schedule: "weekly",
				time: "Sun 18:00",
				enabled: false,
				connectorIds: ["github", "gmail"],
				skillIds: ["docs"],
				runCount: 3
			}
		],
		agents: [
			{
				id: "primary",
				name: "Primary",
				role: "Primary co-pilot",
				model: "xai/grok-4.5 · Auto/Fast/Expert",
				status: "idle",
				tasks: 0,
				color: "#d4d4d8"
			},
			{
				id: "builder",
				name: "Build",
				role: "Build mode",
				model: "xai/grok-4.5 · Build",
				status: "idle",
				tasks: 0,
				color: "#7dd3fc"
			},
			{
				id: "research",
				name: "Research",
				role: "Heavy / Expert",
				model: "xai/grok-4.5 · Heavy",
				status: "offline",
				tasks: 0,
				color: "#34d399"
			},
			{
				id: "ops",
				name: "Ops",
				role: "Ops & automations",
				model: "xai/grok-4.5 · Fast",
				status: "idle",
				tasks: 0,
				color: "#fbbf24"
			}
		],
		activity: [
			{
				id: "a1",
				ts: now - 5 * MINUTE,
				kind: "connector",
				title: "GitHub tools used",
				detail: "list_issues on example/app — 12 open, 2 P0",
				status: "success"
			},
			{
				id: "a2",
				ts: now - 35 * MINUTE,
				kind: "automation",
				title: "Inbox triage completed",
				detail: "3 messages summarized, 1 draft prepared",
				status: "success"
			},
			{
				id: "a3",
				ts: now - 90 * MINUTE,
				kind: "skill",
				title: "Morning Brief ran",
				detail: "Calendar + mail + Linear rolled into day plan",
				status: "success"
			},
			{
				id: "a4",
				ts: now - 3 * HOUR,
				kind: "system",
				title: "Build mode default",
				detail: "Auto / Fast / Expert / Heavy / Build + Imagine loaded",
				status: "success"
			},
			{
				id: "a5",
				ts: now - 8 * MINUTE,
				kind: "desktop",
				title: "Host bridge ready",
				detail: "Unsandboxed CLI · files · apps (Electron or server bridge)",
				status: "success"
			}
		],
		chat: [{
			id: "c0",
			role: "system",
			content: "GrokHub desktop online (v0.1). Modes Auto/Fast/Expert/Heavy/Build map to live xAI Grok models. Add your API key in Settings to connect. Prefix shell with $ for host CLI.",
			ts: now - 2 * MINUTE
		}],
		heartbeatAt: now
	};
}
/** Module-load seeds for first paint; client remounts via createSeeds on reset. */
var boot$1 = createSeeds();
boot$1.connectors;
boot$1.skills;
boot$1.automations;
boot$1.agents;
boot$1.activity;
boot$1.chat;
boot$1.heartbeatAt;
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
function cn(...inputs) {
	return twMerge(clsx(inputs));
}
function formatRelative(ts) {
	if (!Number.isFinite(ts) || ts <= 0) return "—";
	const diff = Date.now() - ts;
	if (diff < 0) {
		if (Math.floor(-diff / 1e3) < 60) return "in a moment";
		return "soon";
	}
	const sec = Math.floor(diff / 1e3);
	if (sec < 10) return "just now";
	if (sec < 60) return `${sec}s ago`;
	const min = Math.floor(sec / 60);
	if (min < 60) return `${min}m ago`;
	const hr = Math.floor(min / 60);
	if (hr < 24) return `${hr}h ago`;
	const day = Math.floor(hr / 24);
	if (day > 60) return "just now";
	return `${day}d ago`;
}
function uid(prefix = "id") {
	return `${prefix}_${Math.random().toString(36).slice(2, 9)}`;
}
function modePrefix(mode, routed) {
	const m = getMode(routed);
	if (mode === "auto" && routed !== "auto") return `[Auto → ${m.label} · ${m.model}]`;
	return `[${m.label} · ${m.model}]`;
}
function replyFor(text, s, routed) {
	const lower = text.toLowerCase();
	const connected = s.connectors.filter((c) => c.status === "connected");
	const enabledSkills = s.skills.filter((sk) => sk.enabled);
	const head = modePrefix(s.mode, routed);
	const depth = getMode(routed).depth;
	const plan = PLAN_LIMITS[s.usage.plan];
	const pct = Math.round(usagePercent(s.usage));
	if (lower.includes("usage") || lower.includes("quota") || lower.includes("limit") || lower.includes("subscription")) return [
		head,
		"",
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
			head,
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
		head,
		"",
		"Standup",
		"",
		"- Yesterday: connector triage + mode routing polish",
		"- Today: desktop host checks and packaging notes",
		"- Blockers: none — usage meter and Imagine ready for demos",
		depth === "code" ? "- Build: keep /standup skill logging shipped items weekly" : ""
	].filter(Boolean).join("\n");
	if (lower.includes("imagine") || lower.startsWith("/imagine")) return [
		head,
		"",
		"Imagine is available in the Imagine panel.",
		"Describe a scene there — GrokHub renders a local preview on this Arch desktop build.",
		`Imagine quota: ${s.usage.imagine}/${plan.imagine} this period (5 units each).`
	].join("\n");
	if (lower.includes("mode") || lower.includes("fast") || lower.includes("expert") || lower.includes("heavy")) return [
		head,
		"",
		"Baked-in Grok modes (same as web):",
		"- Auto — Chooses Fast or Expert",
		"- Fast — Quick responses · Grok 4.5 · 1 unit",
		"- Expert — Thinks hard · Grok 4.5 · 4 units",
		"- Heavy — Team of Experts · Grok 4.5 · 8 units",
		"- Build — Build apps and sites · Grok 4.5 · 2 units",
		"",
		`Active: ${getMode(s.mode).label}${s.mode === "auto" ? ` (this turn → ${getMode(routed).label})` : ""}`,
		"Change modes from the titlebar picker or Settings."
	].join("\n");
	if (lower.includes("connector") || lower.includes("connect")) return [
		head,
		"",
		"Connector status",
		"",
		...s.connectors.map((c) => `- ${c.name}: ${c.status}`)
	].join("\n");
	if (lower.includes("automat") || lower.includes("schedule")) return [
		head,
		"",
		"Automations",
		"",
		...s.automations.map((a) => `- ${a.enabled ? "ON" : "OFF"} ${a.name} (${a.schedule} @ ${a.time}) · ${a.runCount} runs`)
	].join("\n");
	if (lower.includes("skill")) return [
		head,
		"",
		"Skills",
		"",
		...enabledSkills.map((sk) => `- ${sk.slash} — ${sk.name}`)
	].join("\n");
	if (depth === "code" || lower.includes("build") || lower.includes("arch") || lower.includes("desktop")) return [
		head,
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
		head,
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
		head,
		"",
		"Expert analysis",
		"",
		`Reading: ${text}`,
		"",
		"Constraints: local-first control plane, Grok 4.5 modes, Arch desktop target.",
		"Approach: gather connector state → apply enabled skills → leave run log.",
		"Tradeoff: Fast is cheaper/latency; Expert/Heavy spend units for depth.",
		"",
		`Live tools: ${connected.map((c) => c.name).join(", ") || "none connected"}.`
	].join("\n");
	if (depth === "light") return [
		head,
		"",
		`Got it — ${text.slice(0, 120)}${text.length > 120 ? "…" : ""}`,
		`Using ${connected.length} connectors · ${enabledSkills.length} skills · ${pct}% quota.`,
		"Say /morning, open Imagine, or switch to Expert for deeper work."
	].join("\n");
	return [
		head,
		"",
		"Primary co-pilot",
		"",
		`Goal: ${text}`,
		`Using ${connected.length} connectors and ${enabledSkills.length} enabled skills.`,
		"Next: break into steps → pull tools → run skills → log.",
		"Try /morning, /standup, Imagine, or Heavy mode for a team pass."
	].join("\n");
}
function initialFromSeeds() {
	const s = createSeeds();
	return {
		connectors: s.connectors,
		skills: s.skills,
		automations: s.automations,
		activity: s.activity,
		chat: s.chat,
		agents: s.agents,
		heartbeatAt: s.heartbeatAt
	};
}
var boot = initialFromSeeds();
var useGrokHub = create()(persist((set, get) => ({
	nav: "command",
	mode: "build",
	modeMenuOpen: false,
	connectors: boot.connectors,
	skills: boot.skills,
	automations: boot.automations,
	activity: boot.activity,
	chat: boot.chat,
	agents: boot.agents,
	imagineJobs: [],
	imaginePrompt: "",
	imagineAspect: "1:1",
	desktop: {
		startMinimized: false,
		launchOnLogin: false,
		wayland: true,
		tray: true
	},
	usage: createUsage("pro"),
	heartbeatAt: boot.heartbeatAt,
	running: false,
	apiKey: "",
	githubToken: "",
	grokConnected: null,
	grokStatusDetail: "Not connected — add an xAI API key in Settings",
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
	probeGrok: async () => {
		try {
			const { grokProbe } = await import("./grok-client-BL-IqUiv.mjs");
			const r = await grokProbe(get().apiKey || void 0);
			set({
				grokConnected: r.ok,
				grokStatusDetail: r.detail + (r.envConfigured && !get().apiKey ? " (env key)" : "")
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
		set((s) => ({ connectors: s.connectors.map((c) => {
			if (c.id !== id) return c;
			return {
				...c,
				status: c.status === "connected" ? "disconnected" : "connected",
				lastUsed: c.status === "connected" ? c.lastUsed : Date.now()
			};
		}) }));
		const c = get().connectors.find((x) => x.id === id);
		if (c) get().pushActivity({
			kind: "connector",
			title: c.status === "connected" ? `Connected ${c.name}` : `Disconnected ${c.name}`,
			detail: c.status === "connected" ? `Tools available: ${c.tools.join(", ")}` : "OAuth session cleared (demo)",
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
	sendChat: async (text) => {
		const trimmed = text.trim();
		if (!trimmed) return;
		const mode = get().mode;
		const routed = resolveMode(mode, trimmed);
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
		set((s) => ({
			chat: [...s.chat, userMsg],
			running: true
		}));
		get().setAgentStatus(routed === "build" ? "builder" : routed === "heavy" ? "research" : "primary", "working", 1);
		if (routed === "heavy") {
			get().setAgentStatus("ops", "working", 1);
			get().setAgentStatus("builder", "working", 1);
		}
		const isLocalSlash = trimmed.startsWith("/morning") || trimmed.startsWith("/standup") || trimmed.startsWith("/docs") || trimmed.startsWith("/prints");
		let answer;
		let usedLive = false;
		try {
			if (isLocalSlash) {
				await wait(280);
				answer = replyFor(trimmed, get(), routed);
			} else {
				const { grokChat } = await import("./grok-client-BL-IqUiv.mjs");
				const history = get().chat.filter((c) => c.role === "user" || c.role === "assistant").slice(-16).map((c) => ({
					role: c.role,
					content: c.content
				}));
				if (!history.length || history[history.length - 1]?.content !== trimmed) history.push({
					role: "user",
					content: trimmed
				});
				const result = await grokChat({
					messages: history,
					mode: routed,
					apiKey: get().apiKey || void 0
				});
				if (result.ok && result.content) {
					usedLive = true;
					answer = [
						mode === "auto" && routed !== "auto" ? `[Auto → ${m.label} · ${result.model || m.model}]` : `[${m.label} · ${result.model || m.model}]`,
						"",
						result.content
					].join("\n");
					set({
						grokConnected: true,
						grokStatusDetail: `Live · ${result.model || "Grok"}`
					});
				} else {
					answer = [
						modePrefix(mode, routed),
						"",
						"Could not reach Grok.",
						result.error || "Unknown error",
						"",
						"Fix: Settings → paste your xAI API key (console.x.ai) or set XAI_API_KEY.",
						"",
						"— Offline fallback —",
						replyFor(trimmed, get(), routed)
					].join("\n");
					set({
						grokConnected: false,
						grokStatusDetail: result.error || "Grok request failed"
					});
				}
			}
		} catch (e) {
			const msg = e instanceof Error ? e.message : "request failed";
			answer = [
				modePrefix(mode, routed),
				"",
				`Grok connection error: ${msg}`,
				"",
				"— Offline fallback —",
				replyFor(trimmed, get(), routed)
			].join("\n");
			set({
				grokConnected: false,
				grokStatusDetail: msg
			});
		}
		const bot = {
			id: uid("msg"),
			role: "assistant",
			content: answer,
			ts: Date.now(),
			mode: routed
		};
		set((s) => ({
			chat: [...s.chat, bot],
			running: false
		}));
		get().setAgentStatus("primary", "idle", 0);
		get().setAgentStatus("builder", "idle", 0);
		get().setAgentStatus("research", "idle", 0);
		get().setAgentStatus("ops", "idle", 0);
		get().pushActivity({
			kind: "chat",
			title: usedLive ? `Grok · ${m.label}` : `Agent reply · ${m.label}`,
			detail: `${trimmed.slice(0, 80)} · ${bill.cost}u`,
			status: usedLive ? "success" : "failed"
		});
	},
	setImaginePrompt: (v) => set({ imaginePrompt: v }),
	setImagineAspect: (v) => set({ imagineAspect: v }),
	runImagine: async (prompt) => {
		const p = (prompt ?? get().imaginePrompt).trim();
		if (!p) return;
		const bill = get().recordUsage("imagine");
		if (!bill.ok) return;
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
			running: true,
			imaginePrompt: p
		}));
		get().pushActivity({
			kind: "imagine",
			title: "Imagine rendering",
			detail: `${p.slice(0, 100)} · ${bill.cost}u`,
			status: "running"
		});
		const m = getMode(resolveMode(mode, p));
		await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
		const imageDataUrl = renderImaginePreview(p, aspect);
		set((s) => ({
			running: false,
			imagineJobs: s.imagineJobs.map((j) => j.id === id ? {
				...j,
				status: "ready",
				imageDataUrl
			} : j)
		}));
		get().pushActivity({
			kind: "imagine",
			title: "Imagine ready",
			detail: p.slice(0, 120),
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
			agents: fresh.agents,
			imagineJobs: [],
			imaginePrompt: "",
			imagineAspect: "1:1",
			mode: "build",
			heartbeatAt: fresh.heartbeatAt,
			running: false,
			nav: "command",
			modeMenuOpen: false,
			usage: createUsage("pro")
		});
	}
}), {
	name: "grokhub-v2",
	partialize: (s) => ({
		connectors: s.connectors,
		skills: s.skills,
		automations: s.automations,
		activity: s.activity,
		chat: s.chat,
		agents: s.agents,
		mode: s.mode,
		desktop: s.desktop,
		usage: s.usage,
		imagineJobs: s.imagineJobs.slice(0, 8),
		imagineAspect: s.imagineAspect,
		apiKey: s.apiKey,
		githubToken: s.githubToken
	}),
	skipHydration: true
}));
function wait(ms) {
	return new Promise((r) => setTimeout(r, ms));
}
/** Official Grok mark path (xAI brand) — monochrome, scales with currentColor. */
function GrokLogo({ className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("svg", {
		viewBox: "0 0 24 24",
		fill: "currentColor",
		fillRule: "evenodd",
		xmlns: "http://www.w3.org/2000/svg",
		className: cn("shrink-0", className),
		"aria-hidden": true,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("title", { children: "Grok" }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("path", { d: "M9.27 15.29l7.978-5.897c.391-.29.95-.177 1.137.272.98 2.369.542 5.215-1.41 7.169-1.951 1.954-4.667 2.382-7.149 1.406l-2.711 1.257c3.889 2.661 8.611 2.003 11.562-.953 2.341-2.344 3.066-5.539 2.388-8.42l.006.007c-.983-4.232.242-5.924 2.75-9.383.06-.082.12-.164.179-.248l-3.301 3.305v-.01L9.267 15.292M7.623 16.723c-2.792-2.67-2.31-6.801.071-9.184 1.761-1.763 4.647-2.483 7.166-1.425l2.705-1.25a7.808 7.808 0 00-1.829-1A8.975 8.975 0 005.984 5.83c-2.533 2.536-3.33 6.436-1.962 9.764 1.022 2.487-.653 4.246-2.34 6.022-.599.63-1.199 1.259-1.682 1.925l7.62-6.815" })]
	});
}
/** App icon plate for titlebar / desktop entry previews. */
function GrokHubMark({ className }) {
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: cn("flex items-center justify-center rounded border border-[var(--color-border)] bg-[var(--color-elevated)] text-[var(--color-fg)]", className),
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(GrokLogo, { className: "h-[62%] w-[62%]" })
	});
}
var ICONS = {
	auto: Sparkles,
	fast: Zap,
	expert: Brain,
	heavy: Users,
	build: Hammer
};
function ModePicker() {
	const mode = useGrokHub((s) => s.mode);
	const open = useGrokHub((s) => s.modeMenuOpen);
	const setMode = useGrokHub((s) => s.setMode);
	const setModeMenuOpen = useGrokHub((s) => s.setModeMenuOpen);
	const ref = (0, import_react.useRef)(null);
	const active = GROK_MODES.find((m) => m.id === mode) ?? GROK_MODES[0];
	const ActiveIcon = ICONS[active.id];
	(0, import_react.useEffect)(() => {
		if (!open) return;
		const onDoc = (e) => {
			if (!ref.current?.contains(e.target)) setModeMenuOpen(false);
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
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "relative",
		ref,
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
			type: "button",
			onClick: () => setModeMenuOpen(!open),
			className: cn("flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 text-left transition-colors hover:border-[var(--color-border-strong)]", open && "border-[var(--color-border-strong)]"),
			"aria-haspopup": "listbox",
			"aria-expanded": open,
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
					className: "hidden text-[10px] text-[var(--color-subtle)] sm:inline",
					children: "Grok 4.5"
				})
			]
		}), open && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			role: "listbox",
			className: "absolute right-0 top-[calc(100%+6px)] z-50 w-[min(100vw-2rem,300px)] overflow-hidden rounded-[var(--radius-lg)] border border-[var(--color-border)] bg-[var(--color-panel)] p-1.5 shadow-[var(--shadow-soft)]",
			children: GROK_MODES.map((m) => {
				const Icon = ICONS[m.id];
				const selected = m.id === mode;
				return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
					type: "button",
					role: "option",
					"aria-selected": selected,
					onClick: () => setMode(m.id),
					className: cn("flex w-full items-start gap-3 rounded-[var(--radius-md)] px-2.5 py-2.5 text-left transition-colors", selected ? "bg-[var(--color-elevated)]" : "hover:bg-[var(--color-elevated)]/70"),
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "mt-0.5 h-4 w-4 shrink-0 text-[var(--color-muted)]" }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "min-w-0 flex-1",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
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
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-xs text-[var(--color-muted)]",
							children: m.subtitle
						})]
					})]
				}, m.id);
			})
		})]
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
/** Compact titlebar / sidebar chip */
function UsageMeterChip({ className }) {
	const usage = useGrokHub((s) => s.usage);
	const setNav = useGrokHub((s) => s.setNav);
	const plan = PLAN_LIMITS[usage.plan];
	const pct = usagePercent(usage);
	const tone = usageTone(pct);
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
		type: "button",
		onClick: () => setNav("settings"),
		title: `${plan.label}: ${formatUnits(usage.usedUnits)} / ${formatUnits(plan.units)} units`,
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
					style: { width: `${pct}%` }
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
var buttonVariants = cva("inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[var(--radius-sm)] text-sm font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-ring)] disabled:pointer-events-none disabled:opacity-50 active:scale-[0.98]", {
	variants: {
		variant: {
			default: "bg-[var(--color-accent)] text-[var(--color-accent-fg)] hover:opacity-90",
			secondary: "bg-[var(--color-elevated)] text-[var(--color-fg)] border border-[var(--color-border)] hover:border-[var(--color-border-strong)]",
			ghost: "text-[var(--color-muted)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]",
			danger: "bg-[color-mix(in_oklab,var(--color-danger)_18%,transparent)] text-[var(--color-danger)] border border-[color-mix(in_oklab,var(--color-danger)_35%,transparent)] hover:bg-[color-mix(in_oklab,var(--color-danger)_28%,transparent)]"
		},
		size: {
			default: "h-10 px-4 py-2",
			sm: "h-8 px-3 text-xs",
			lg: "h-11 px-5",
			icon: "h-10 w-10"
		}
	},
	defaultVariants: {
		variant: "default",
		size: "default"
	}
});
var Button = import_react.forwardRef(({ className, variant, size, ...props }, ref) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
	ref,
	className: cn(buttonVariants({
		variant,
		size,
		className
	})),
	...props
}));
Button.displayName = "Button";
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
var SUGGESTIONS = [
	"What can you help me with?",
	"/morning",
	"$ uname -a",
	"What's my usage?",
	"Explain my modes"
];
function ChatView() {
	const chat = useGrokHub((s) => s.chat);
	const sendChat = useGrokHub((s) => s.sendChat);
	const running = useGrokHub((s) => s.running);
	const mode = useGrokHub((s) => s.mode);
	const setNav = useGrokHub((s) => s.setNav);
	const pushActivity = useGrokHub((s) => s.pushActivity);
	const recordUsage = useGrokHub((s) => s.recordUsage);
	const usage = useGrokHub((s) => s.usage);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const [text, setText] = (0, import_react.useState)("");
	const [localRunning, setLocalRunning] = (0, import_react.useState)(false);
	const endRef = (0, import_react.useRef)(null);
	const modeMeta = getMode(mode);
	const busy = running || localRunning;
	const plan = PLAN_LIMITS[usage.plan];
	const pct = Math.round(usagePercent(usage));
	(0, import_react.useEffect)(() => {
		endRef.current?.scrollIntoView({ behavior: "smooth" });
	}, [chat, busy]);
	async function runShell(command) {
		setLocalRunning(true);
		const userLine = command.startsWith("$") ? command : `$ ${command}`;
		useGrokHub.setState((s) => ({ chat: [...s.chat, {
			id: `u_${Date.now()}`,
			role: "user",
			content: userLine,
			ts: Date.now(),
			mode
		}] }));
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
			const { hostExec } = await import("./host-client-BQWTZ47b.mjs");
			const r = await hostExec(cmd);
			const body = [
				`[Desktop host · ${r.ok ? "ok" : "fail"} · exit ${r.code ?? "?"} · ${bill.cost}u]`,
				`cwd: ${r.cwd}`,
				"",
				r.stdout || "(no stdout)",
				r.stderr ? `\n[stderr]\n${r.stderr}` : ""
			].filter(Boolean).join("\n");
			useGrokHub.setState((s) => ({ chat: [...s.chat, {
				id: `a_${Date.now()}`,
				role: "assistant",
				content: body,
				ts: Date.now(),
				mode
			}] }));
			pushActivity({
				kind: "desktop",
				title: r.ok ? "Host command ok" : "Host command failed",
				detail: `${cmd.slice(0, 100)} · ${bill.cost}u`,
				status: r.ok ? "success" : "failed"
			});
		} catch (e) {
			useGrokHub.setState((s) => ({ chat: [...s.chat, {
				id: `a_${Date.now()}`,
				role: "assistant",
				content: `Host exec error: ${e instanceof Error ? e.message : "failed"}`,
				ts: Date.now(),
				mode
			}] }));
		} finally {
			setLocalRunning(false);
		}
	}
	async function onSend(value) {
		const payload = (value ?? text).trim();
		if (!payload || busy) return;
		if (payload.toLowerCase().includes("imagine") && !payload.startsWith("/") && !payload.startsWith("$")) setNav("imagine");
		setText("");
		if (payload.startsWith("$") || payload.startsWith("/sh ")) {
			await runShell(payload);
			return;
		}
		await sendChat(payload);
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
		className: "mx-auto flex h-full min-h-0 max-w-3xl flex-col gap-3",
		children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, {
			className: "flex min-h-0 flex-1 flex-col overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardHeader, {
				className: "shrink-0 border-b border-[var(--color-border)]",
				children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex items-start justify-between gap-3",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
						className: "text-sm",
						children: "Agent session"
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
						"Live Grok via xAI · ",
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "font-mono",
							children: "$"
						}),
						" for host shell · mode units apply"
					] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-col items-end gap-1",
						children: [
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								className: "font-mono",
								children: modeMeta.label
							}),
							/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								variant: grokConnected ? "success" : "default",
								className: "text-[10px]",
								children: grokConnected ? "Grok live" : "Offline / key needed"
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
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-5",
					children: [
						chat.map((m) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: cn("flex", m.role === "user" ? "justify-end" : "justify-start"),
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
								className: cn("max-w-[92%] rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed whitespace-pre-wrap", m.role === "user" ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]" : m.role === "system" ? "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]" : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)]"),
								children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
									className: "mb-1 flex flex-wrap items-center gap-2 text-[10px] uppercase tracking-wide text-[var(--color-subtle)]",
									children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("span", { children: [
										m.role,
										" · ",
										/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: m.ts })
									] }), m.mode && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
										className: "rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case",
										children: getMode(m.mode).label
									})]
								}), m.content]
							})
						}, m.id)),
						busy && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "text-xs text-[var(--color-subtle)]",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
								className: "shimmer rounded px-1",
								children: localRunning ? "Host running…" : `${modeMeta.label} · Grok thinking…`
							})
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { ref: endRef })
					]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "shrink-0 space-y-2 border-t border-[var(--color-border)] p-3 md:p-4",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
						className: "flex flex-wrap gap-1.5",
						children: SUGGESTIONS.map((s) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
							type: "button",
							disabled: busy,
							onClick: () => void onSend(s),
							className: "rounded-full border border-[var(--color-border)] px-2.5 py-1 text-xs text-[var(--color-muted)] transition-colors hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]",
							children: s
						}, s))
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
						className: "flex gap-2",
						onSubmit: (e) => {
							e.preventDefault();
							onSend();
						},
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							value: text,
							onChange: (e) => setText(e.target.value),
							placeholder: "Message Grok… or $ shell",
							disabled: busy,
							className: "flex-1"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							type: "submit",
							disabled: busy || !text.trim(),
							size: "icon",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Send, { className: "h-4 w-4" })
						})]
					})]
				})]
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
					children: GROK_MODES.map((m) => {
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
	const toggleConnector = useGrokHub((s) => s.toggleConnector);
	const [q, setQ] = (0, import_react.useState)("");
	const filtered = (0, import_react.useMemo)(() => {
		const needle = q.trim().toLowerCase();
		if (!needle) return connectors;
		return connectors.filter((c) => c.name.toLowerCase().includes(needle) || c.category.toLowerCase().includes(needle) || c.description.toLowerCase().includes(needle));
	}, [connectors, q]);
	const connected = connectors.filter((c) => c.status === "connected").length;
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "space-y-5",
		children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Card, { children: /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
			className: "gap-3 sm:flex-row sm:items-end sm:justify-between",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
				className: "flex items-center gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Cable, { className: "h-4 w-4" }), "Grok Connectors"]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: ["OAuth tools Grok can use in chat — email, files, code, CRM, and custom MCP.", ` ${connected} connected.`] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
				value: q,
				onChange: (e) => setQ(e.target.value),
				placeholder: "Search connectors",
				className: "sm:max-w-xs"
			})]
		}) }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
			className: "grid gap-3 md:grid-cols-2 xl:grid-cols-3",
			children: filtered.map((c) => {
				const on = c.status === "connected";
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
							variant: on ? "success" : "default",
							children: on ? "connected" : "offline"
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
								onClick: () => toggleConnector(c.id),
								children: on ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Link2Off, { className: "h-3.5 w-3.5" }), "Disconnect"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Check, { className: "h-3.5 w-3.5" }), "Connect"] })
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
				const mod = await import("./host-client-BQWTZ47b.mjs");
				if (cancelled) return;
				setApi(mod);
				setIsShell(mod.isDesktopShell());
				setLoading(true);
				const i = await mod.hostInfo();
				if (cancelled) return;
				setInfo(i);
				setCwd(i.cwd);
				setDirPath(i.homedir || i.cwd);
				if (!probed.current) {
					probed.current = true;
					try {
						recordUsage("host");
						const r = await mod.hostExec("uname -a && whoami && pwd && echo --- && ls -la | head -20", i.cwd);
						if (!cancelled) {
							setResult(r);
							setHistory(["uname -a && whoami && pwd && echo --- && ls -la | head -20"]);
						}
					} catch {}
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
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, {
				className: "gap-2",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardTitle, {
					className: "flex items-center gap-2 text-sm",
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(ShieldAlert, { className: "h-4 w-4 text-[var(--color-warn)]" }), "Desktop host · unsandboxed"]
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
	const running = useGrokHub((s) => s.running);
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
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Baked into GrokHub desktop — local preview renderer for Arch offline use. Pair with Expert/Heavy modes for stronger art direction in chat." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "flex flex-wrap gap-2",
					children: [ASPECTS.map((a) => /* @__PURE__ */ (0, import_jsx_runtime.jsx)("button", {
						type: "button",
						onClick: () => setImagineAspect(a),
						className: a === aspect ? "rounded-full border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-1.5 text-xs font-medium" : "rounded-full border border-[var(--color-border)] px-3 py-1.5 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
						children: a
					}, a)), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
						className: "ml-auto font-mono",
						children: mode
					})]
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("form", {
					className: "flex flex-col gap-2 sm:flex-row",
					onSubmit: (e) => {
						e.preventDefault();
						runImagine();
					},
					children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
						value: prompt,
						onChange: (e) => setImaginePrompt(e.target.value),
						placeholder: "Moody night desk, dual monitors, soft amber lamp, film still…",
						disabled: running
					}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
						type: "submit",
						disabled: running || !prompt.trim(),
						className: "sm:w-36",
						children: running ? /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(LoaderCircle, { className: "h-4 w-4 animate-spin" }), "Render"] }) : /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(import_jsx_runtime.Fragment, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Sparkles, { className: "h-4 w-4" }), "Generate"] })
					})]
				})]
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
						download: `grokhub-imagine-${latest.id}.svg`,
						className: "inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Download, { className: "h-3.5 w-3.5" }), "Save SVG"]
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
	const [keyDraft, setKeyDraft] = (0, import_react.useState)(apiKey);
	const [ghDraft, setGhDraft] = (0, import_react.useState)(githubToken);
	const [probing, setProbing] = (0, import_react.useState)(false);
	const [update, setUpdate] = (0, import_react.useState)(null);
	const [updateBusy, setUpdateBusy] = (0, import_react.useState)(false);
	const [updateLog, setUpdateLog] = (0, import_react.useState)("");
	(0, import_react.useEffect)(() => {
		setKeyDraft(apiKey);
	}, [apiKey]);
	(0, import_react.useEffect)(() => {
		setGhDraft(githubToken);
	}, [githubToken]);
	(0, import_react.useEffect)(() => {
		checkUpdate(githubToken || void 0).then(setUpdate).catch((e) => setUpdate({
			currentVersion: "0.1",
			currentSha: null,
			remoteSha: null,
			remoteMessage: null,
			updateAvailable: false,
			repo: "blackviperxiii-ui/spring-dove-reef-apple",
			branch: "main",
			installRoot: null,
			detail: e instanceof Error ? e.message : "check failed"
		}));
	}, [githubToken]);
	async function saveAndProbe() {
		setApiKey(keyDraft.trim());
		setProbing(true);
		try {
			await probeGrok();
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
			const r = await applyUpdate(ghDraft.trim() || void 0);
			setUpdateLog([
				r.detail,
				"",
				...r.steps || []
			].join("\n"));
			if (r.ok) {
				const s = await checkUpdate(ghDraft.trim() || void 0);
				setUpdate(s);
			}
		} catch (e) {
			setUpdateLog(e instanceof Error ? e.message : "update failed");
		} finally {
			setUpdateBusy(false);
		}
	}
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "mx-auto max-w-3xl space-y-5 pb-8",
		children: [
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterPanel, {}),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Grok / xAI connection"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardDescription, { children: [
				"Agent chat uses the live xAI API (api.x.ai). Paste a key from",
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono",
					children: "console.x.ai"
				}),
				" or export",
				" ",
				/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
					className: "font-mono",
					children: "XAI_API_KEY"
				}),
				" in the environment."
			] })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap items-center gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
							variant: grokConnected ? "success" : "default",
							children: grokConnected ? "Connected" : "Not connected"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: grokStatusDetail
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
							className: "text-xs font-medium text-[var(--color-muted)]",
							children: "xAI API key"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							type: "password",
							autoComplete: "off",
							value: keyDraft,
							onChange: (e) => setKeyDraft(e.target.value),
							placeholder: "xai-…"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							onClick: () => void saveAndProbe(),
							disabled: probing,
							children: probing ? "Testing…" : "Save & test"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							onClick: () => {
								setKeyDraft("");
								setApiKey("");
							},
							children: "Clear key"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsx)("p", {
						className: "text-xs text-[var(--color-subtle)]",
						children: "Key stays on this device (local storage). It is only sent to api.x.ai when you chat."
					})
				]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Updates (GitHub)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Pull the latest GrokHub build from the repository and reinstall. Private repos need a GitHub token with contents:read." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
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
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
									"local ",
									update.currentSha || "—",
									" → remote ",
									update.remoteSha || "—"
								] }),
								/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [
									update.repo,
									"@",
									update.branch
								] }),
								update.remoteMessage && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-[var(--color-subtle)]",
									children: update.remoteMessage
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", { children: update.detail })
							]
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "space-y-1.5",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("label", {
							className: "text-xs font-medium text-[var(--color-muted)]",
							children: "GitHub token (optional, private repo)"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Input, {
							type: "password",
							autoComplete: "off",
							value: ghDraft,
							onChange: (e) => setGhDraft(e.target.value),
							placeholder: "ghp_… or github_pat_…"
						})]
					}),
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
						className: "flex flex-wrap gap-2",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							disabled: updateBusy,
							onClick: () => void onCheckUpdate(),
							children: "Check for updates"
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							disabled: updateBusy || (update ? !update.updateAvailable : false),
							onClick: () => void onInstallUpdate(),
							children: updateBusy ? "Working…" : "Install latest"
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
				children: "Model modes (baked in)"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Same control surface as Grok web — Auto, Fast, Expert, Heavy, Build. All on Grok 4.5 family models via xAI." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-2",
				children: GROK_MODES.map((m) => {
					const selected = m.id === mode;
					const cost = m.id === "heavy" ? "8 units" : m.id === "expert" ? "4 units" : m.id === "build" ? "2 units" : m.id === "auto" ? "1.5 units" : "1 unit";
					return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
						type: "button",
						onClick: () => setMode(m.id),
						className: cn("flex w-full items-center justify-between gap-3 rounded-[var(--radius-md)] border px-3 py-3 text-left transition-colors", selected ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)]" : "border-[var(--color-border)] hover:border-[var(--color-border-strong)]"),
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2 text-sm font-medium",
							children: [m.label, m.id === "build" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Badge, {
								className: "text-[10px]",
								children: "Beta"
							})]
						}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "text-xs text-[var(--color-muted)]",
							children: [
								m.subtitle,
								" · ",
								cost,
								"/turn"
							]
						})] }), selected && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", {
							className: "text-xs text-[var(--color-muted)]",
							children: "Active"
						})]
					}, m.id);
				})
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Unsandboxed desktop host"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Full user-session access: shell, files, and installed apps. Host CLI burns 0.25 units per command." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardContent, {
				className: "space-y-3",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
					className: "rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_35%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-sm text-[var(--color-warn)]",
					children: "Commands run as your Linux user. Treat this like giving an agent your terminal."
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
					onClick: () => setNav("desktop"),
					children: "Open Desktop host"
				})]
			})] }),
			/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(Card, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)(CardHeader, { children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardTitle, {
				className: "text-sm",
				children: "Arch Linux desktop shell"
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardDescription, { children: "Electron preferences for Wayland/X11. Window auto-fits the work area on launch." })] }), /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CardContent, {
				className: "space-y-3",
				children: [
					[
						"wayland",
						"Prefer Wayland",
						"Ozone platform flags for Hyprland / KDE / GNOME"
					],
					[
						"tray",
						"System tray",
						"Minimize to tray; click icon to restore"
					],
					[
						"launchOnLogin",
						"Launch on login",
						"Autostart via ~/.config/autostart"
					],
					[
						"startMinimized",
						"Start minimized",
						"Boot to tray only"
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
			/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
				variant: "secondary",
				onClick: resetDemo,
				children: "Reset demo data"
			})
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
var APP_VERSION = "0.1";
var NAV = [
	{
		id: "chat",
		label: "Agent",
		icon: MessageSquare
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
	const refreshStaleTimes = useGrokHub((s) => s.refreshStaleTimes);
	const resetDemo = useGrokHub((s) => s.resetDemo);
	const grokConnected = useGrokHub((s) => s.grokConnected);
	const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
	const probeGrok = useGrokHub((s) => s.probeGrok);
	const [mobileOpen, setMobileOpen] = (0, import_react.useState)(false);
	const [isDesktop, setIsDesktop] = (0, import_react.useState)(false);
	const modeMeta = getMode(mode);
	(0, import_react.useEffect)(() => {
		const p = useGrokHub.persist.rehydrate();
		Promise.resolve(p).finally(() => {
			useGrokHub.getState().refreshStaleTimes();
			useGrokHub.getState().tickHeartbeat();
			useGrokHub.getState().probeGrok();
		});
		setIsDesktop(Boolean(window.grokhubDesktop));
	}, []);
	(0, import_react.useEffect)(() => {
		const hb = window.setInterval(() => tickHeartbeat(), 3e4);
		return () => window.clearInterval(hb);
	}, [tickHeartbeat]);
	(0, import_react.useEffect)(() => {
		setMobileOpen(false);
	}, [nav]);
	const drag = { WebkitAppRegion: "drag" };
	const noDrag = { WebkitAppRegion: "no-drag" };
	return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
		className: "flex h-dvh max-h-dvh flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]",
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
			className: "mx-auto flex min-h-0 w-full max-w-[1600px] flex-1 overflow-hidden",
			children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("aside", {
				className: "hidden w-56 shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)] bg-[var(--color-surface)] md:flex",
				children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)("nav", {
					className: "scroll-panel flex flex-1 flex-col gap-1 p-3",
					children: NAV.map((item) => {
						const Icon = item.icon;
						return /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("button", {
							type: "button",
							onClick: () => setNav(item.id),
							className: cn("flex h-10 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors", nav === item.id ? "bg-[var(--color-elevated)] text-[var(--color-fg)]" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]"),
							children: [/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Icon, { className: "h-4 w-4 shrink-0" }), item.label]
						}, item.id);
					})
				}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
					className: "shrink-0 space-y-3 border-t border-[var(--color-border)] p-4",
					children: [
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(UsageMeterChip, { className: "w-full" }),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "flex items-center gap-2 text-xs text-[var(--color-muted)]",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("span", { className: "pulse-live inline-block h-2 w-2 rounded-full bg-[var(--color-success)]" }),
								"Heartbeat ",
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)(RelativeTime, { ts: heartbeatAt })
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 py-2",
							children: [
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-[10px] uppercase tracking-wide text-[var(--color-subtle)]",
									children: "Mode"
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-xs font-medium",
									children: modeMeta.label
								}),
								/* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
									className: "text-[10px] text-[var(--color-muted)]",
									children: modeMeta.subtitle
								})
							]
						}),
						/* @__PURE__ */ (0, import_jsx_runtime.jsx)(Button, {
							variant: "secondary",
							size: "sm",
							className: "w-full",
							onClick: () => {
								resetDemo();
								refreshStaleTimes();
							},
							children: "Reset demo"
						})
					]
				})]
			}), /* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
				className: "flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden",
				children: [
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("header", {
						className: "flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-bg)_88%,transparent)] px-4 py-3 backdrop-blur-md md:px-6",
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
										" · ",
										grokConnected ? "live Grok" : "connect in Settings"
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
									children: [modeMeta.label, " · 4.5"]
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
					/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("main", {
						className: "flex min-h-0 flex-1 flex-col overflow-hidden p-4 md:p-6",
						children: [/* @__PURE__ */ (0, import_jsx_runtime.jsxs)("div", {
							className: "scroll-panel min-h-0 flex-1",
							children: [
								nav === "command" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(CommandView, {}),
								nav === "connectors" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ConnectorsView, {}),
								nav === "skills" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SkillsView, {}),
								nav === "automations" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AutomationsView, {}),
								nav === "agents" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(AgentsView, {}),
								nav === "imagine" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ImagineView, {}),
								nav === "desktop" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(DesktopHostView, {}),
								nav === "settings" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)(SettingsView, {})
							]
						}), nav === "chat" && /* @__PURE__ */ (0, import_jsx_runtime.jsx)("div", {
							className: "min-h-0 flex-1 overflow-hidden",
							children: /* @__PURE__ */ (0, import_jsx_runtime.jsx)(ChatView, {})
						})]
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
export { HomePage as component, grokProbe as i, checkUpdate as n, grokChat as r, applyUpdate as t };
