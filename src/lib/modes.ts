import type { GrokMode, GrokModeId } from "./types";
import {
  emptyCatalog,
  friendlyModelName,
  pickFlagshipModel,
  sanitizeChatModel,
  routeAuto,
  type ResolvedCatalog,
  type RouteContext,
  type AutoRouteResult,
} from "./models-catalog";

/**
 * Mode catalog — Adaptive uses a permanent Fast / Balanced / Build / Max map.
 * Think and Heavy were removed; persisted values remap on load.
 */

/** Legacy type kept so old persisted JSON still parses; pins are ignored. */
export type ModelModeOverrides = Partial<
  Record<Exclude<GrokModeId, "auto">, string | null | undefined>
>;

export const OVERRIDABLE_MODES: Exclude<GrokModeId, "auto">[] = [];

/** Retired modes → current permanent modes. */
export function normalizeMode(id: string | null | undefined): GrokModeId {
  const s = String(id || "").trim();
  if (s === "expert" || s === "think") return "balanced";
  if (s === "heavy") return "max";
  if (s === "fast" || s === "balanced" || s === "max" || s === "build" || s === "auto") {
    return s;
  }
  return "auto";
}

export function cleanModelOverrides(
  _raw: ModelModeOverrides | null | undefined,
): ModelModeOverrides {
  // Overrides retired — Adaptive slots are permanent.
  return {};
}

export function hasModelOverride(
  _overrides: ModelModeOverrides | null | undefined,
  _mode: GrokModeId,
): boolean {
  return false;
}

export function applyModelOverride(
  _mode: GrokModeId,
  autoModel: string,
  _overrides?: ModelModeOverrides | null,
): string {
  return autoModel;
}

export const GROK_MODES: GrokMode[] = [
  {
    id: "auto",
    label: "Adaptive",
    subtitle: "Routes Fast · Balanced · Build · Max",
    model: "Adaptive",
    modelId: "auto",
    icon: "auto",
    latencyMs: [400, 900],
    depth: "standard",
  },
  {
    id: "fast",
    label: "Fast",
    subtitle: "Quick chat · low tokens",
    model: "Grok 4.1 Fast",
    modelId: "grok-4-1-fast-non-reasoning",
    icon: "fast",
    latencyMs: [250, 500],
    depth: "light",
  },
  {
    id: "balanced",
    label: "Balanced",
    subtitle: "Everyday chat · Grok 4.3",
    model: "Grok 4.3",
    modelId: "grok-4.3",
    icon: "balanced",
    latencyMs: [500, 1000],
    depth: "standard",
  },
  {
    id: "max",
    label: "Max",
    subtitle: "Top-tier flagship · Grok 4.6",
    model: "Grok 4.6",
    modelId: "grok-4.6",
    icon: "max",
    latencyMs: [1500, 2800],
    depth: "team",
  },
  {
    id: "build",
    label: "Build",
    subtitle: "Long coding sessions · Grok Build",
    model: "Grok Build",
    modelId: "grok-code-fast-1",
    icon: "build",
    latencyMs: [700, 1400],
    depth: "code",
  },
];

export function getMode(id: GrokModeId): GrokMode {
  const nid = normalizeMode(id);
  return GROK_MODES.find((m) => m.id === nid) ?? GROK_MODES[0]!;
}

/** Permanent slot model (live catalog match, no user pin). */
function autoModelForMode(id: GrokModeId, catalog: ResolvedCatalog): string {
  const s = catalog.slots;
  const nid = normalizeMode(id);
  if (nid === "fast") return s.fast;
  if (nid === "balanced") return s.balanced;
  if (nid === "max") return pickFlagshipModel(catalog.all || []) || s.heavy || "grok-4.6";
  if (nid === "build") return s.build;
  return s.balanced;
}

export function getModesWithCatalog(
  catalog: ResolvedCatalog = emptyCatalog(),
  _overrides?: ModelModeOverrides | null,
): GrokMode[] {
  return GROK_MODES.map((m) => {
    if (m.id === "auto") {
      return {
        ...m,
        label: "Adaptive",
        subtitle: "⚡ Fast · ⚖️ Balanced · 🛠️ Build · 🚀 Max (Grok 4.6)",
        model: "Adaptive",
      };
    }
    const modelId = sanitizeChatModel(
      autoModelForMode(m.id, catalog),
      m.id,
      catalog.all || [],
    );
    const name = friendlyModelName(modelId);
    if (m.id === "fast") {
      return { ...m, modelId, model: name, subtitle: `⚡ Quick chat · ${name}` };
    }
    if (m.id === "balanced") {
      return { ...m, modelId, model: name, subtitle: `⚖️ Everyday · ${name}` };
    }
    if (m.id === "max") {
      return { ...m, modelId, model: name, subtitle: `🚀 Max · ${name}` };
    }
    if (m.id === "build") {
      return { ...m, modelId, model: name, subtitle: `🛠️ Build apps · ${name}` };
    }
    return { ...m, modelId, model: name };
  });
}

export function resolveMode(id: GrokModeId, prompt: string, ctx?: RouteContext): GrokModeId {
  const nid = normalizeMode(id);
  if (nid !== "auto") return nid;
  const r = routeAuto(prompt, emptyCatalog(), ctx);
  if (r.routedMode === "imagine") return "fast";
  return normalizeMode(r.routedMode);
}

export function resolveModeWithCatalog(
  id: GrokModeId,
  prompt: string,
  catalog: ResolvedCatalog,
  ctx?: RouteContext,
): GrokModeId {
  const nid = normalizeMode(id);
  if (nid !== "auto") return nid;
  const r = routeAuto(prompt, catalog, ctx);
  if (r.routedMode === "imagine") return "fast";
  return normalizeMode(r.routedMode);
}

export function modelIdForMode(
  id: GrokModeId,
  prompt = "",
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
  _overrides?: ModelModeOverrides | null,
): string {
  const nid = normalizeMode(id);
  if (nid === "auto") {
    const r = routeAuto(prompt, catalog, ctx);
    const routed = normalizeMode(r.routedMode === "imagine" ? "fast" : r.routedMode);
    return sanitizeChatModel(r.modelId, routed, catalog.all || []);
  }
  return sanitizeChatModel(autoModelForMode(nid, catalog), nid, catalog.all || []);
}

export function autoRouteFor(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
  _overrides?: ModelModeOverrides | null,
): AutoRouteResult {
  const r = routeAuto(prompt, catalog, ctx);
  const routed = normalizeMode(r.routedMode === "imagine" ? "fast" : r.routedMode);
  return {
    ...r,
    routedMode:
      routed === "fast" || routed === "balanced" || routed === "max" || routed === "build"
        ? routed
        : "balanced",
    modelId: sanitizeChatModel(r.modelId, routed, catalog.all || []),
  };
}

export function tierForMode(id: GrokModeId): AutoRouteResult["tier"] {
  const nid = normalizeMode(id);
  if (nid === "fast") return "fast";
  if (nid === "balanced") return "balanced";
  if (nid === "build") return "build";
  if (nid === "max") return "deep";
  return "balanced";
}

export function modeBadge(id: GrokModeId, catalog?: ResolvedCatalog): string {
  const modes = catalog ? getModesWithCatalog(catalog) : GROK_MODES;
  const nid = normalizeMode(id);
  const m = modes.find((x) => x.id === nid) ?? modes[0]!;
  return m.id === "auto" ? m.label : `${m.label} · ${m.model}`;
}

export function stripAssistantChrome(content: string): string {
  return content
    .replace(/^\[(?:Auto|Adaptive)[^\]]*\]\s*\n*/gm, "")
    .replace(/^— Offline fallback —\s*\n*/gm, "")
    .replace(/^Could not reach Grok\.\s*\n*/gm, "")
    .replace(/^Grok connection error:.*$/gm, "")
    .replace(/^Your OAuth session is saved\..*$/gm, "")
    .replace(/^Fix: Settings →.*$/gm, "")
    .replace(/^Not connected to Grok\..*$/gm, "")
    .trim();
}
