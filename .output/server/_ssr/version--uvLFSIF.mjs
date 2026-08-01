import { r as __exportAll } from "../_runtime.mjs";
import { t as __exportAll$1 } from "./rolldown-runtime-D7D4PA-g.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/version--uvLFSIF.js
var version__uvLFSIF_exports = /* @__PURE__ */ __exportAll({
	a: () => modeBadge,
	c: () => resolveModeWithCatalog,
	d: () => buildCatalog,
	f: () => emptyCatalog,
	h: () => needsGrokClassification,
	i: () => getModesWithCatalog,
	l: () => stripAssistantChrome,
	m: () => models_catalog_exports,
	n: () => autoRouteFor,
	o: () => modelIdForMode,
	p: () => friendlyModelName,
	r: () => getMode,
	s: () => resolveMode,
	t: () => APP_VERSION,
	u: () => applyGrokPlan
});
var models_catalog_exports = /* @__PURE__ */ __exportAll$1({
	ESSENTIAL_NAME_HINTS: () => ESSENTIAL_NAME_HINTS,
	SLOT_CANDIDATES: () => SLOT_CANDIDATES,
	applyGrokPlan: () => applyGrokPlan,
	buildCatalog: () => buildCatalog,
	buildClassifyPrompt: () => buildClassifyPrompt,
	emptyCatalog: () => emptyCatalog,
	filterEssential: () => filterEssential,
	friendlyModelName: () => friendlyModelName,
	modelsSignature: () => modelsSignature,
	needsGrokClassification: () => needsGrokClassification,
	parseGrokSlotPlan: () => parseGrokSlotPlan,
	pickSlotModel: () => pickSlotModel,
	resolveSlotsHeuristic: () => resolveSlotsHeuristic,
	routeAuto: () => routeAuto
});
/** Heuristic preferred API ids per product slot (first match against live list wins). */
var SLOT_CANDIDATES = {
	fast: [
		"grok-4-1-fast-non-reasoning",
		"grok-4-1-fast",
		"grok-4-fast",
		"grok-3-mini-fast",
		"grok-2-latest"
	],
	balanced: [
		"grok-4.3",
		"grok-4",
		"grok-3",
		"grok-2"
	],
	smart: [
		"grok-4.5",
		"grok-4-5",
		"grok-4.5-latest",
		"grok-4.20",
		"grok-4.3",
		"grok-4"
	],
	heavy: [
		"grok-4.5",
		"grok-4-5",
		"grok-4.5-latest",
		"grok-4.3",
		"grok-4"
	],
	build: [
		"grok-build-0.1",
		"grok-code-fast-1",
		"grok-build",
		"build-0.1",
		"grok-code",
		"code-grok-1"
	],
	imagine: [
		"grok-2-image",
		"grok-2-image-1212",
		"grok-imagine-image",
		"grok-2-vision-1212"
	]
};
/** Models we consider product-essential (shown in Settings). */
var ESSENTIAL_NAME_HINTS = [
	/grok-4\.5/i,
	/grok-4-5/i,
	/grok-4\.3/i,
	/grok-4-1-fast/i,
	/grok-4-fast/i,
	/grok-code/i,
	/grok-build/i,
	/build-0\.1/i,
	/grok-2-image/i,
	/imagine/i,
	/^grok-4$/i
];
/** Skip non-product models even if API returns them. */
var SKIP_MODEL_RE = /embed|whisper|tts|audio|moderation|realtime|search|beta-internal|deprecated/i;
var FALLBACK_SLOTS = {
	fast: "grok-4-1-fast-non-reasoning",
	balanced: "grok-4.3",
	smart: "grok-4.5",
	heavy: "grok-4.5",
	build: "grok-build-0.1",
	imagine: "grok-2-image"
};
function normalizeId(id) {
	return id.trim();
}
function modelsSignature(ids) {
	return [...ids].map((s) => s.trim().toLowerCase()).filter(Boolean).sort().join("|");
}
function scoreMatch(live, candidate) {
	const a = live.toLowerCase();
	const b = candidate.toLowerCase();
	if (a === b) return 100;
	if (a.startsWith(b) || b.startsWith(a)) return 80;
	if (a.includes(b) || b.includes(a)) return 60;
	const a2 = a.replace(/[._]/g, "");
	const b2 = b.replace(/[._]/g, "");
	if (a2 === b2) return 90;
	if (a2.includes(b2) || b2.includes(a2)) return 50;
	return 0;
}
/** Pick best live model for a slot from discovered ids (heuristic). */
function pickSlotModel(slot, liveIds) {
	const candidates = SLOT_CANDIDATES[slot];
	let best = FALLBACK_SLOTS[slot];
	let bestScore = 0;
	for (const cand of candidates) for (const live of liveIds) {
		const s = scoreMatch(live, cand);
		if (s > bestScore) {
			bestScore = s;
			best = live;
		}
	}
	if (bestScore === 0) return FALLBACK_SLOTS[slot];
	return best;
}
function filterEssential(liveIds) {
	if (!liveIds.length) return Object.values(FALLBACK_SLOTS).filter((v, i, a) => a.indexOf(v) === i);
	const out = [];
	for (const id of liveIds) {
		if (SKIP_MODEL_RE.test(id)) continue;
		if (ESSENTIAL_NAME_HINTS.some((re) => re.test(id))) out.push(id);
	}
	const slots = resolveSlotsHeuristic(liveIds);
	for (const id of Object.values(slots)) if (id && !out.includes(id) && liveIds.some((l) => l.toLowerCase() === id.toLowerCase())) out.push(id);
	else if (id && !out.includes(id) && liveIds.includes(id)) out.push(id);
	return out.map((id) => liveIds.find((l) => l.toLowerCase() === id.toLowerCase()) || id).filter((v, i, a) => a.findIndex((x) => x.toLowerCase() === v.toLowerCase()) === i).sort((a, b) => a.localeCompare(b));
}
function resolveSlotsHeuristic(liveIds) {
	return {
		fast: pickSlotModel("fast", liveIds),
		balanced: pickSlotModel("balanced", liveIds),
		smart: pickSlotModel("smart", liveIds),
		heavy: pickSlotModel("heavy", liveIds),
		build: pickSlotModel("build", liveIds),
		imagine: pickSlotModel("imagine", liveIds)
	};
}
function buildCatalog(liveIds, prior) {
	const all = liveIds.map(normalizeId).filter(Boolean);
	const sig = modelsSignature(all);
	if (prior && prior.signature === sig && prior.classifiedBy === "grok" && prior.slots) return {
		all,
		essential: prior.essential?.length ? prior.essential : filterEssential(all),
		slots: prior.slots,
		fetchedAt: Date.now(),
		source: all.length ? "live" : "fallback",
		classifiedBy: "grok",
		classifiedAt: prior.classifiedAt || Date.now(),
		signature: sig,
		classifyNotes: prior.classifyNotes
	};
	const slots = resolveSlotsHeuristic(all);
	return {
		all,
		essential: filterEssential(all),
		slots,
		fetchedAt: Date.now(),
		source: all.length ? "live" : "fallback",
		classifiedBy: "heuristic",
		classifiedAt: Date.now(),
		signature: sig
	};
}
function emptyCatalog() {
	return buildCatalog([]);
}
/** Apply a Grok-produced slot plan onto a catalog (validates ids against live list). */
function applyGrokPlan(catalog, plan) {
	const live = catalog.all;
	const liveSet = new Map(live.map((id) => [id.toLowerCase(), id]));
	const pick = (want, fallback) => {
		if (!want) return fallback;
		const hit = liveSet.get(want.toLowerCase());
		if (hit) return hit;
		for (const id of live) if (scoreMatch(id, want) >= 60) return id;
		return fallback;
	};
	const heuristic = resolveSlotsHeuristic(live);
	const slots = {
		fast: pick(plan.slots.fast, heuristic.fast),
		balanced: pick(plan.slots.balanced, heuristic.balanced),
		smart: pick(plan.slots.smart, heuristic.smart),
		heavy: pick(plan.slots.heavy, heuristic.heavy),
		build: pick(plan.slots.build, heuristic.build),
		imagine: pick(plan.slots.imagine, heuristic.imagine)
	};
	let essential = (plan.essential || []).map((id) => liveSet.get(id.toLowerCase()) || id).filter((id) => liveSet.has(id.toLowerCase()) || live.includes(id));
	if (!essential.length) essential = filterEssential(live);
	for (const id of Object.values(slots)) {
		const liveId = liveSet.get(id.toLowerCase()) || id;
		if (!essential.some((e) => e.toLowerCase() === liveId.toLowerCase())) essential.push(liveId);
	}
	essential = essential.filter((v, i, a) => a.findIndex((x) => x.toLowerCase() === v.toLowerCase()) === i).sort((a, b) => a.localeCompare(b));
	return {
		...catalog,
		slots,
		essential,
		classifiedBy: "grok",
		classifiedAt: Date.now(),
		classifyNotes: plan.notes,
		source: live.length ? "live" : catalog.source
	};
}
function needsGrokClassification(catalog, maxAgeMs = 1440 * 60 * 1e3) {
	if (!catalog.all.length) return false;
	if (catalog.classifiedBy !== "grok") return true;
	if (catalog.signature !== modelsSignature(catalog.all)) return true;
	if (Date.now() - (catalog.classifiedAt || 0) > maxAgeMs) return true;
	return false;
}
/** Prompt Grok to classify live model ids into product slots. */
function buildClassifyPrompt(modelIds) {
	return `You classify xAI Grok API model IDs for GrokHub (desktop agent).

Available model IDs (use ONLY these exact strings):
${modelIds.map((m) => `- ${m}`).join("\n")}

Assign the best model for each product slot. Prefer newest generation in that class.
Slots:
- fast: quick low-token chat (non-reasoning / mini / fast variants)
- balanced: solid everyday chat (e.g. 4.3-class)
- smart: hard reasoning / expert (e.g. 4.5-class flagship)
- heavy: multi-angle / max brain (usually same as smart or best available)
- build: long coding sessions / agent coding (code or build models)
- imagine: image generation (if none, use empty string "")

Also list "essential": only product chat/code/image models worth showing in the UI (exclude embeddings, audio, internal).

Return ONLY valid JSON, no markdown:
{"fast":"...","balanced":"...","smart":"...","heavy":"...","build":"...","imagine":"...","essential":["..."],"notes":"one short sentence"}`;
}
/** Parse Grok classifier JSON (tolerates fences / extra text). */
function parseGrokSlotPlan(text, liveIds) {
	if (!text?.trim()) return null;
	let raw = text.trim();
	const fence = raw.match(/```(?:json)?\s*([\s\S]*?)```/i);
	if (fence?.[1]) raw = fence[1].trim();
	const brace = raw.match(/\{[\s\S]*\}/);
	if (brace) raw = brace[0];
	try {
		const j = JSON.parse(raw);
		const get = (k) => typeof j[k] === "string" ? String(j[k]).trim() : "";
		const slots = {
			fast: get("fast"),
			balanced: get("balanced"),
			smart: get("smart"),
			heavy: get("heavy"),
			build: get("build"),
			imagine: get("imagine")
		};
		if (!slots.fast && !slots.balanced && !slots.smart) return null;
		return {
			slots,
			essential: Array.isArray(j.essential) ? j.essential.map(String).filter(Boolean) : filterEssential(liveIds),
			notes: typeof j.notes === "string" ? j.notes : void 0
		};
	} catch {
		return null;
	}
}
var IMAGE_RE = /\b(imagine|image|picture|photo|draw|render|generate\s+(an?\s+)?(image|pic|art|logo|icon|wallpaper)|illustration|visuali[sz]e)\b/i;
var CODE_RE = /\b(code|coding|implement|refactor|typescript|javascript|python|rust|golang|react|component|function|class|bugfix|compile|lint|docker|kubernetes|pkgbuild|aur|api\s+route|pull\s+request|unit\s+test|css|html|sql|bash|shell\s+script|write\s+(me\s+)?(a\s+)?(script|app|site|page|endpoint))\b/i;
var SMART_RE = /\b(architect|design\s+system|trade-?off|tradeoff|root\s+cause|debug|why\s+is|compare|evaluate|critique|security|threat|prove|theorem|math|optimiz|complex|multi-?step|deep\s+dive|analyze\s+carefully|reason\s+about)\b/i;
var RESEARCH_RE = /\b(research|survey|literature|sources?|citations?|summarize\s+(the\s+)?(paper|article|doc)|investigate)\b/i;
var TEAM_RE = /\b(team\s+of|multi-?agent|heavy|debate|red\s*team|from\s+every\s+angle|ops\s+and\s+build|critiques?)\b/i;
var FAST_RE = /\b(hi|hello|hey|thanks|thank\s+you|ping|quick|tl;?dr|eli5|short\s+answer|one\s+line|yes\/no|what\s+time|who\s+are\s+you)\b/i;
/**
* Auto mode router — picks intent + concrete model balancing quality vs tokens.
*/
function routeAuto(prompt, catalog = emptyCatalog()) {
	const p = prompt.trim();
	const words = p.toLowerCase().split(/\s+/).filter(Boolean).length;
	const slots = catalog.slots;
	if (IMAGE_RE.test(p)) return {
		routedMode: "imagine",
		modelId: slots.imagine,
		intent: "image",
		reason: `Image → ${friendlyModelName(slots.imagine)}`,
		openImagine: true
	};
	if (TEAM_RE.test(p) || words > 80 && SMART_RE.test(p)) return {
		routedMode: "heavy",
		modelId: slots.heavy,
		intent: "team",
		reason: `Heavy/team → ${friendlyModelName(slots.heavy)}`
	};
	const codeHit = CODE_RE.test(p);
	if (codeHit && (words > 24 || p.includes("```") || /\b(full|complete|entire|rewrite|migrate|scaffold|implement)\b/i.test(p)) || codeHit && /\b(app|site|package|module|service)\b/i.test(p)) return {
		routedMode: "build",
		modelId: slots.build,
		intent: "code",
		reason: `Coding → ${friendlyModelName(slots.build)}`
	};
	if (codeHit && words <= 24) return {
		routedMode: "fast",
		modelId: slots.fast,
		intent: "chat_fast",
		reason: `Short code Q → ${friendlyModelName(slots.fast)}`
	};
	if (RESEARCH_RE.test(p) || SMART_RE.test(p) || words > 60 || p.length > 400) return {
		routedMode: "expert",
		modelId: slots.smart,
		intent: "chat_smart",
		reason: `Deep reasoning → ${friendlyModelName(slots.smart)}`
	};
	if (words > 28 || /\b(plan|explain|how\s+do\s+i|help\s+me|walk\s+through|step\s+by\s+step)\b/i.test(p)) return {
		routedMode: "expert",
		modelId: slots.balanced,
		intent: "chat_balanced",
		reason: `Medium → ${friendlyModelName(slots.balanced)}`
	};
	if (FAST_RE.test(p) || words <= 12) return {
		routedMode: "fast",
		modelId: slots.fast,
		intent: "chat_fast",
		reason: `Quick chat → ${friendlyModelName(slots.fast)}`
	};
	return {
		routedMode: "expert",
		modelId: slots.balanced,
		intent: "chat_balanced",
		reason: `Default Auto → ${friendlyModelName(slots.balanced)}`
	};
}
function friendlyModelName(id) {
	if (!id) return "—";
	if (/4\.5|4-5/i.test(id)) return "Grok 4.5";
	if (/4\.3/i.test(id)) return "Grok 4.3";
	if (/4-1-fast|4\.1.?fast|4-fast/i.test(id)) return "Grok 4.1 Fast";
	if (/build-0\.1|grok-build/i.test(id)) return "Grok Build 0.1";
	if (/code/i.test(id)) return "Grok Code";
	if (/image|imagine/i.test(id)) return "Imagine";
	if (/^grok-4$/i.test(id)) return "Grok 4";
	return id;
}
/**
* Mode catalog — labels match Grok web; modelId is resolved against live
* essential models (4.5 / 4.3 / 4.1 Fast / Build) when available.
*/
var GROK_MODES = [
	{
		id: "auto",
		label: "Auto",
		subtitle: "Routes Fast · 4.3 · 4.5 · Build · Imagine",
		model: "Auto",
		modelId: "auto",
		icon: "auto",
		latencyMs: [400, 900],
		depth: "standard"
	},
	{
		id: "fast",
		label: "Fast",
		subtitle: "Quick chat · low tokens",
		model: "Grok 4.1 Fast",
		modelId: "grok-4-1-fast-non-reasoning",
		icon: "fast",
		latencyMs: [250, 500],
		depth: "light"
	},
	{
		id: "expert",
		label: "Expert",
		subtitle: "Strong reasoning · Grok 4.5 / 4.3",
		model: "Grok 4.5",
		modelId: "grok-4.5",
		icon: "expert",
		latencyMs: [900, 1600],
		depth: "deep"
	},
	{
		id: "heavy",
		label: "Heavy",
		subtitle: "Team of Experts · max brain",
		model: "Grok 4.5",
		modelId: "grok-4.5",
		icon: "heavy",
		latencyMs: [1400, 2400],
		depth: "team"
	},
	{
		id: "build",
		label: "Build",
		subtitle: "Long coding sessions · Grok Build",
		model: "Grok Build",
		modelId: "grok-code-fast-1",
		icon: "build",
		latencyMs: [700, 1400],
		depth: "code"
	}
];
function getMode(id) {
	return GROK_MODES.find((m) => m.id === id) ?? GROK_MODES[0];
}
/** Modes with live model ids/labels from catalog. */
function getModesWithCatalog(catalog = emptyCatalog()) {
	const s = catalog.slots;
	return GROK_MODES.map((m) => {
		if (m.id === "auto") return {
			...m,
			subtitle: `Router · ${friendlyModelName(s.fast)} / ${friendlyModelName(s.balanced)} / ${friendlyModelName(s.smart)} / ${friendlyModelName(s.build)}`,
			model: "Auto"
		};
		if (m.id === "fast") return {
			...m,
			modelId: s.fast,
			model: friendlyModelName(s.fast),
			subtitle: `Quick chat · ${friendlyModelName(s.fast)}`
		};
		if (m.id === "expert") return {
			...m,
			modelId: s.smart,
			model: friendlyModelName(s.smart),
			subtitle: `Thinks hard · ${friendlyModelName(s.smart)}`
		};
		if (m.id === "heavy") return {
			...m,
			modelId: s.heavy,
			model: friendlyModelName(s.heavy),
			subtitle: `Team of Experts · ${friendlyModelName(s.heavy)}`
		};
		if (m.id === "build") return {
			...m,
			modelId: s.build,
			model: friendlyModelName(s.build),
			subtitle: `Build apps & sites · ${friendlyModelName(s.build)}`
		};
		return m;
	});
}
/**
* Auto routes by intent (kept for mode badge). Concrete model is modelIdForMode.
* Returns a GrokModeId for usage metering / agent assignment.
*/
function resolveMode(id, prompt) {
	if (id !== "auto") return id;
	const r = routeAuto(prompt, emptyCatalog());
	if (r.routedMode === "imagine") return "fast";
	return r.routedMode;
}
/** Resolve mode using live catalog (preferred). */
function resolveModeWithCatalog(id, prompt, catalog) {
	if (id !== "auto") return id;
	const r = routeAuto(prompt, catalog);
	if (r.routedMode === "imagine") return "fast";
	return r.routedMode;
}
/** Resolve the concrete xAI model id for a mode + prompt (+ live catalog). */
function modelIdForMode(id, prompt = "", catalog = emptyCatalog()) {
	const slots = catalog.slots;
	if (id === "auto") return routeAuto(prompt, catalog).modelId;
	if (id === "fast") return slots.fast;
	if (id === "expert") return slots.smart;
	if (id === "heavy") return slots.heavy;
	if (id === "build") return slots.build;
	return slots.balanced;
}
/** Full auto route (for UI status + imagine handoff). */
function autoRouteFor(prompt, catalog = emptyCatalog()) {
	return routeAuto(prompt, catalog);
}
function modeBadge(id, catalog) {
	const modes = catalog ? getModesWithCatalog(catalog) : GROK_MODES;
	const m = modes.find((x) => x.id === id) ?? modes[0];
	return m.id === "auto" ? m.label : `${m.label} · ${m.model}`;
}
/** Strip UI chrome we used to inject into assistant text (keep history clean for the model). */
function stripAssistantChrome(content) {
	return content.replace(/^\[(?:Auto → )?[^\]]+\]\s*\n*/gm, "").replace(/^— Offline fallback —\s*\n*/gm, "").replace(/^Could not reach Grok\.\s*\n*/gm, "").replace(/^Grok connection error:.*$/gm, "").replace(/^Your OAuth session is saved\..*$/gm, "").replace(/^Fix: Settings →.*$/gm, "").replace(/^Not connected to Grok\..*$/gm, "").trim();
}
/** Single source of truth for display / packaging version. */
var APP_VERSION = "0.2.9";
`${APP_VERSION}`;
//#endregion
export { emptyCatalog as a, getModesWithCatalog as c, needsGrokClassification as d, resolveMode as f, version__uvLFSIF_exports as h, buildCatalog as i, modeBadge as l, stripAssistantChrome as m, applyGrokPlan as n, friendlyModelName as o, resolveModeWithCatalog as p, autoRouteFor as r, getMode as s, APP_VERSION as t, modelIdForMode as u };
