/**
 * Essential Grok models + live discovery + Auto router.
 * Slot assignment can be refined by Grok itself when the model list changes.
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
  routedMode: "fast" | "expert" | "heavy" | "build" | "imagine";
  modelId: string;
  intent: RouteIntent;
  reason: string;
  openImagine?: boolean;
};

/** Heuristic preferred API ids per product slot (first match against live list wins). */
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
    "grok-build-0.1",
    "grok-code-fast-1",
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
  /^grok-4$/i,
];

/** Skip non-product models even if API returns them. */
const SKIP_MODEL_RE =
  /embed|whisper|tts|audio|moderation|realtime|search|beta-internal|deprecated/i;

export type ResolvedCatalog = {
  all: string[];
  essential: string[];
  slots: Record<ModelSlot, string>;
  fetchedAt: number;
  source: "live" | "fallback";
  /** How slots were chosen */
  classifiedBy: "heuristic" | "grok";
  classifiedAt: number;
  /** Sorted model-list signature used for classification cache */
  signature: string;
  /** Free-text notes from Grok classifier (optional) */
  classifyNotes?: string;
};

export type GrokSlotPlan = {
  slots: Record<ModelSlot, string>;
  essential: string[];
  notes?: string;
};

const FALLBACK_SLOTS: Record<ModelSlot, string> = {
  fast: "grok-4-1-fast-non-reasoning",
  balanced: "grok-4.3",
  smart: "grok-4.5",
  heavy: "grok-4.5",
  build: "grok-build-0.1",
  imagine: "grok-2-image",
};

const SLOT_KEYS: ModelSlot[] = [
  "fast",
  "balanced",
  "smart",
  "heavy",
  "build",
  "imagine",
];

function normalizeId(id: string): string {
  return id.trim();
}

export function modelsSignature(ids: string[]): string {
  return [...ids]
    .map((s) => s.trim().toLowerCase())
    .filter(Boolean)
    .sort()
    .join("|");
}

function scoreMatch(live: string, candidate: string): number {
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
export function pickSlotModel(slot: ModelSlot, liveIds: string[]): string {
  const candidates = SLOT_CANDIDATES[slot];
  let best = FALLBACK_SLOTS[slot];
  let bestScore = 0;
  for (const cand of candidates) {
    for (const live of liveIds) {
      const s = scoreMatch(live, cand);
      if (s > bestScore) {
        bestScore = s;
        best = live;
      }
    }
  }
  if (bestScore === 0) return FALLBACK_SLOTS[slot];
  return best;
}

export function filterEssential(liveIds: string[]): string[] {
  if (!liveIds.length) {
    return Object.values(FALLBACK_SLOTS).filter((v, i, a) => a.indexOf(v) === i);
  }
  const out: string[] = [];
  for (const id of liveIds) {
    if (SKIP_MODEL_RE.test(id)) continue;
    if (ESSENTIAL_NAME_HINTS.some((re) => re.test(id))) out.push(id);
  }
  const slots = resolveSlotsHeuristic(liveIds);
  for (const id of Object.values(slots)) {
    if (id && !out.includes(id) && liveIds.some((l) => l.toLowerCase() === id.toLowerCase())) {
      out.push(id);
    } else if (id && !out.includes(id) && liveIds.includes(id)) {
      out.push(id);
    }
  }
  // Prefer live casing
  return out
    .map((id) => liveIds.find((l) => l.toLowerCase() === id.toLowerCase()) || id)
    .filter((v, i, a) => a.findIndex((x) => x.toLowerCase() === v.toLowerCase()) === i)
    .sort((a, b) => a.localeCompare(b));
}

export function resolveSlotsHeuristic(liveIds: string[]): Record<ModelSlot, string> {
  return {
    fast: pickSlotModel("fast", liveIds),
    balanced: pickSlotModel("balanced", liveIds),
    smart: pickSlotModel("smart", liveIds),
    heavy: pickSlotModel("heavy", liveIds),
    build: pickSlotModel("build", liveIds),
    imagine: pickSlotModel("imagine", liveIds),
  };
}

export function buildCatalog(
  liveIds: string[],
  prior?: Partial<ResolvedCatalog>,
): ResolvedCatalog {
  const all = liveIds.map(normalizeId).filter(Boolean);
  const sig = modelsSignature(all);
  // Reuse Grok classification if signature unchanged
  if (
    prior &&
    prior.signature === sig &&
    prior.classifiedBy === "grok" &&
    prior.slots
  ) {
    return {
      all,
      essential: prior.essential?.length ? prior.essential : filterEssential(all),
      slots: prior.slots,
      fetchedAt: Date.now(),
      source: all.length ? "live" : "fallback",
      classifiedBy: "grok",
      classifiedAt: prior.classifiedAt || Date.now(),
      signature: sig,
      classifyNotes: prior.classifyNotes,
    };
  }
  const slots = resolveSlotsHeuristic(all);
  return {
    all,
    essential: filterEssential(all),
    slots,
    fetchedAt: Date.now(),
    source: all.length ? "live" : "fallback",
    classifiedBy: "heuristic",
    classifiedAt: Date.now(),
    signature: sig,
  };
}

export function emptyCatalog(): ResolvedCatalog {
  return buildCatalog([]);
}

/** Apply a Grok-produced slot plan onto a catalog (validates ids against live list). */
export function applyGrokPlan(
  catalog: ResolvedCatalog,
  plan: GrokSlotPlan,
): ResolvedCatalog {
  const live = catalog.all;
  const liveSet = new Map(live.map((id) => [id.toLowerCase(), id]));
  const pick = (want: string | undefined, fallback: string): string => {
    if (!want) return fallback;
    const hit = liveSet.get(want.toLowerCase());
    if (hit) return hit;
    // partial match
    for (const id of live) {
      if (scoreMatch(id, want) >= 60) return id;
    }
    return fallback;
  };
  const heuristic = resolveSlotsHeuristic(live);
  const slots: Record<ModelSlot, string> = {
    fast: pick(plan.slots.fast, heuristic.fast),
    balanced: pick(plan.slots.balanced, heuristic.balanced),
    smart: pick(plan.slots.smart, heuristic.smart),
    heavy: pick(plan.slots.heavy, heuristic.heavy),
    build: pick(plan.slots.build, heuristic.build),
    imagine: pick(plan.slots.imagine, heuristic.imagine),
  };

  let essential = (plan.essential || [])
    .map((id) => liveSet.get(id.toLowerCase()) || id)
    .filter((id) => liveSet.has(id.toLowerCase()) || live.includes(id));
  if (!essential.length) essential = filterEssential(live);
  // Always include slot winners
  for (const id of Object.values(slots)) {
    const liveId = liveSet.get(id.toLowerCase()) || id;
    if (!essential.some((e) => e.toLowerCase() === liveId.toLowerCase())) {
      essential.push(liveId);
    }
  }
  essential = essential
    .filter((v, i, a) => a.findIndex((x) => x.toLowerCase() === v.toLowerCase()) === i)
    .sort((a, b) => a.localeCompare(b));

  return {
    ...catalog,
    slots,
    essential,
    classifiedBy: "grok",
    classifiedAt: Date.now(),
    classifyNotes: plan.notes,
    source: live.length ? "live" : catalog.source,
  };
}

export function needsGrokClassification(
  catalog: ResolvedCatalog,
  maxAgeMs = 24 * 60 * 60 * 1000,
): boolean {
  if (!catalog.all.length) return false;
  if (catalog.classifiedBy !== "grok") return true;
  if (catalog.signature !== modelsSignature(catalog.all)) return true;
  if (Date.now() - (catalog.classifiedAt || 0) > maxAgeMs) return true;
  return false;
}

/** Prompt Grok to classify live model ids into product slots. */
export function buildClassifyPrompt(modelIds: string[]): string {
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
export function parseGrokSlotPlan(
  text: string,
  liveIds: string[],
): GrokSlotPlan | null {
  if (!text?.trim()) return null;
  let raw = text.trim();
  const fence = raw.match(/```(?:json)?\s*([\s\S]*?)```/i);
  if (fence?.[1]) raw = fence[1].trim();
  const brace = raw.match(/\{[\s\S]*\}/);
  if (brace) raw = brace[0];
  try {
    const j = JSON.parse(raw) as Record<string, unknown>;
    const get = (k: string) => (typeof j[k] === "string" ? String(j[k]).trim() : "");
    const slots: Record<ModelSlot, string> = {
      fast: get("fast"),
      balanced: get("balanced"),
      smart: get("smart"),
      heavy: get("heavy"),
      build: get("build"),
      imagine: get("imagine"),
    };
    // Require at least fast + smart or balanced
    if (!slots.fast && !slots.balanced && !slots.smart) return null;
    const essential = Array.isArray(j.essential)
      ? (j.essential as unknown[]).map(String).filter(Boolean)
      : filterEssential(liveIds);
    return {
      slots,
      essential,
      notes: typeof j.notes === "string" ? j.notes : undefined,
    };
  } catch {
    return null;
  }
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
      reason: `Image → ${friendlyModelName(slots.imagine)}`,
      openImagine: true,
    };
  }

  if (TEAM_RE.test(p) || (words > 80 && SMART_RE.test(p))) {
    return {
      routedMode: "heavy",
      modelId: slots.heavy,
      intent: "team",
      reason: `Heavy/team → ${friendlyModelName(slots.heavy)}`,
    };
  }

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
      reason: `Coding → ${friendlyModelName(slots.build)}`,
    };
  }

  if (codeHit && words <= 24) {
    return {
      routedMode: "fast",
      modelId: slots.fast,
      intent: "chat_fast",
      reason: `Short code Q → ${friendlyModelName(slots.fast)}`,
    };
  }

  if (RESEARCH_RE.test(p) || SMART_RE.test(p) || words > 60 || p.length > 400) {
    return {
      routedMode: "expert",
      modelId: slots.smart,
      intent: "chat_smart",
      reason: `Deep reasoning → ${friendlyModelName(slots.smart)}`,
    };
  }

  if (
    words > 28 ||
    /\b(plan|explain|how\s+do\s+i|help\s+me|walk\s+through|step\s+by\s+step)\b/i.test(p)
  ) {
    return {
      routedMode: "expert",
      modelId: slots.balanced,
      intent: "chat_balanced",
      reason: `Medium → ${friendlyModelName(slots.balanced)}`,
    };
  }

  if (FAST_RE.test(p) || words <= 12) {
    return {
      routedMode: "fast",
      modelId: slots.fast,
      intent: "chat_fast",
      reason: `Quick chat → ${friendlyModelName(slots.fast)}`,
    };
  }

  return {
    routedMode: "expert",
    modelId: slots.balanced,
    intent: "chat_balanced",
    reason: `Default Auto → ${friendlyModelName(slots.balanced)}`,
  };
}

export function friendlyModelName(id: string): string {
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

export { SLOT_KEYS };
