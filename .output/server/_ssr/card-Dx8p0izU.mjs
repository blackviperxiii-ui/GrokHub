import "../_runtime.mjs";
import { C as usagePercent, S as unitsFromTokens, _ as needsGrokClassification, a as buildCatalog, b as resolveModeWithCatalog, c as emptyCatalog, f as getMode, g as modelIdForMode, i as autoRouteFor, l as ensurePeriod, m as inferPlanFromAuth, n as PLAN_LIMITS, o as costFor, r as applyGrokPlan, s as createUsage, x as stripAssistantChrome, y as resolveMode } from "./version-DpPTbU9E.mjs";
import { t as createSeeds } from "./seed-C2Zadwtn.mjs";
import { F as require_react, P as require_jsx_runtime } from "../_libs/@tanstack/react-router+[...].mjs";
import { t as cva } from "../_libs/class-variance-authority+clsx.mjs";
import { r as cn, s as uid } from "./GrokLogo-eJjdnOC_.mjs";
import { n as computeNextRun } from "./automation-schedule-pHDdbmqk.mjs";
import { n as persist, r as create, t as createJSONStorage } from "../_libs/zustand.mjs";
require_react();
var import_jsx_runtime = require_jsx_runtime();
var MIRROR_PREFIX = "grokhub.persist.";
function electronState() {
	return typeof window !== "undefined" ? window.grokhubDesktop?.state : void 0;
}
var persistentStorage = {
	getItem: async (name) => {
		const e = electronState();
		if (e?.get) try {
			const r = await e.get(name);
			if (r?.value != null && r.value !== "") {
				try {
					localStorage.setItem(MIRROR_PREFIX + name, r.value);
				} catch {}
				return r.value;
			}
		} catch {}
		try {
			const direct = localStorage.getItem(MIRROR_PREFIX + name) || localStorage.getItem(name);
			if (direct) return direct;
			if (name === "grokhub-memory-v1") for (const legacy of [
				"grokhub-clean-v4",
				"grokhub-clean-v3",
				"grokhub-clean-v2"
			]) {
				const old = localStorage.getItem(legacy);
				if (old) {
					try {
						localStorage.setItem(MIRROR_PREFIX + name, old);
						if (e?.set) await e.set(name, old);
					} catch {}
					return old;
				}
			}
			return null;
		} catch {
			return null;
		}
	},
	setItem: async (name, value) => {
		try {
			localStorage.setItem(MIRROR_PREFIX + name, value);
			localStorage.setItem(name, value);
		} catch {}
		const e = electronState();
		if (e?.set) try {
			await e.set(name, value);
		} catch {}
	},
	removeItem: async (name) => {
		try {
			localStorage.removeItem(MIRROR_PREFIX + name);
			localStorage.removeItem(name);
		} catch {}
		const e = electronState();
		if (e?.remove) try {
			await e.remove(name);
		} catch {}
	}
};
async function memoryInfo() {
	const e = electronState();
	if (e?.info) try {
		return await e.info();
	} catch {
		return null;
	}
	return {
		path: "browser localStorage",
		userData: "browser"
	};
}
async function exportMemory() {
	const e = electronState();
	if (e?.exportAll) try {
		const r = await e.exportAll();
		return {
			ok: true,
			json: JSON.stringify(r, null, 2)
		};
	} catch (err) {
		return {
			ok: false,
			error: err instanceof Error ? err.message : "export failed"
		};
	}
	try {
		const keys = {};
		for (let i = 0; i < localStorage.length; i++) {
			const k = localStorage.key(i);
			if (!k) continue;
			if (k.startsWith("grokhub") || k.startsWith(MIRROR_PREFIX)) keys[k] = localStorage.getItem(k) || "";
		}
		return {
			ok: true,
			json: JSON.stringify({
				exportedAt: Date.now(),
				keys
			}, null, 2)
		};
	} catch (err) {
		return {
			ok: false,
			error: err instanceof Error ? err.message : "export failed"
		};
	}
}
async function importMemory(json) {
	const e = electronState();
	if (e?.importAll) try {
		return await e.importAll(json);
	} catch (err) {
		return {
			ok: false,
			error: err instanceof Error ? err.message : "import failed"
		};
	}
	try {
		const body = JSON.parse(json);
		const keys = body?.data?.keys || body?.keys || {};
		for (const [k, v] of Object.entries(keys)) {
			localStorage.setItem(String(k), String(v));
			localStorage.setItem(MIRROR_PREFIX + k, String(v));
		}
		return { ok: true };
	} catch (err) {
		return {
			ok: false,
			error: err instanceof Error ? err.message : "import failed"
		};
	}
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
		case "4:3": return {
			w: 900,
			h: 675
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
	const { w, h } = dims(aspect === "auto" ? "1:1" : aspect);
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
/** Website-style prompt templates / tools */
var IMAGINE_PRESETS = [
	{
		id: "photo-edit",
		label: "Photo edit",
		prefix: "Photo edit: "
	},
	{
		id: "restyle",
		label: "Restyle",
		prefix: "Restyle in a bold artistic style: "
	},
	{
		id: "resize",
		label: "Smart resize",
		prefix: "Smart compose for social, subject centered: "
	},
	{
		id: "bg-remove",
		label: "BG remover",
		prefix: "Subject on clean transparent-style studio background: "
	},
	{
		id: "pfp",
		label: "Profile pic",
		prefix: "Square profile picture portrait, clean background: "
	},
	{
		id: "emoji",
		label: "Emoji",
		prefix: "Cute emoji sticker, bold outline, simple shapes: "
	},
	{
		id: "merch",
		label: "Merch",
		prefix: "Merch mockup design, print-ready graphic: "
	},
	{
		id: "cinematic",
		label: "Cinematic",
		prefix: "Cinematic still, anamorphic lens flare, film grain: "
	}
];
var MAX_HITS = 80;
var MAX_TRANSITIONS_PER_KEY = 12;
function emptyQuickAssistMemory() {
	return {
		version: 1,
		hits: [],
		transitions: {},
		lastChipKey: null,
		totalEvents: 0,
		updatedAt: Date.now()
	};
}
function normalizeMemory(raw) {
	const empty = emptyQuickAssistMemory();
	if (!raw || typeof raw !== "object") return empty;
	const m = raw;
	if (m.version !== 1 || !Array.isArray(m.hits)) return empty;
	return {
		version: 1,
		hits: m.hits.filter((h) => h && typeof h.key === "string" && h.value).slice(0, MAX_HITS).map((h) => ({
			key: h.key,
			label: String(h.label || h.value).slice(0, 48),
			value: String(h.value).slice(0, 400),
			kind: [
				"chat",
				"shell",
				"nav",
				"mode"
			].includes(h.kind) ? h.kind : "chat",
			uses: Math.max(0, Number(h.uses) || 0),
			typedUses: Math.max(0, Number(h.typedUses) || 0),
			lastUsedAt: Number(h.lastUsedAt) || 0,
			hourHits: Array.isArray(h.hourHits) ? h.hourHits.slice(0, 24).map((n) => Math.max(0, Number(n) || 0)) : []
		})),
		transitions: m.transitions && typeof m.transitions === "object" ? m.transitions : {},
		lastChipKey: typeof m.lastChipKey === "string" ? m.lastChipKey : null,
		totalEvents: Math.max(0, Number(m.totalEvents) || 0),
		updatedAt: Number(m.updatedAt) || Date.now()
	};
}
function valueKey(value, kind) {
	const v = value.trim().toLowerCase().replace(/\s+/g, " ");
	return `${kind || "x"}:${v.slice(0, 160)}`;
}
function chipMemoryKey(chip) {
	if (chip.id.startsWith("learn-") || chip.id.startsWith("recent-")) return valueKey(chip.value, chip.kind);
	return `id:${chip.id}`;
}
function bumpHour(hourHits, hour) {
	const arr = hourHits.length === 24 ? [...hourHits] : Array.from({ length: 24 }, () => 0);
	arr[hour] = (arr[hour] || 0) + 1;
	return arr;
}
function upsertHit(memory, hit) {
	const hour = (/* @__PURE__ */ new Date()).getHours();
	const existing = memory.hits.find((h) => h.key === hit.key);
	let hits;
	if (existing) hits = memory.hits.map((h) => h.key === hit.key ? {
		...h,
		label: hit.label || h.label,
		value: hit.value || h.value,
		kind: hit.kind || h.kind,
		uses: h.uses + (hit.usesDelta ?? 1),
		typedUses: h.typedUses + (hit.typedDelta ?? 0),
		lastUsedAt: Date.now(),
		hourHits: bumpHour(h.hourHits, hour)
	} : h);
	else hits = [{
		key: hit.key,
		label: hit.label,
		value: hit.value,
		kind: hit.kind,
		uses: hit.usesDelta ?? 1,
		typedUses: hit.typedDelta ?? 0,
		lastUsedAt: Date.now(),
		hourHits: bumpHour([], hour)
	}, ...memory.hits];
	hits.sort((a, b) => {
		const sa = a.uses * 2 + a.typedUses + a.lastUsedAt / 0x9184e72a000;
		return b.uses * 2 + b.typedUses + b.lastUsedAt / 0x9184e72a000 - sa;
	});
	hits = hits.slice(0, MAX_HITS);
	return {
		...memory,
		hits,
		totalEvents: memory.totalEvents + 1,
		updatedAt: Date.now()
	};
}
function recordTransition(memory, fromKey, toKey) {
	if (!fromKey || fromKey === toKey) return {
		...memory,
		lastChipKey: toKey
	};
	const transitions = { ...memory.transitions };
	const row = { ...transitions[fromKey] || {} };
	row[toKey] = (row[toKey] || 0) + 1;
	const entries = Object.entries(row).sort((a, b) => b[1] - a[1]).slice(0, MAX_TRANSITIONS_PER_KEY);
	transitions[fromKey] = Object.fromEntries(entries);
	const keys = Object.keys(transitions);
	if (keys.length > MAX_HITS) for (const k of keys.slice(MAX_HITS)) delete transitions[k];
	return {
		...memory,
		transitions,
		lastChipKey: toKey
	};
}
/** User tapped a quick chip. */
function rememberChipClick(memory, chip) {
	const key = chipMemoryKey(chip);
	let next = upsertHit(memory, {
		key,
		label: chip.label,
		value: chip.value,
		kind: chip.kind,
		usesDelta: 1
	});
	next = recordTransition(next, memory.lastChipKey, key);
	return next;
}
/**
* Free-typed prompt — reinforce similar learned chips and create habits.
* Shell commands and short repeatable prompts become learnable chips.
*/
function rememberTypedPrompt(memory, text) {
	const raw = text.trim();
	if (!raw || raw.length < 2) return memory;
	if (raw.startsWith("[Automation:")) return memory;
	const kind = raw.startsWith("$") || raw.startsWith("/sh ") ? "shell" : "chat";
	if (kind === "chat" && raw.length > 120) return boostMatching(memory, raw);
	const key = valueKey(raw, kind);
	let next = upsertHit(memory, {
		key,
		label: kind === "shell" ? raw.length > 28 ? raw.slice(0, 27) + "…" : raw : raw.length > 32 ? raw.slice(0, 31) + "…" : raw,
		value: raw,
		kind,
		usesDelta: 1,
		typedDelta: 1
	});
	next = recordTransition(next, memory.lastChipKey, key);
	next = boostMatching(next, raw);
	return next;
}
function boostMatching(memory, text) {
	const tokens = text.toLowerCase().split(/\s+/).filter((t) => t.length > 3).slice(0, 8);
	if (!tokens.length) return memory;
	const hits = memory.hits.map((h) => {
		const hay = `${h.label} ${h.value}`.toLowerCase();
		let match = 0;
		for (const tok of tokens) if (hay.includes(tok)) match += 1;
		if (match === 0) return h;
		return {
			...h,
			uses: h.uses + (match >= 2 ? .35 : .15),
			lastUsedAt: h.lastUsedAt
		};
	});
	return {
		...memory,
		hits,
		updatedAt: Date.now()
	};
}
/** Score boost for an existing chip from memory. */
function memoryBoostForChip(memory, chip, now = Date.now()) {
	if (!memory.hits.length) return 0;
	const key = chipMemoryKey(chip);
	const byId = memory.hits.find((h) => h.key === key);
	const byValue = memory.hits.find((h) => h.value.trim().toLowerCase() === chip.value.trim().toLowerCase());
	const hit = byId || byValue;
	let boost = 0;
	if (hit) {
		boost += Math.min(48, Math.log2(1 + hit.uses) * 12);
		boost += Math.min(12, hit.typedUses * 1.5);
		const days = Math.max(0, now - hit.lastUsedAt) / 864e5;
		const recency = Math.max(0, 22 - days * 3);
		boost += recency;
		const hour = new Date(now).getHours();
		if (hit.hourHits[hour] && hit.hourHits[hour] > 0) boost += Math.min(10, hit.hourHits[hour] * 2);
	}
	if (memory.lastChipKey && memory.transitions[memory.lastChipKey]) {
		const tKey = hit?.key || key;
		const n = memory.transitions[memory.lastChipKey][tKey] || 0;
		if (n > 0) boost += Math.min(18, n * 4);
	}
	if (!hit && memory.hits.length) {
		const hay = `${chip.label} ${chip.value}`.toLowerCase();
		for (const h of memory.hits.slice(0, 15)) {
			if (h.uses < 2) continue;
			const other = h.value.toLowerCase();
			if (hay.includes(other.slice(0, 20)) || other.includes(hay.slice(0, 20))) {
				boost += Math.min(10, h.uses);
				break;
			}
		}
	}
	return boost;
}
/** Chips synthesized purely from habits (always available). */
function learnedChipsFromMemory(memory, now = Date.now()) {
	return memory.hits.filter((h) => h.uses >= 1 && h.value.trim().length > 0).filter((h) => h.kind === "chat" || h.kind === "shell").map((h) => {
		const ageDays = (now - h.lastUsedAt) / 864e5;
		const score = 25 + Math.min(45, Math.log2(1 + h.uses) * 10) + Math.max(0, 15 - ageDays * 2) + (h.typedUses > 0 ? 5 : 0);
		return {
			id: `learn-${h.key.slice(0, 40)}`,
			label: h.label,
			value: h.value,
			kind: h.kind,
			score,
			hint: h.uses >= 5 ? "habit" : "learned"
		};
	}).sort((a, b) => b.score - a.score).slice(0, 8);
}
/** Apply memory boosts to a chip list and inject learned favorites. */
function applyMemoryToChips(chips, memory) {
	if (!memory?.hits?.length) return chips;
	const now = Date.now();
	const boosted = chips.map((c) => ({
		...c,
		score: c.score + memoryBoostForChip(memory, c, now)
	}));
	const learned = learnedChipsFromMemory(memory, now);
	const byVal = /* @__PURE__ */ new Map();
	for (const c of [...boosted, ...learned]) {
		const k = c.value.trim().toLowerCase();
		const prev = byVal.get(k);
		if (!prev || c.score > prev.score) byVal.set(k, c);
	}
	return Array.from(byVal.values());
}
/** Waits for user approval of host commands (agent tool loop). */
var hostConfirmWaiter = null;
function requestHostConfirm(set, cmds, risks, botId) {
	return new Promise((resolve) => {
		hostConfirmWaiter = resolve;
		set({
			pendingHostConfirm: {
				cmds,
				risks,
				botId
			},
			streamStatus: "Waiting for host approval…"
		});
	});
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
	imagineAspect: "auto",
	imagineMediaKind: "image",
	imagineQuality: "speed",
	imagineReference: null,
	imagineBusy: false,
	imagineError: null,
	desktop: {
		startMinimized: false,
		launchOnLogin: false,
		wayland: true,
		tray: true,
		confirmHostCommands: true,
		confirmDestructiveOnly: true
	},
	usage: createUsage("pro"),
	heartbeatAt: boot.heartbeatAt,
	running: false,
	streamStatus: null,
	streamingMessageId: null,
	pendingHostConfirm: null,
	quickAssistMemory: emptyQuickAssistMemory(),
	modelCatalog: emptyCatalog(),
	lastModelsFetchAt: 0,
	apiKey: "",
	githubToken: "",
	oauth: null,
	ssoCookie: "",
	openClawWorkspace: null,
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
	resolveHostConfirm: (allow) => {
		const pending = hostConfirmWaiter;
		hostConfirmWaiter = null;
		set({ pendingHostConfirm: null });
		pending?.(allow);
	},
	hydrateSecrets: async () => {
		try {
			const { loadAllSecrets } = await import("./secrets-client-By5XiP9R.mjs");
			const sec = await loadAllSecrets();
			const patch = {};
			if (sec.apiKey) patch.apiKey = sec.apiKey;
			if (sec.githubToken) patch.githubToken = sec.githubToken;
			if (sec.ssoCookie) patch.ssoCookie = sec.ssoCookie;
			if (sec.oauth) try {
				patch.oauth = JSON.parse(sec.oauth);
			} catch {}
			if (Object.keys(patch).length) set(patch);
		} catch {}
	},
	recordQuickAssistChip: (chip) => {
		set((s) => ({ quickAssistMemory: rememberChipClick(s.quickAssistMemory, chip) }));
	},
	recordQuickAssistTyped: (text) => {
		const trimmed = text.trim();
		if (!trimmed) return;
		set((s) => ({ quickAssistMemory: rememberTypedPrompt(s.quickAssistMemory, trimmed) }));
	},
	clearQuickAssistMemory: () => {
		set({ quickAssistMemory: emptyQuickAssistMemory() });
	},
	syncWebsiteConnectors: async () => {
		try {
			const { fetchWebsiteConnectors } = await import("./website-connectors-Bguaf5Ot.mjs");
			const { createSeeds } = await import("./seed-C2Zadwtn.mjs").then((n) => n.n).then((n) => n.n);
			const r = await fetchWebsiteConnectors({
				ssoCookie: get().ssoCookie || void 0,
				bearer: get().oauth?.accessToken
			});
			const catalog = createSeeds().connectors;
			set((s) => {
				const byId = new Map(s.connectors.map((c) => [c.id, c]));
				for (const c of catalog) if (!byId.has(c.id)) byId.set(c.id, c);
				for (const hit of r.connectors) {
					const prev = byId.get(hit.id);
					if (!prev) {
						byId.set(hit.id, {
							id: hit.id,
							name: hit.name,
							category: "Website",
							description: "Linked on Grok website",
							status: "connected",
							tools: [],
							accountLabel: hit.accountLabel,
							source: "website",
							liveTools: hit.id === "github",
							lastUsed: Date.now()
						});
						continue;
					}
					byId.set(hit.id, {
						...prev,
						status: "connected",
						accountLabel: hit.accountLabel || prev.accountLabel,
						source: prev.source === "token" || prev.liveTools ? prev.source : "website",
						liveTools: prev.id === "github" ? Boolean(get().githubToken) || prev.liveTools : prev.id === "desktop-host" || prev.id === "grok-xai" ? true : false,
						lastUsed: Date.now(),
						description: hit.accountLabel ? `${prev.description.split(" · ")[0]} · ${hit.accountLabel}` : prev.description
					});
				}
				if (get().githubToken) {
					const gh = byId.get("github");
					if (gh) byId.set("github", {
						...gh,
						status: "connected",
						liveTools: true,
						source: "token",
						lastUsed: Date.now()
					});
				}
				return { connectors: Array.from(byId.values()) };
			});
			if (r.ok && r.connectors.length) get().pushActivity({
				kind: "connector",
				title: "Website connectors synced",
				detail: r.detail,
				status: "success"
			});
			return {
				ok: r.ok,
				detail: r.detail,
				count: r.connectors.length
			};
		} catch (e) {
			return {
				ok: false,
				detail: e instanceof Error ? e.message : "sync failed",
				count: 0
			};
		}
	},
	setApiKey: (key) => {
		set({
			apiKey: key,
			grokConnected: null
		});
		import("./secrets-client-By5XiP9R.mjs").then((m) => m.secretsSet("apiKey", key));
	},
	setGithubToken: (token) => {
		set({ githubToken: token });
		import("./secrets-client-By5XiP9R.mjs").then((m) => m.secretsSet("githubToken", token));
	},
	setSsoCookie: (cookie) => {
		set({ ssoCookie: cookie.trim() });
		import("./secrets-client-By5XiP9R.mjs").then((m) => m.secretsSet("ssoCookie", cookie.trim()));
		get().refreshUsage();
	},
	startGrokOAuth: async () => {
		const { oauthStart } = await import("./grok-client-DNfOOpUN.mjs");
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
		const { oauthPoll } = await import("./grok-client-DNfOOpUN.mjs");
		const r = await oauthPoll(pending.deviceCode);
		if (r.status === "ready") {
			import("./secrets-client-By5XiP9R.mjs").then((m) => m.secretsSet("oauth", JSON.stringify(r.tokens)));
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
			ssoCookie: "",
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
	linkGrokWebsiteSession: async () => {
		try {
			if (typeof window !== "undefined" && window.grokhubDesktop?.grok?.linkWebsiteSession) {
				const r = await window.grokhubDesktop.grok.linkWebsiteSession();
				if (r?.cookie) {
					set({ ssoCookie: r.cookie });
					await get().refreshUsage();
					get().syncWebsiteConnectors();
					get().pushActivity({
						kind: "auth",
						title: "Grok website linked",
						detail: "Weekly usage will sync from grok.com",
						status: "success"
					});
					return {
						ok: true,
						detail: "Grok website session linked"
					};
				}
				return {
					ok: false,
					detail: r?.error || "No SSO cookie captured. Stay until Grok chat loads, then try again — or paste sso= from browser cookies."
				};
			}
			if (typeof window !== "undefined") window.open("https://grok.com/", "_blank", "noopener,noreferrer");
			return {
				ok: false,
				detail: "Opened grok.com — copy the sso cookie (DevTools → Application → Cookies) and paste it below. Full auto-link works in the Arch desktop app."
			};
		} catch (e) {
			return {
				ok: false,
				detail: e instanceof Error ? e.message : "link failed"
			};
		}
	},
	importOpenClawWorkspace: async (path) => {
		try {
			const { hostReadOpenClawWorkspace } = await import("./host-client-D2zxhrZc.mjs");
			const { mapOpenClawWorkspace } = await import("./openclaw-import-CL8OB3U2.mjs");
			const raw = await hostReadOpenClawWorkspace(path);
			if (!raw?.ok) return {
				ok: false,
				detail: raw?.error || "Could not read OpenClaw workspace"
			};
			const mapped = mapOpenClawWorkspace(raw);
			set((s) => {
				const bySlash = new Map(s.skills.map((sk) => [sk.slash, sk]));
				for (const sk of mapped.skills) bySlash.set(sk.slash, sk);
				const mergedSkills = Array.from(bySlash.values());
				const others = s.agents.filter((a) => !a.id.startsWith("openclaw-"));
				const mergedAgents = [...mapped.agents, ...others];
				const autoNames = new Set(s.automations.map((a) => a.name));
				return {
					skills: mergedSkills,
					agents: mergedAgents,
					automations: [...mapped.automations.filter((a) => !autoNames.has(a.name)), ...s.automations],
					openClawWorkspace: {
						root: mapped.root,
						importedAt: Date.now(),
						filesImported: mapped.filesImported,
						contextBundle: mapped.contextBundle,
						identityName: mapped.identityName
					}
				};
			});
			get().pushActivity({
				kind: "system",
				title: "OpenClaw workspace imported",
				detail: `${mapped.root} · ${mapped.skills.length} skills · ${mapped.filesImported.length} files`,
				status: "success"
			});
			const warn = mapped.warnings.length ? ` · ${mapped.warnings[0]}` : "";
			return {
				ok: true,
				detail: `Imported ${mapped.skills.length} skills, ${mapped.automations.length} automations from ${mapped.root}${warn}`,
				skills: mapped.skills.length,
				automations: mapped.automations.length
			};
		} catch (e) {
			return {
				ok: false,
				detail: e instanceof Error ? e.message : "import failed"
			};
		}
	},
	clearOpenClawWorkspace: () => {
		set((s) => ({
			openClawWorkspace: null,
			agents: s.agents.filter((a) => !a.id.startsWith("openclaw-")),
			skills: s.skills.filter((sk) => !sk.id.startsWith("ocskill")),
			automations: s.automations.filter((a) => !a.name.startsWith("OpenClaw "))
		}));
		get().pushActivity({
			kind: "system",
			title: "OpenClaw workspace cleared",
			detail: "Imported skills/agents/context removed",
			status: "success"
		});
	},
	probeGrok: async () => {
		try {
			const { grokProbe, oauthEnsure } = await import("./grok-client-DNfOOpUN.mjs");
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
	renameThread: (id, title) => {
		const next = title.trim().slice(0, 80);
		if (!next) return;
		set((s) => ({ threads: s.threads.map((t) => t.id === id ? {
			...t,
			title: next,
			updatedAt: Date.now()
		} : t) }));
	},
	setPlan: (plan) => {
		const prev = get().usage;
		set({ usage: createUsage(plan) });
		get().pushActivity({
			kind: "usage",
			title: `Plan → ${PLAN_LIMITS[plan].label}`,
			detail: `Limit ${PLAN_LIMITS[plan].units} units / month (was ${PLAN_LIMITS[prev.plan].label}) · meter reset for plan change`,
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
				byMode,
				lastPolledAt: Date.now(),
				source: "local"
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
	recordTokenUsage: (tokens, mode, rateLimit) => {
		const cost = unitsFromTokens(tokens, mode);
		let ok = true;
		set((s) => {
			const base = ensurePeriod(s.usage);
			const lim = PLAN_LIMITS[base.plan];
			if (base.usedUnits + cost > lim.units * 1.05) ok = false;
			const prompt = tokens.prompt_tokens ?? 0;
			const completion = tokens.completion_tokens ?? Math.max(0, (tokens.total_tokens ?? 0) - prompt);
			const total = tokens.total_tokens ?? prompt + completion;
			const byMode = { ...base.byMode };
			if (mode) byMode[mode] = (byMode[mode] ?? 0) + 1;
			return { usage: {
				...base,
				usedUnits: Math.round((base.usedUnits + cost) * 100) / 100,
				messages: base.messages + 1,
				byMode,
				promptTokens: base.promptTokens + prompt,
				completionTokens: base.completionTokens + completion,
				totalTokens: base.totalTokens + total,
				lastPolledAt: Date.now(),
				source: "live",
				rateLimitRemaining: rateLimit?.remaining ?? base.rateLimitRemaining ?? null,
				rateLimitLimit: rateLimit?.limit ?? base.rateLimitLimit ?? null,
				rateLimitResetAt: rateLimit?.resetAt ?? base.rateLimitResetAt ?? null
			} };
		});
		return {
			ok,
			cost
		};
	},
	refreshUsage: async () => {
		const st = get();
		let usage = ensurePeriod(st.usage);
		const inferred = inferPlanFromAuth({
			hasOauth: Boolean(st.oauth?.accessToken),
			hasApiKey: Boolean(st.apiKey?.trim()),
			email: st.oauth?.email || st.profile?.email,
			name: st.oauth?.name || st.profile?.displayName
		});
		if (usage.plan === "free" && inferred !== "free") usage = {
			...usage,
			plan: inferred
		};
		try {
			const { fetchGrokWebsiteUsage } = await import("./grok-website-usage-CUFrqYHh.mjs").then((n) => n.r).then((n) => n.r);
			let sso = st.ssoCookie?.trim() || "";
			if (!sso && typeof window !== "undefined" && window.grokhubDesktop?.grok?.getWebsiteSso) try {
				const r = await window.grokhubDesktop.grok.getWebsiteSso();
				if (r?.cookie) {
					sso = r.cookie;
					set({ ssoCookie: sso });
				}
			} catch {}
			const web = await fetchGrokWebsiteUsage({
				ssoCookie: sso || null,
				bearer: null
			});
			if (web.ok) {
				const planMap = web.planId === "heavy" || web.planId === "pro" ? "pro" : web.planId === "free" ? "free" : "super";
				const unitCap = PLAN_LIMITS[planMap].units;
				usage = {
					...usage,
					plan: planMap,
					periodStart: web.periodStart || usage.periodStart,
					periodEnd: web.periodEnd || usage.periodEnd,
					usedUnits: Math.round(web.creditUsagePercent / 100 * unitCap * 100) / 100,
					source: "website",
					lastPolledAt: Date.now(),
					website: {
						planLabel: web.planLabel,
						creditUsagePercent: web.creditUsagePercent,
						periodType: web.periodType,
						periodStart: web.periodStart,
						periodEnd: web.periodEnd,
						productUsage: web.productUsage,
						prepaidBalanceCents: web.prepaidBalanceCents,
						onDemandCapCents: web.onDemandCapCents,
						onDemandUsedCents: web.onDemandUsedCents,
						error: null
					}
				};
				set({ usage });
				return;
			} else if (web.error) usage = {
				...usage,
				website: {
					planLabel: usage.website?.planLabel || "—",
					creditUsagePercent: usage.website?.creditUsagePercent ?? 0,
					periodType: usage.website?.periodType || "unknown",
					periodStart: usage.website?.periodStart ?? null,
					periodEnd: usage.website?.periodEnd ?? null,
					productUsage: usage.website?.productUsage || [],
					prepaidBalanceCents: usage.website?.prepaidBalanceCents ?? 0,
					onDemandCapCents: usage.website?.onDemandCapCents ?? 0,
					onDemandUsedCents: usage.website?.onDemandUsedCents ?? 0,
					error: web.error
				}
			};
		} catch {}
		usage = {
			...usage,
			lastPolledAt: Date.now()
		};
		set({ usage });
	},
	resetUsagePeriod: () => {
		const plan = get().usage.plan;
		set({ usage: createUsage(plan) });
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
				const { hostInfo } = await import("./host-client-D2zxhrZc.mjs");
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
					lastUsed: Date.now(),
					liveTools: true,
					source: "token",
					accountLabel: user.login || row.accountLabel
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
		if ((/* @__PURE__ */ new Set([
			"gmail",
			"gdrive",
			"google-calendar",
			"notion",
			"outlook",
			"outlook-calendar",
			"teams",
			"linear",
			"box",
			"canva",
			"stripe",
			"vercel"
		])).has(id) || c.source === "website" && id !== "github") {
			if (!get().ssoCookie) {
				set({ nav: "settings" });
				get().pushActivity({
					kind: "connector",
					title: `Link Grok website for ${c.name}`,
					detail: "Settings → Link Grok website, then Connect again to sync Installed status",
					status: "failed"
				});
				return;
			}
			const synced = await get().syncWebsiteConnectors();
			const row = get().connectors.find((x) => x.id === id);
			if (row?.status === "connected") get().pushActivity({
				kind: "connector",
				title: `${c.name} synced from website`,
				detail: row.accountLabel || synced.detail,
				status: "success"
			});
			else get().pushActivity({
				kind: "connector",
				title: `${c.name} not found on website`,
				detail: "Open grok.com → Skills and Connectors, connect it there, then re-sync (Connect again).",
				status: "failed"
			});
			return;
		}
		const url = { "custom-mcp": "" }[id];
		if (url && typeof window !== "undefined") window.open(url, "_blank", "noopener,noreferrer");
		get().pushActivity({
			kind: "connector",
			title: `${c.name}`,
			detail: "No local connector wiring for this id yet.",
			status: "queued"
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
		if (get().running) {
			get().pushActivity({
				kind: "automation",
				title: `Skipped: ${auto.name}`,
				detail: "Agent is busy — will retry on next schedule tick",
				status: "queued"
			});
			return;
		}
		const routed = resolveMode(get().mode, auto.instructions);
		const m = getMode(routed);
		const bill = get().recordUsage("automation", routed);
		if (!bill.ok) return;
		set({
			running: true,
			streamStatus: `Automation: ${auto.name}`
		});
		get().setAgentStatus("ops", "working", 1);
		get().pushActivity({
			kind: "automation",
			title: `Automation started: ${auto.name}`,
			detail: `${auto.instructions.slice(0, 100)} · ${m.label} · ${bill.cost}u`,
			status: "running"
		});
		let summary = "";
		let ok = true;
		try {
			const { grokChat } = await import("./grok-client-DNfOOpUN.mjs");
			[
				`You are running a scheduled automation named "${auto.name}".`,
				"Follow the instructions. Be concise. If host shell is needed, reply with HOST_CMD lines.",
				"",
				auto.instructions
			].join("\n");
			if (get().oauth?.accessToken || get().apiKey) {
				await get().sendChat(`[Automation: ${auto.name}]\n${auto.instructions}`);
				summary = "Ran via agent chat";
			} else {
				summary = "Not connected to Grok — automation recorded only";
				ok = false;
			}
		} catch (e) {
			ok = false;
			summary = e instanceof Error ? e.message : "automation failed";
		}
		const { computeNextRun } = await import("./automation-schedule-pHDdbmqk.mjs").then((n) => n.t).then((n) => n.t);
		set((s) => ({
			running: false,
			streamStatus: null,
			automations: s.automations.map((a) => a.id === id ? {
				...a,
				lastRun: Date.now(),
				runCount: a.runCount + 1,
				nextRun: a.schedule === "once" ? void 0 : computeNextRun(a.schedule, a.time, Date.now(), Date.now()),
				enabled: a.schedule === "once" ? false : a.enabled
			} : a)
		}));
		get().setAgentStatus("ops", "idle", 0);
		get().pushActivity({
			kind: "automation",
			title: ok ? `Automation completed: ${auto.name}` : `Automation failed: ${auto.name}`,
			detail: summary,
			status: ok ? "success" : "failed"
		});
	},
	tickAutomations: async () => {
		const { dueAutomations, ensureAutomationSchedule } = await import("./automation-schedule-pHDdbmqk.mjs").then((n) => n.t).then((n) => n.t);
		const now = Date.now();
		set((s) => ({ automations: s.automations.map((a) => ensureAutomationSchedule(a, now)) }));
		const due = dueAutomations(get().automations, now);
		for (const a of due.slice(0, 1)) await get().runAutomation(a.id);
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
			nextRun: computeNextRun(input.schedule, input.time, Date.now())
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
		{
			const u = ensurePeriod(get().usage);
			const est = costFor("message", routed);
			if (u.usedUnits + est > PLAN_LIMITS[u.plan].units * 1.02) {
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
						content: `Quota exceeded on ${PLAN_LIMITS[u.plan].label}. Wait for period reset or switch plan in Settings.`,
						ts: Date.now()
					}
				] }));
				return;
			}
		}
		let bill = {
			ok: true,
			cost: costFor("message", routed)
		};
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
		const { extractHostCommands, stripHostCommands, inferHostCommandsFromUser } = await import("./grok-CuRiPbVH.mjs");
		const { extractConnectorCommands, stripConnectorCommands, runConnectorTool } = await import("./connector-tools-C-fkJiLT.mjs");
		const scrubAssistant = (s) => stripConnectorCommands(stripHostCommands(s));
		try {
			if (isLocalSlash) {
				set({ streamStatus: "Running skill…" });
				await wait(280);
				if (abort.signal.aborted || gen !== chatGeneration) aborted = true;
				else {
					bill = get().recordUsage("message", routed);
					finalAnswer = replyFor(trimmed, get(), routed);
					patchBot(finalAnswer, { streaming: false });
				}
			} else {
				const { grokChatStream } = await import("./grok-client-DNfOOpUN.mjs");
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
					const oc = get().openClawWorkspace;
					const result = await grokChatStream({
						messages: history,
						mode: routed,
						model: modelId,
						apiKey: get().apiKey || void 0,
						accessToken: get().oauth?.accessToken,
						tokens: get().oauth,
						workspaceContext: [oc?.contextBundle || "", (await import("./grok-CuRiPbVH.mjs")).connectorContextBlock(get().connectors)].filter(Boolean).join("\n").slice(0, 28e3) || void 0
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
							const scrub = (s) => scrubAssistant(s) || "…";
							if (!globalThis.__ghRaf) globalThis.__ghRaf = requestAnimationFrame(() => {
								globalThis.__ghRaf = 0;
								if (gen !== chatGeneration) return;
								patchBot(scrub(roundText), { streaming: true });
							});
						}
					});
					if (result.tokens) set({ oauth: result.tokens });
					if (result.aborted || abort.signal.aborted || gen !== chatGeneration) {
						aborted = true;
						break;
					}
					if (result.ok && (result.content || roundText)) {
						usedLive = true;
						if (result.usage) bill = get().recordTokenUsage(result.usage, routed, result.rateLimit);
						else if (rounds === 1) bill = get().recordUsage("message", routed);
						const full = stripAssistantChrome(result.content || roundText);
						const visible = scrubAssistant(full);
						accumulated = full;
						patchBot(visible || "Working on your machine…", { streaming: true });
						set({
							grokConnected: true,
							grokStatusDetail: `Live · ${result.model || modelId}`
						});
						let cmds = extractHostCommands(full);
						let connCmds = extractConnectorCommands(full);
						if (!cmds.length && rounds === 1) cmds = inferHostCommandsFromUser(trimmed);
						if (!cmds.length && !connCmds.length) {
							finalAnswer = visible || full;
							break;
						}
						if (connCmds.length) {
							set({ streamStatus: "Running connector tools…" });
							const outputs = [];
							for (const cc of connCmds.slice(0, 3)) {
								if (abort.signal.aborted || gen !== chatGeneration) {
									aborted = true;
									break;
								}
								set({ streamStatus: `Connector: ${cc.connectorId} ${cc.tool}…` });
								const row = get().connectors.find((c) => c.id === cc.connectorId);
								patchBot(`${visible || "Using connector…"}\n\n_Running_\n\`CONNECTOR_CMD: ${cc.connectorId} ${cc.tool}\``, { streaming: true });
								try {
									const r = await runConnectorTool({
										connectorId: cc.connectorId,
										tool: cc.tool,
										args: cc.args,
										githubToken: get().githubToken,
										websiteConnected: row?.status === "connected",
										accountLabel: row?.accountLabel
									});
									outputs.push([
										`CONNECTOR ${cc.connectorId} ${cc.tool}`,
										r.ok ? "ok" : "failed",
										r.detail
									].join("\n"));
									get().pushActivity({
										kind: "connector",
										title: `${cc.connectorId}:${cc.tool}`,
										detail: r.detail.slice(0, 160),
										status: r.ok ? "success" : "failed"
									});
									if (r.ok) set((s) => ({ connectors: s.connectors.map((c) => c.id === cc.connectorId ? {
										...c,
										lastUsed: Date.now()
									} : c) }));
								} catch (e) {
									outputs.push(`CONNECTOR ${cc.connectorId} ${cc.tool}\n[error] ${e instanceof Error ? e.message : "failed"}`);
								}
							}
							if (aborted) break;
							const toolBlock = [
								"CONNECTOR_RESULT (authoritative — use this, do not invent data):",
								outputs.join("\n\n---\n\n"),
								"",
								"Summarize for the user. Only emit another CONNECTOR_CMD if needed."
							].join("\n");
							history.push({
								role: "assistant",
								content: full
							});
							history.push({
								role: "user",
								content: toolBlock
							});
							patchBot([
								visible || "Checked connectors.",
								"",
								"```",
								outputs.join("\n\n"),
								"```",
								"",
								"_Summarizing…_"
							].join("\n"), { streaming: true });
							if (!cmds.length) continue;
						}
						if (!cmds.length) {
							finalAnswer = visible || full;
							break;
						}
						const { classifyHostCommand, needsHostConfirm, riskLabel } = await import("./host-safety-DQsWSdll.mjs");
						const riskList = cmds.slice(0, 3).map((c) => riskLabel(classifyHostCommand(c)));
						const desk = get().desktop;
						if (needsHostConfirm(cmds.slice(0, 3), {
							confirmAll: Boolean(desk.confirmHostCommands) && !desk.confirmDestructiveOnly,
							confirmDestructive: Boolean(desk.confirmHostCommands)
						})) {
							if (!await requestHostConfirm(set, cmds.slice(0, 3), riskList, botId)) {
								finalAnswer = (visible || "") + "\n\n_Host commands cancelled — not run on your machine._";
								patchBot(finalAnswer, { streaming: false });
								break;
							}
						}
						set({ streamStatus: "Running on your desktop…" });
						const { hostExec } = await import("./host-client-D2zxhrZc.mjs");
						const outputs = [];
						for (const cmd of cmds.slice(0, 3)) {
							get().pushActivity({
								kind: "desktop",
								title: "Host command",
								detail: cmd.slice(0, 160),
								status: "running"
							});
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
						"**Could not reach Grok.**",
						"",
						err,
						"",
						hasOauth ? "Your OAuth session is saved. Try: Settings → Disconnect → Connect with Grok OAuth again, or paste an xAI API key as fallback." : "Fix: Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key."
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
	setImagineMediaKind: (v) => set({ imagineMediaKind: v }),
	setImagineQuality: (v) => set({ imagineQuality: v }),
	setImagineReference: (v) => set({ imagineReference: v }),
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
		const mediaKind = get().imagineMediaKind;
		const quality = get().imagineQuality;
		const referenceDataUrl = get().imagineReference || void 0;
		const mode = get().mode;
		const id = uid("img");
		const job = {
			id,
			prompt: p,
			aspect,
			ts: Date.now(),
			status: "rendering",
			mode,
			mediaKind,
			quality,
			referenceDataUrl
		};
		set((s) => ({
			imagineJobs: [job, ...s.imagineJobs].slice(0, 24),
			imagineBusy: true,
			imaginePrompt: p,
			imagineError: null
		}));
		get().pushActivity({
			kind: "imagine",
			title: mediaKind === "video" ? "Imagine video rendering" : "Imagine rendering",
			detail: `${p.slice(0, 80)} · ${aspect} · ${quality} · ${bill.cost}u`,
			status: "running"
		});
		let imageDataUrl;
		let videoDataUrl;
		let source = "local";
		let model;
		let err = null;
		let outKind = mediaKind;
		try {
			const { grokImagine } = await import("./grok-client-DNfOOpUN.mjs");
			const live = await grokImagine({
				prompt: p,
				apiKey: get().apiKey || void 0,
				accessToken: get().oauth?.accessToken,
				tokens: get().oauth,
				aspect,
				quality,
				mediaKind,
				referenceDataUrl
			});
			if (live.ok && (live.imageDataUrl || live.videoDataUrl)) {
				imageDataUrl = live.imageDataUrl;
				videoDataUrl = live.videoDataUrl;
				source = "xai";
				model = live.model;
				if (live.mediaKind === "video" || live.mediaKind === "image") outKind = live.mediaKind;
				if (live.error) err = live.error;
				if (live.tokens) set({ oauth: live.tokens });
			} else err = live.error || "live Imagine unavailable";
		} catch (e) {
			err = e instanceof Error ? e.message : "Imagine request failed";
		}
		if (!imageDataUrl && !videoDataUrl) {
			imageDataUrl = renderImaginePreview(p, aspect === "auto" ? "1:1" : aspect);
			source = "local";
			outKind = "image";
		}
		set((s) => ({
			imagineBusy: false,
			imagineError: source === "local" && err ? err : err && source === "xai" ? err : null,
			imagineJobs: s.imagineJobs.map((j) => j.id === id ? {
				...j,
				status: "ready",
				imageDataUrl,
				videoDataUrl,
				mediaKind: outKind,
				quality,
				model,
				source,
				error: err || void 0
			} : j)
		}));
		get().pushActivity({
			kind: "imagine",
			title: source === "xai" ? outKind === "video" ? "Imagine video ready (Grok)" : "Imagine ready (Grok)" : "Imagine ready (local preview)",
			detail: source === "xai" ? `${p.slice(0, 80)} · ${model || "xAI"} · ${aspect}/${quality}` : `${p.slice(0, 80)}${err ? ` · live failed: ${err}` : " · offline SVG"}`,
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
		set((s) => ({
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
			oauthPending: null,
			ssoCookie: "",
			openClawWorkspace: null,
			quickAssistMemory: emptyQuickAssistMemory(),
			pendingHostConfirm: null
		});
	}
}), {
	name: "grokhub-memory-v1",
	storage: createJSONStorage(() => persistentStorage),
	partialize: (s) => ({
		connectors: s.connectors,
		skills: s.skills,
		automations: s.automations,
		threads: s.threads.slice(0, 80).map((t) => ({
			...t,
			messages: (t.messages || []).slice(-120)
		})),
		activeThreadId: s.activeThreadId,
		agents: s.agents,
		mode: s.mode,
		desktop: s.desktop,
		usage: s.usage,
		imagineJobs: s.imagineJobs.slice(0, 16).map((j) => {
			const { imageDataUrl, videoDataUrl, referenceDataUrl, ...rest } = j;
			const keepImg = imageDataUrl && imageDataUrl.startsWith("data:image/svg") && imageDataUrl.length < 8e4 ? imageDataUrl : void 0;
			return {
				...rest,
				imageDataUrl: keepImg
			};
		}),
		imagineAspect: s.imagineAspect,
		imagineMediaKind: s.imagineMediaKind,
		imagineQuality: s.imagineQuality,
		openClawWorkspace: s.openClawWorkspace ? {
			...s.openClawWorkspace,
			contextBundle: s.openClawWorkspace.contextBundle.slice(0, 8e4)
		} : null,
		profile: s.profile,
		modelCatalog: s.modelCatalog,
		lastModelsFetchAt: s.lastModelsFetchAt,
		chat: s.chat.slice(-200),
		activity: s.activity.slice(0, 100),
		quickAssistMemory: s.quickAssistMemory
	}),
	version: 1,
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
		s.quickAssistMemory = normalizeMemory(s.quickAssistMemory);
		try {
			const cat = createSeeds().connectors;
			const cur = Array.isArray(s.connectors) ? s.connectors : [];
			const byId = new Map(cur.map((c) => [c.id, c]));
			for (const c of cat) if (!byId.has(c.id)) byId.set(c.id, c);
			s.connectors = Array.from(byId.values());
		} catch {}
		if (s.imagineMediaKind !== "image" && s.imagineMediaKind !== "video") s.imagineMediaKind = "image";
		if (s.imagineQuality !== "speed" && s.imagineQuality !== "quality") s.imagineQuality = "speed";
		if (!s.imagineAspect) s.imagineAspect = "auto";
		if (!Array.isArray(s.imagineJobs)) s.imagineJobs = [];
		const desk = s.desktop;
		if (desk) {
			if (desk.confirmHostCommands === void 0) desk.confirmHostCommands = true;
			if (desk.confirmDestructiveOnly === void 0) desk.confirmDestructiveOnly = true;
			s.desktop = desk;
		}
		const u = s.usage;
		if (u) {
			const tokens = Number(u.totalTokens ?? 0);
			const used = Number(u.usedUnits ?? 0);
			if (tokens === 0 && (used === 842 || used === 210 || used === 28 || !("totalTokens" in u))) s.usage = createUsage(u.plan || "pro");
			else s.usage = ensurePeriod(u);
		}
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
//#endregion
export { CardHeader as a, applyMemoryToChips as c, memoryInfo as d, useGrokHub as f, CardDescription as i, exportMemory as l, Card as n, CardTitle as o, CardContent as r, IMAGINE_PRESETS as s, Badge as t, importMemory as u };
