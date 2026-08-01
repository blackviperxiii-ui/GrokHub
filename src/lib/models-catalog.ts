/**
 * Essential Grok models + live discovery + Auto router.
 * Only keeps models that matter for the product surface.
 */

export type ModelSlot = "fast" | "balanced" | "smart" | "build" | "imagine" | "heavy";

export type RouteIntent =
  | "chat_fast"
  | "chat_balanced"
  | "chat_smart"
  | "code"
  | "image"
  | "research"
  | "team";

export type AutoRouteResult = {
  /** Mode badge for UI (never leaves "auto" when user picked Auto — we track routedMode separately) */
  routedMode: "fast" | "expert" | "heavy" | "build" | "imagine";
  modelId: string;
  intent: RouteIntent;
  reason: string;
  /** Prefer opening Imagine tab instead of chat completion */
  openImagine?: boolean;
};

/** Preferred API ids per product slot (first match against live list wins). */
export const SLOT_CANDIDATES: Record<ModelSlot, string[]> = {
  fast: [
    "grok-4-1-fast-non-reasoning",
    "grok-4-1-fast",
    "grok-4-fast",
    "grok-3-mini-fast",
    "grok-2-latest",
  ],
  balanced: ["grok-4.3", "grok-4", "grok-3", "grok-2"],
  smart: [
    "grok-4.5",
    "grok-4-5",
    "grok-4.5-latest",
    "grok-4.20",
    "grok-4.3",
    "grok-4",
  ],
  heavy: [
    "grok-4.5",
    "grok-4-5",
    "grok-4.5-latest",
    "grok-4.3",
    "grok-4",
  ],
  build: [
    "grok-code-fast-1",
    "grok-build-0.1",
    "grok-build",
    "build-0.1",
    "grok-code",
    "code-grok-1",
  ],
  imagine: [
    "grok-2-image",
    "grok-2-image-1212",
    "grok-imagine-image",
    "grok-2-vision-1212",
  ],
};

/** Models we consider product-essential (shown in Settings). */
export const ESSENTIAL_NAME_HINTS = [
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
];

export type ResolvedCatalog = {
  /** All raw ids from xAI (or empty if offline) */
  all: string[];
  /** Essential subset only */
  essential: string[];
  /** Best id per slot */
  slots: Record<ModelSlot, string>;
  fetchedAt: number;
  source: "live" | "fallback";
};

const FALLBACK_SLOTS: Record<ModelSlot, string> = {
  fast: "grok-4-1-fast-non-reasoning",
  balanced: "grok-4.3",
  smart: "grok-4.5",
  heavy: "grok-4.5",
  build: "grok-code-fast-1",
  imagine: "grok-2-image",
};

function normalizeId(id: string): string {
  return id.trim();
}

function scoreMatch(live: string, candidate: string): number {
  const a = live.toLowerCase();
  const b = candidate.toLowerCase();
  if (a === b) return 100;
  if (a.startsWith(b) || b.startsWith(a)) return 80;
  if (a.includes(b) || b.includes(a)) return 60;
  // fuzzy: grok-4.5 vs grok-4-5
  const a2 = a.replace(/[._]/g, "");
  const b2 = b.replace(/[._]/g, "");
  if (a2 === b2) return 90;
  if (a2.includes(b2) || b2.includes(a2)) return 50;
  return 0;
}

/** Pick best live model for a slot from discovered ids. */
export function pickSlotModel(slot: ModelSlot, liveIds: string[]): string {
  const candidates = SLOT_CANDIDATES[slot];
  let best = FALLBACK_SLOTS[slot];
  let bestScore = 0;
  for (const cand of candidates) {
    for (const live of liveIds) {
      const s = scoreMatch(live, cand);
      if (s > bestScore) {
        bestScore = s;
        best = live; // prefer actual live id casing
      }
    }
  }
  // If nothing matched live list, still return preferred fallback id
  if (bestScore === 0 && liveIds.length === 0) return FALLBACK_SLOTS[slot];
  if (bestScore === 0) {
    // try first candidate that looks present-ish or fallback
    return FALLBACK_SLOTS[slot];
  }
  return best;
}

export function filterEssential(liveIds: string[]): string[] {
  if (!liveIds.length) {
    return Object.values(FALLBACK_SLOTS).filter(
      (v, i, a) => a.indexOf(v) === i,
    );
  }
  const out: string[] = [];
  for (const id of liveIds) {
    if (ESSENTIAL_NAME_HINTS.some((re) => re.test(id))) {
      out.push(id);
    }
  }
  // Always ensure slot winners are listed
  const slots = resolveSlots(liveIds);
  for (const id of Object.values(slots)) {
    if (!out.includes(id)) out.push(id);
  }
  return out.sort((a, b) => a.localeCompare(b));
}

export function resolveSlots(liveIds: string[]): Record<ModelSlot, string> {
  return {
    fast: pickSlotModel("fast", liveIds),
    balanced: pickSlotModel("balanced", liveIds),
    smart: pickSlotModel("smart", liveIds),
    heavy: pickSlotModel("heavy", liveIds),
    build: pickSlotModel("build", liveIds),
    imagine: pickSlotModel("imagine", liveIds),
  };
}

export function buildCatalog(liveIds: string[]): ResolvedCatalog {
  const all = liveIds.map(normalizeId).filter(Boolean);
  const slots = resolveSlots(all);
  return {
    all,
    essential: filterEssential(all),
    slots,
    fetchedAt: Date.now(),
    source: all.length ? "live" : "fallback",
  };
}

export function emptyCatalog(): ResolvedCatalog {
  return buildCatalog([]);
}

// ─── Auto router ───────────────────────────────────────────────────────────

const IMAGE_RE =
  /\b(imagine|image|picture|photo|draw|render|generate\s+(an?\s+)?(image|pic|art|logo|icon|wallpaper)|illustration|visuali[sz]e)\b/i;

const CODE_RE =
  /\b(code|coding|implement|refactor|typescript|javascript|python|rust|golang|react|component|function|class|bugfix|compile|lint|docker|kubernetes|pkgbuild|aur|api\s+route|pull\s+request|unit\s+test|css|html|sql|bash|shell\s+script|write\s+(me\s+)?(a\s+)?(script|app|site|page|endpoint))\b/i;

const SMART_RE =
  /\b(architect|design\s+system|trade-?off|tradeoff|root\s+cause|debug|why\s+is|compare|evaluate|critique|security|threat|prove|theorem|math|optimiz|complex|multi-?step|deep\s+dive|analyze\s+carefully|reason\s+about)\b/i;

const RESEARCH_RE =
  /\b(research|survey|literature|sources?|citations?|summarize\s+(the\s+)?(paper|article|doc)|investigate)\b/i;

const TEAM_RE =
  /\b(team\s+of|multi-?agent|heavy|debate|red\s*team|from\s+every\s+angle|ops\s+and\s+build|critiques?)\b/i;

const FAST_RE =
  /\b(hi|hello|hey|thanks|thank\s+you|ping|quick|tl;?dr|eli5|short\s+answer|one\s+line|yes\/no|what\s+time|who\s+are\s+you)\b/i;

/**
 * Auto mode router — picks intent + concrete model balancing quality vs tokens.
 *
 * Rules of thumb:
 * - Image → Imagine pipeline (no chat model burn)
 * - Long coding sessions → Build (code model)
 * - Needs brains (hard reasoning) → 4.5 / smart
 * - Medium analysis → 4.3 / balanced
 * - Quick chat → 4.1 fast
 * - Team / multi-angle → heavy slot (4.5 when available)
 */
export function routeAuto(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
): AutoRouteResult {
  const p = prompt.trim();
  const lower = p.toLowerCase();
  const words = lower.split(/\s+/).filter(Boolean).length;
  const slots = catalog.slots;

  if (IMAGE_RE.test(p)) {
    return {
      routedMode: "imagine",
      modelId: slots.imagine,
      intent: "image",
      reason: "Image generation intent → Imagine",
      openImagine: true,
    };
  }

  if (TEAM_RE.test(p) || (words > 80 && SMART_RE.test(p))) {
    return {
      routedMode: "heavy",
      modelId: slots.heavy,
      intent: "team",
      reason: "Multi-angle / heavy reasoning → smart model",
    };
  }

  // Coding — prefer build for implementation-heavy prompts
  const codeHit = CODE_RE.test(p);
  const longCode =
    codeHit &&
    (words > 24 ||
      p.includes("```") ||
      /\b(full|complete|entire|rewrite|migrate|scaffold|implement)\b/i.test(p));

  if (longCode || (codeHit && /\b(app|site|package|module|service)\b/i.test(p))) {
    return {
      routedMode: "build",
      modelId: slots.build,
      intent: "code",
      reason: "Coding session → Build model",
    };
  }

  if (codeHit && words <= 24) {
    // Small code Q → fast is fine (saves tokens)
    return {
      routedMode: "fast",
      modelId: slots.fast,
      intent: "chat_fast",
      reason: "Short code question → Fast",
    };
  }

  if (RESEARCH_RE.test(p) || SMART_RE.test(p) || words > 60 || p.length > 400) {
    return {
      routedMode: "expert",
      modelId: slots.smart,
      intent: "chat_smart",
      reason: "Deep / research prompt → 4.5-class model",
    };
  }

  // Medium: plan, explain why, compare briefly
  if (
    words > 28 ||
    /\b(plan|explain|how\s+do\s+i|help\s+me|walk\s+through|step\s+by\s+step)\b/i.test(p)
  ) {
    return {
      routedMode: "expert",
      modelId: slots.balanced,
      intent: "chat_balanced",
      reason: "Medium complexity → 4.3-class model",
    };
  }

  if (FAST_RE.test(p) || words <= 12) {
    return {
      routedMode: "fast",
      modelId: slots.fast,
      intent: "chat_fast",
      reason: "Quick chat → Fast (token thrifty)",
    };
  }

  // Default: balanced quality/cost
  return {
    routedMode: "expert",
    modelId: slots.balanced,
    intent: "chat_balanced",
    reason: "Default Auto → balanced model",
  };
}

/** Human label for a model id. */
export function friendlyModelName(id: string): string {
  if (/4\.5|4-5/i.test(id)) return "Grok 4.5";
  if (/4\.3/i.test(id)) return "Grok 4.3";
  if (/4-1-fast|4\.1.?fast|4-fast/i.test(id)) return "Grok 4.1 Fast";
  if (/code|build/i.test(id)) return "Grok Build";
  if (/image|imagine/i.test(id)) return "Imagine";
  if (/^grok-4$/i.test(id)) return "Grok 4";
  return id;
}
