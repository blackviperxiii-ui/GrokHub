import type { GrokMode, GrokModeId } from "./types";

export const GROK_MODES: GrokMode[] = [
  {
    id: "auto",
    label: "Auto",
    subtitle: "Chooses Fast or Expert",
    model: "Grok 4.5",
    icon: "auto",
    latencyMs: [400, 900],
    depth: "standard",
  },
  {
    id: "fast",
    label: "Fast",
    subtitle: "Quick responses · Grok 4.5",
    model: "Grok 4.5",
    icon: "fast",
    latencyMs: [250, 500],
    depth: "light",
  },
  {
    id: "expert",
    label: "Expert",
    subtitle: "Thinks hard · Grok 4.5",
    model: "Grok 4.5",
    icon: "expert",
    latencyMs: [900, 1600],
    depth: "deep",
  },
  {
    id: "heavy",
    label: "Heavy",
    subtitle: "Team of Experts · Grok 4.5",
    model: "Grok 4.5",
    icon: "heavy",
    latencyMs: [1400, 2400],
    depth: "team",
  },
  {
    id: "build",
    label: "Build",
    subtitle: "Build apps and sites · Grok 4.5",
    model: "Grok 4.5",
    icon: "build",
    latencyMs: [700, 1400],
    depth: "code",
  },
];

export function getMode(id: GrokModeId): GrokMode {
  return GROK_MODES.find((m) => m.id === id) ?? GROK_MODES[0]!;
}

/** Auto routes simple prompts to Fast, deeper ones to Expert. */
export function resolveMode(id: GrokModeId, prompt: string): GrokModeId {
  if (id !== "auto") return id;
  const p = prompt.toLowerCase();
  const heavySignals =
    p.includes("architect") ||
    p.includes("debug") ||
    p.includes("why") ||
    p.includes("compare") ||
    p.includes("research") ||
    p.includes("plan") ||
    p.length > 160 ||
    p.split(/\s+/).length > 28;
  return heavySignals ? "expert" : "fast";
}

export function modeBadge(id: GrokModeId): string {
  const m = getMode(id);
  return `${m.label} · ${m.model}`;
}
