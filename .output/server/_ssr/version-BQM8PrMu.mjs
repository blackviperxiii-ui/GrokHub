//#region node_modules/.nitro/vite/services/ssr/assets/version-BQM8PrMu.js
/**
* Mode catalog — labels match Grok web; modelId is what we send to api.x.ai.
* SuperGrok / OAuth typically exposes grok-4.3 + fast / code variants.
*/
var GROK_MODES = [
	{
		id: "auto",
		label: "Auto",
		subtitle: "Chooses Fast or Expert",
		model: "Auto",
		modelId: "auto",
		icon: "auto",
		latencyMs: [400, 900],
		depth: "standard"
	},
	{
		id: "fast",
		label: "Fast",
		subtitle: "Quick responses · grok-4-1-fast",
		model: "grok-4-1-fast",
		modelId: "grok-4-1-fast-non-reasoning",
		icon: "fast",
		latencyMs: [250, 500],
		depth: "light"
	},
	{
		id: "expert",
		label: "Expert",
		subtitle: "Thinks hard · grok-4.3",
		model: "grok-4.3",
		modelId: "grok-4.3",
		icon: "expert",
		latencyMs: [900, 1600],
		depth: "deep"
	},
	{
		id: "heavy",
		label: "Heavy",
		subtitle: "Team of Experts · grok-4.3",
		model: "grok-4.3",
		modelId: "grok-4.3",
		icon: "heavy",
		latencyMs: [1400, 2400],
		depth: "team"
	},
	{
		id: "build",
		label: "Build",
		subtitle: "Build apps and sites · grok-code-fast-1",
		model: "grok-code-fast-1",
		modelId: "grok-code-fast-1",
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
	return p.includes("architect") || p.includes("debug") || p.includes("why") || p.includes("compare") || p.includes("research") || p.includes("plan") || p.includes("implement") || p.includes("refactor") || p.includes("design") || p.length > 160 || p.split(/\s+/).length > 28 ? "expert" : "fast";
}
/** Resolve the concrete xAI model id for a mode + prompt. */
function modelIdForMode(id, prompt = "") {
	const routed = resolveMode(id, prompt);
	if (routed === "auto") return getMode("fast").modelId;
	return getMode(routed).modelId;
}
function modeBadge(id) {
	const m = getMode(id);
	return m.id === "auto" ? m.label : `${m.label} · ${m.model}`;
}
/** Strip UI chrome we used to inject into assistant text (keep history clean for the model). */
function stripAssistantChrome(content) {
	return content.replace(/^\[(?:Auto → )?[^\]]+\]\s*\n*/gm, "").replace(/^— Offline fallback —\s*\n*/gm, "").replace(/^Could not reach Grok\.\s*\n*/gm, "").replace(/^Grok connection error:.*$/gm, "").replace(/^Your OAuth session is saved\..*$/gm, "").replace(/^Fix: Settings →.*$/gm, "").replace(/^Not connected to Grok\..*$/gm, "").trim();
}
/** Single source of truth for display / packaging version. */
var APP_VERSION = "0.2.7";
`${APP_VERSION}`;
//#endregion
export { modelIdForMode as a, modeBadge as i, GROK_MODES as n, resolveMode as o, getMode as r, stripAssistantChrome as s, APP_VERSION as t };
