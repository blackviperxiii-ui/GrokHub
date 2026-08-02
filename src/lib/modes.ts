import type { GrokMode, GrokModeId } from "./types";
import {
  emptyCatalog,
  friendlyModelName,
  routeAuto,
  type ResolvedCatalog,
  type RouteContext,
  type AutoRouteResult,
} from "./models-catalog";

/**
 * Mode catalog — labels match Grok web; modelId is resolved against live
 * essential models (4.5 / 4.3 / 4.1 Fast / Build) when available.
 */
export const GROK_MODES: GrokMode[] = [
  {
    id: "auto",
    label: "Adaptive",
    subtitle: "Smart router · Fast · Think · Deep · Build",
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
    subtitle: "Team of Experts · max brain",
    model: "Grok 4.5",
    modelId: "grok-4.5",
    icon: "heavy",
    latencyMs: [1400, 2400],
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

/** Modes with live model ids/labels from catalog. */
export function getModesWithCatalog(catalog: ResolvedCatalog = emptyCatalog()): GrokMode[] {
  const s = catalog.slots;
  return GROK_MODES.map((m) => {
    if (m.id === "auto") {
      return {
        ...m,
        label: "Adaptive",
        subtitle: `Routes ⚡ Fast · 🧠 Think · 🔬 Deep · 🛠️ Build`,
        model: "Adaptive",
      };
    }
    if (m.id === "fast") {
      return {
        ...m,
        modelId: s.fast,
        model: friendlyModelName(s.fast),
        subtitle: `⚡ Quick chat · ${friendlyModelName(s.fast)}`,
      };
    }
    if (m.id === "expert") {
      return {
        ...m,
        modelId: s.smart,
        model: friendlyModelName(s.smart),
        subtitle: `🧠 Think hard · ${friendlyModelName(s.smart)}`,
      };
    }
    if (m.id === "heavy") {
      return {
        ...m,
        modelId: s.heavy,
        model: friendlyModelName(s.heavy),
        subtitle: `🔬 Deep / team · ${friendlyModelName(s.heavy)}`,
      };
    }
    if (m.id === "build") {
      return {
        ...m,
        modelId: s.build,
        model: friendlyModelName(s.build),
        subtitle: `🛠️ Build apps · ${friendlyModelName(s.build)}`,
      };
    }
    return m;
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

/** Resolve the concrete xAI model id for a mode + prompt (+ live catalog). */
export function modelIdForMode(
  id: GrokModeId,
  prompt = "",
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
): string {
  const slots = catalog.slots;
  if (id === "auto") {
    return routeAuto(prompt, catalog, ctx).modelId;
  }
  if (id === "fast") return slots.fast;
  if (id === "expert") return slots.smart;
  if (id === "heavy") return slots.heavy;
  if (id === "build") return slots.build;
  return slots.balanced;
}

/** Full adaptive route (for UI status + imagine handoff). */
export function autoRouteFor(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx?: RouteContext,
): AutoRouteResult {
  return routeAuto(prompt, catalog, ctx);
}

/** Map a fixed mode to display tier tags. */
export function tierForMode(id: GrokModeId): AutoRouteResult["tier"] {
  if (id === "fast") return "fast";
  if (id === "build") return "build";
  if (id === "heavy") return "deep";
  if (id === "expert") return "think";
  return "think";
}

export function modeBadge(id: GrokModeId, catalog?: ResolvedCatalog): string {
  const modes = catalog ? getModesWithCatalog(catalog) : GROK_MODES;
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
