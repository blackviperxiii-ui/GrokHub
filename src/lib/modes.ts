import type { GrokMode, GrokModeId } from "./types";
import {
  emptyCatalog,
  friendlyModelName,
  pickFlagshipModel,
  routeAuto,
  type ResolvedCatalog,
  type RouteContext,
  type AutoRouteResult,
} from "./models-catalog";

/**
 * Mode catalog — labels match Grok web; modelId is resolved against live
 * essential models (4.5 / 4.3 / 4.1 Fast / Build) when available.
 * User overrides (Settings) pin a concrete model id per mode without
 * blocking auto assignment for modes left on "Auto".
 */

/** User pin: mode → model id. Omit / empty = auto from catalog. */
export type ModelModeOverrides = Partial<
  Record<Exclude<GrokModeId, "auto">, string | null | undefined>
>;

export const OVERRIDABLE_MODES: Exclude<GrokModeId, "auto">[] = [
  "fast",
  "balanced",
  "expert",
  "heavy",
  "max",
  "build",
];

export function cleanModelOverrides(
  raw: ModelModeOverrides | null | undefined,
): ModelModeOverrides {
  const out: ModelModeOverrides = {};
  if (!raw || typeof raw !== "object") return out;
  for (const id of OVERRIDABLE_MODES) {
    const v = raw[id];
    if (typeof v === "string" && v.trim()) out[id] = v.trim();
  }
  return out;
}

export function hasModelOverride(
  overrides: ModelModeOverrides | null | undefined,
  mode: GrokModeId,
): boolean {
  if (mode === "auto") return false;
  const v = overrides?.[mode];
  return typeof v === "string" && v.trim().length > 0;
}

/** Prefer user pin when set; otherwise keep auto/catalog model. */
export function applyModelOverride(
  mode: GrokModeId,
  autoModel: string,
  overrides?: ModelModeOverrides | null,
): string {
  if (mode === "auto") return autoModel;
  const o = overrides?.[mode];
  if (typeof o === "string" && o.trim()) return o.trim();
  return autoModel;
}

export const GROK_MODES: GrokMode[] = [
  {
    id: "auto",
    label: "Adaptive",
    subtitle: "Score router · hysteresis · usage-aware",
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
    subtitle: "Everyday chat · Grok 4.3 class",
    model: "Grok 4.3",
    modelId: "grok-4.3",
    icon: "balanced",
    latencyMs: [500, 1000],
    depth: "standard",
  },
  {
    id: "expert",
    label: "Expert",
    subtitle: "Strong reasoning · Grok 4.20",
    model: "Grok 4.20 Reasoning",
    modelId: "grok-4.20-reasoning",
    icon: "expert",
    latencyMs: [900, 1600],
    depth: "deep",
  },
  {
    id: "heavy",
    label: "Heavy",
    subtitle: "Team of Experts · multi-angle",
    model: "Grok 4.5",
    modelId: "grok-4.5",
    icon: "heavy",
    latencyMs: [1400, 2400],
    depth: "team",
  },
  {
    id: "max",
    label: "Max",
    subtitle: "Top-tier flagship · Grok 4.5",
    model: "Grok 4.5",
    modelId: "grok-4.5",
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
  return GROK_MODES.find((m) => m.id === id) ?? GROK_MODES[0]!;
}

/** Auto slot model before user override. */
function autoModelForMode(id: GrokModeId, catalog: ResolvedCatalog): string {
  const s = catalog.slots;
  if (id === "fast") return s.fast;
  if (id === "balanced") return s.balanced;
  if (id === "expert") return s.smart;
  if (id === "heavy") return pickFlagshipModel(catalog.all || []) || s.heavy;
  if (id === "max") return pickFlagshipModel(catalog.all || []) || s.heavy || "grok-4.5";
  if (id === "build") return s.build;
  return s.balanced;
}

/** Modes with live model ids/labels from catalog (+ optional user overrides). */
export function getModesWithCatalog(
  catalog: ResolvedCatalog = emptyCatalog(),
  overrides?: ModelModeOverrides | null,
): GrokMode[] {
  const s = catalog.slots;
  const ov = cleanModelOverrides(overrides);
  return GROK_MODES.map((m) => {
    if (m.id === "auto") {
      return {
        ...m,
        label: "Adaptive",
        subtitle: `⚡ Fast · ⚖️ Balanced · 🧠 Think · 🛠️ Build · 🔬 Deep`,
        model: "Adaptive",
      };
    }
    const autoId = autoModelForMode(m.id, catalog);
    const modelId = applyModelOverride(m.id, autoId, ov);
    const pinned = hasModelOverride(ov, m.id);
    const name = friendlyModelName(modelId);
    if (m.id === "fast") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `⚡ Override · ${name}`
          : `⚡ Quick chat · ${name}`,
      };
    }
    if (m.id === "balanced") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `⚖️ Override · ${name}`
          : `⚖️ Everyday · ${name}`,
      };
    }
    if (m.id === "expert") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `🧠 Override · ${name}`
          : `🧠 Think hard · ${name}`,
      };
    }
    if (m.id === "heavy") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `🔬 Override · ${name}`
          : `🔬 Deep / team · ${name}`,
      };
    }
    if (m.id === "max") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `🚀 Override · ${name}`
          : `🚀 Max · top-tier ${name}`,
      };
    }
    if (m.id === "build") {
      return {
        ...m,
        modelId,
        model: name,
        subtitle: pinned
          ? `🛠️ Override · ${name}`
          : `🛠️ Build apps · ${name}`,
      };
    }
    return { ...m, modelId, model: name };
  });
}

/**
 * Adaptive routes by multi-signal scoring. Concrete model is modelIdForMode.
 */
export function resolveMode(id: GrokModeId, prompt: string, ctx?: RouteContext): GrokModeId {
  if (id !== "auto") return id;
  const r = routeAuto(prompt, emptyCatalog(), ctx);
  if (r.routedMode === "imagine") return "fast";
  return r.routedMode;
}

/** Resolve mode using live catalog (preferred). */
export function resolveModeWithCatalog(
  id: GrokModeId,
  prompt: string,
  catalog: ResolvedCatalog,
  ctx?: RouteContext,
): GrokModeId {
  if (id !== "auto") return id;
  const r = routeAuto(prompt, catalog, ctx);
  if (r.routedMode === "imagine") return "fast";
  return r.routedMode;
}

/** Resolve the concrete xAI model id for a mode + prompt (+ live catalog + overrides). */
export function modelIdForMode(
  id: GrokModeId,
  prompt = "",
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
  overrides?: ModelModeOverrides | null,
): string {
  const ov = cleanModelOverrides(overrides);
  if (id === "auto") {
    const r = routeAuto(prompt, catalog, ctx);
    const routed: GrokModeId =
      r.routedMode === "imagine" ? "fast" : (r.routedMode as GrokModeId);
    return applyModelOverride(routed, r.modelId, ov);
  }
  return applyModelOverride(id, autoModelForMode(id, catalog), ov);
}

/** Full adaptive route (for UI status + imagine handoff). */
export function autoRouteFor(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
  overrides?: ModelModeOverrides | null,
): AutoRouteResult {
  const r = routeAuto(prompt, catalog, ctx);
  const routed: GrokModeId =
    r.routedMode === "imagine" ? "fast" : r.routedMode;
  return {
    ...r,
    modelId: applyModelOverride(routed, r.modelId, cleanModelOverrides(overrides)),
  };
}

/** Map a fixed mode to display tier tags. */
export function tierForMode(id: GrokModeId): AutoRouteResult["tier"] {
  if (id === "fast") return "fast";
  if (id === "balanced") return "balanced";
  if (id === "build") return "build";
  if (id === "heavy" || id === "max") return "deep";
  if (id === "expert") return "think";
  return "think";
}

export function modeBadge(id: GrokModeId, catalog?: ResolvedCatalog, overrides?: ModelModeOverrides | null): string {
  const modes = catalog ? getModesWithCatalog(catalog, overrides) : GROK_MODES;
  const m = modes.find((x) => x.id === id) ?? modes[0]!;
  return m.id === "auto" ? m.label : `${m.label} · ${m.model}`;
}

/** Strip UI chrome we used to inject into assistant text (keep history clean for the model). */
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
