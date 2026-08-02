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

/** User-facing Adaptive tier shown on messages */
export type RouteTier = "fast" | "balanced" | "think" | "deep" | "build" | "imagine";

export type AutoRouteResult = {
  routedMode: "fast" | "balanced" | "expert" | "heavy" | "build" | "imagine";
  modelId: string;
  intent: RouteIntent;
  reason: string;
  /** Short human reason for hover tooltips */
  reasonDetail: string;
  tier: RouteTier;
  /** UI label e.g. ⚡ Fast */
  tierLabel: string;
  openImagine?: boolean;
  /** Internal scores for debugging / UI */
  scores?: Record<string, number>;
};

export type RouteContext = {
  historyTurns?: number;
  recentUserText?: string;
  recentAssistantText?: string;
  hasAttachments?: boolean;
  lastRouteTier?: RouteTier;
  lastRoutedMode?: "fast" | "balanced" | "expert" | "heavy" | "max" | "build" | "imagine";

  /** Soft bias from learning engine: positive = prefer tier */
  learningBias?: Partial<Record<RouteTier, number>>;
  /** 0–1 weekly usage used (website or local estimate) */
  usagePressure?: number;
  /** Prefer cheaper tiers (free Grok fallback) */
  preferFree?: boolean;
  /** Previous assistant turn failed / timed out */
  lastRouteFailed?: boolean;
  /** Recent turns used Desktop Host tools */
  usedHostRecently?: boolean;
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
    "grok-4.20-reasoning",
    "grok-4-20-reasoning",
    "grok-4.20",
    "grok-4-20",
    "grok-4.5",
    "grok-4-5",
    "grok-4.5-latest",
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
  /grok-4\.20/i,
  /grok-4-20/i,
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
  /embed|whisper|tts|audio|moderation|realtime|search|beta-internal|deprecated|multi[-_]?agent|multiagent/i;

/** Models that only work on multi-agent / responses APIs — never chat/completions. */
export function isMultiAgentModel(id: string): boolean {
  const s = String(id || "").toLowerCase();
  if (!s) return false;
  if (/multi[-_]?agents?|multiagents?/.test(s)) return true;
  if (/agent[-_]?team|team[-_]?agent|swarm|orchestrat/.test(s)) return true;
  // e.g. grok-4-agents, grok-agents-preview
  if (/grok/.test(s) && /(?:^|[-_.])agents?(?:$|[-_.])/.test(s)) return true;
  return false;
}

/**
 * Final gate before /chat/completions — never send multi-agent or wrong Max model.
 */
export function sanitizeChatModel(
  model: string,
  mode?: string,
  liveIds: string[] = [],
): string {
  let m = String(model || "").trim();
  const modeId = String(mode || "");
  if (!m) {
    m =
      modeId === "max" || modeId === "heavy"
        ? pickFlagshipModel(liveIds) || "grok-4.5"
        : modeId === "fast"
          ? "grok-4-1-fast-non-reasoning"
          : modeId === "build"
            ? "grok-build-0.1"
            : modeId === "balanced"
              ? "grok-4.3"
              : "grok-4.20-reasoning";
  }
  if (isMultiAgentModel(m)) {
    m =
      modeId === "max" || modeId === "heavy"
        ? pickFlagshipModel(liveIds) || "grok-4.5"
        : modeId === "fast"
          ? "grok-4-1-fast-non-reasoning"
          : modeId === "build"
            ? "grok-build-0.1"
            : "grok-4.20-reasoning";
  }
  // Max/Heavy must not land on 4.20 reasoning
  if ((modeId === "max" || modeId === "heavy") && /4[.-]?20/i.test(m)) {
    m = pickFlagshipModel(liveIds) || "grok-4.5";
  }
  if (modeId === "max") {
    m = pickFlagshipModel(liveIds) || m || "grok-4.5";
  }
  if (isMultiAgentModel(m)) m = "grok-4.5";
  return m;
}

/** Safe models for chat completions from a live list. */
export function filterChatCompletionModels(liveIds: string[]): string[] {
  return (liveIds || []).filter((id) => id && !SKIP_MODEL_RE.test(id) && !isMultiAgentModel(id));
}

/** True top-tier single-agent flagship (Max / Heavy), not reasoning-4.20 or multi-agent. */
export function isFlagshipModel(id: string): boolean {
  const s = String(id || "");
  if (!s || isMultiAgentModel(s)) return false;
  if (/4[.-]?20/i.test(s)) return false;
  return /grok-4[.-]?5/i.test(s);
}

/** Pick Max/flagship from live list — always grok-4.5 class, never multi-agent or 4.20. */
export function pickFlagshipModel(liveIds: string[]): string {
  const live = (liveIds || []).filter(
    (id) => id && !SKIP_MODEL_RE.test(id) && !isMultiAgentModel(id),
  );
  const preferred = ["grok-4.5", "grok-4-5", "grok-4.5-latest", "grok-4-5-latest"];
  for (const want of preferred) {
    const hit = live.find((id) => id.toLowerCase() === want.toLowerCase());
    if (hit) return hit;
  }
  const fourFive = live.find((id) => isFlagshipModel(id));
  if (fourFive) return fourFive;
  const soft = live.find(
    (id) => /grok-4[.-]5/i.test(id) && !/4[.-]20|reasoning|multi/i.test(id),
  );
  if (soft) return soft;
  for (const want of ["grok-4.3", "grok-4", "grok-3"]) {
    const hit = live.find((id) => {
      const low = id.toLowerCase();
      if (isMultiAgentModel(id) || /4[.-]?20/i.test(id)) return false;
      if (low === want) return true;
      if (low.startsWith(want)) {
        const next = low[want.length] || "";
        return !/[0-9]/.test(next);
      }
      return false;
    });
    if (hit) return hit;
  }
  return "grok-4.5";
}

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
  smart: "grok-4.20-reasoning",
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
  if (isMultiAgentModel(a) || isMultiAgentModel(b)) return 0;
  const a2 = a.replace(/[._]/g, "-");
  const b2 = b.replace(/[._]/g, "-");
  if (a2 === b2) return 95;
  const prefixOk = (longer: string, shorter: string) => {
    if (!longer.startsWith(shorter)) return false;
    if (longer.length === shorter.length) return true;
    const next = longer[shorter.length]!;
    if (/[0-9]/.test(next)) return false; // grok-4 vs grok-4.20
    return /[-_.]/.test(next);
  };
  if (prefixOk(a, b) || prefixOk(b, a)) return 80;
  if (prefixOk(a2, b2) || prefixOk(b2, a2)) return 75;
  if (b.length >= 10 && (a.includes(b) || b.includes(a))) return 55;
  return 0;
}

/** Pick best live model for a slot from discovered ids (heuristic). */
export function pickSlotModel(slot: ModelSlot, liveIds: string[]): string {
  const candidates = SLOT_CANDIDATES[slot];
  const usable = (liveIds || []).filter(
    (id) => id && !SKIP_MODEL_RE.test(id) && !isMultiAgentModel(id),
  );
  if (slot === "heavy") {
    return pickFlagshipModel(usable.length ? usable : liveIds);
  }
  let best = FALLBACK_SLOTS[slot];
  let bestScore = 0;
  for (const cand of candidates) {
    for (const live of usable) {
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
  const all = liveIds
    .map(normalizeId)
    .filter(Boolean)
    .filter((id) => !isMultiAgentModel(id));
  const sig = modelsSignature(all);
  // Reuse Grok classification if signature unchanged
  if (
    prior &&
    prior.signature === sig &&
    prior.classifiedBy === "grok" &&
    prior.slots
  ) {
    const slots = { ...prior.slots } as Record<ModelSlot, string>;
    for (const k of SLOT_KEYS) {
      slots[k] = sanitizeChatModel(slots[k], k === "heavy" ? "heavy" : k === "smart" ? "expert" : k, all);
    }
    if (/4[.-]?20/i.test(slots.heavy) || isMultiAgentModel(slots.heavy)) {
      slots.heavy = pickFlagshipModel(all);
    }
    return {
      all,
      essential: prior.essential?.length
        ? prior.essential.filter((id) => !isMultiAgentModel(id))
        : filterEssential(all),
      slots,
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
    if (isMultiAgentModel(want)) return fallback;
    const hit = liveSet.get(want.toLowerCase());
    if (hit && !isMultiAgentModel(hit)) return hit;
    for (const id of live) {
      if (isMultiAgentModel(id)) continue;
      if (scoreMatch(id, want) >= 70) return id;
    }
    return fallback;
  };
  const heuristic = resolveSlotsHeuristic(live);
  const slots: Record<ModelSlot, string> = {
    fast: pick(plan.slots.fast, heuristic.fast),
    balanced: pick(plan.slots.balanced, heuristic.balanced),
    smart: pick(plan.slots.smart, heuristic.smart),
    heavy: pickFlagshipModel(live) || pick(plan.slots.heavy, heuristic.heavy),
    build: pick(plan.slots.build, heuristic.build),
    imagine: pick(plan.slots.imagine, heuristic.imagine),
  };
  for (const k of SLOT_KEYS) {
    if (isMultiAgentModel(slots[k])) {
      slots[k] = heuristic[k] || FALLBACK_SLOTS[k];
    }
  }
  if (isMultiAgentModel(slots.heavy) || /4[.-]?20/i.test(slots.heavy)) {
    slots.heavy = pickFlagshipModel(live);
  }

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
- smart: hard reasoning / Think mode (prefer grok-4.20-reasoning, then 4.5-class flagship)
- heavy: top single-agent flagship for Max/Deep (prefer grok-4.5). NEVER multi-agent model ids (chat completions rejects them).
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

// ─── Adaptive router (score-based + hysteresis) ─────────────────────────────

const IMAGE_RE =
  /\b(imagine|image|picture|photo|draw|render|generate\s+(an?\s+)?(image|pic|art|logo|icon|wallpaper)|illustration|visuali[sz]e|edit\s+(this\s+)?(image|photo|pic))\b/i;

const CODE_RE =
  /\b(code|coding|implement|refactor|typescript|javascript|python|rust|golang|react|component|function|class|bugfix|compile|lint|docker|kubernetes|pkgbuild|aur|api\s+route|pull\s+request|unit\s+test|css|html|sql|bash|shell\s+script|write\s+(me\s+)?(a\s+)?(script|app|site|page|endpoint)|typecheck|stack.?trace|PR\b|merge\s+conflict)\b/i;

const ARCH_RE =
  /\b(architect(?:ure)?|system\s+design|design\s+system|trade-?offs?|scalability|data\s+model|schema|migration\s+plan|service\s+boundary|microservice|event-?driven|end-?to-?end|production\s+ready|multi-?region)\b/i;

const SMART_RE =
  /\b(root\s+cause|debug|why\s+is|compare|evaluate|critique|security|threat|prove|theorem|math|optimiz|complex|multi-?step|deep\s+dive|analyze\s+carefully|reason\s+about|pros?\s+and\s+cons?|step\s+by\s+step)\b/i;

const JUDGMENT_RE =
  /\b(what\s+do\s+you\s+think|how\s+do\s+(you\s+)?(feel|see)|feels?\s+off|doesn'?t\s+feel|something'?s\s+off|seems?\s+(off|wrong|weird)|how\s+(can|do)\s+i\s+improve|improve\s+(this|it|the)|make\s+(this|it)\s+better|thoughts\s+on|your\s+(take|opinion)|review\s+(this|my)|is\s+this\s+(good|ok|right|wrong)|what'?s\s+wrong|why\s+is\s+this|honest\s+feedback|feedback\s+on|rate\s+this|look\s+(right|wrong)|still\s+broken|not\s+working\s+right|help\s+me\s+(decide|choose|pick)|should\s+i)\b/i;

/** Light opinion / short feedback — Balanced, not Think */
const LIGHT_JUDGMENT_RE =
  /^(what\s+do\s+you\s+think|thoughts(\s+on\s+this)?|your\s+take|feedback\??|how\s+does\s+this\s+look|does\s+this\s+look\s+(ok|good|right)|seems?\s+off|feels?\s+off)[.!?\s]*$/i;

const RESEARCH_RE =
  /\b(research|survey|literature|sources?|citations?|summarize\s+(the\s+)?(paper|article|doc)|investigate|thorough|in[\s-]?depth)\b/i;

const TEAM_RE =
  /\b(team\s+of|multi-?agent|heavy|debate|red\s*team|from\s+every\s+angle|ops\s+and\s+build|critiques?)\b/i;

const FAST_RE =
  /^(hi|hello|hey|thanks|thank\s+you|ty|thx|ping|ok|okay|cool|gm|good\s+(morning|night)|lol|sup|yo|k|kk|np|sure|yep|yup|nope|got\s+it|sounds\s+good)[.!?]*$/i;

const CREATIVE_RE =
  /\b(poem|story|joke|brainstorm|rename|tagline|copywriting|marketing\s+blurb|tweet|slogan|creative)\b/i;

const UX_RE =
  /\b(ux|ui|layout|spacing|sidebar|composer|chips|visual hierarchy|accessibility|dark\s+mode|responsive|design\s+polish|badge|toast|banner)\b/i;

const TOOL_RE =
  /\b(\$\s|HOST_CMD|shell|cli|run\s+(this\s+)?command|on\s+my\s+(machine|desktop|system)|list\s+files|read\s+file|edit\s+file|~\/\.config|\/usr\/lib|journalctl|systemctl)\b/i;

const DEBUG_SESSION_RE =
  /\b(bug|broken|error|crash|fix|debug|stack|trace|failing|regression|doesn'?t\s+work|not\s+working)\b/i;

const FOLLOW_UP_RE =
  /^(yes|yeah|yep|yup|sure|please|ok|okay)([,.]?\s+(please|continue|proceed|go\s+(on|ahead)|do\s+(it|that)|try\s+(it|that|again)|fix\s+it))?[.!]*$|^(do\s+it|do\s+that|go\s+ahead|continue|proceed|try\s+(it|that|again)|fix\s+it|same|also|and\s+(also|then)|ok\s+(do|go|try|please)|sounds\s+good|that\s+one|this\s+one|again|more|keep\s+going)[.!]*$/i;

const FOLLOW_UP_SOFT_RE =
  /\b(also|and\s+then|same\s+for|do\s+the\s+same|one\s+more|instead|rather|actually|wait)\b/i;

const HEAVY_CODE_RE =
  /\b(implement|refactor|rewrite|migrate|scaffold|architect|full\s+app|end-?to-?end|production|add\s+tests?)\b/i;

/** Ordered ladder for hysteresis (imagine handled separately). */
export const TIER_LADDER: RouteTier[] = [
  "fast",
  "balanced",
  "think",
  "build",
  "deep",
];

const TIER_RANK: Record<RouteTier, number> = {
  fast: 0,
  balanced: 1,
  think: 2,
  build: 3,
  deep: 4,
  imagine: 1,
};

/** Min score gap to jump more than one ladder step in a single turn */
const HYSTERESIS_JUMP = 18;
/** Soft hold toward last tier */
const STICKY_BONUS = 10;

export function tierMeta(tier: RouteTier): {
  label: string;
  emoji: string;
  short: string;
  tone: string;
} {
  if (tier === "fast")
    return {
      label: "⚡ Fast",
      emoji: "⚡",
      short: "Fast",
      tone: "border-[color-mix(in_oklab,var(--color-success)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-success)_14%,transparent)] text-[var(--color-success)]",
    };
  if (tier === "balanced")
    return {
      label: "⚖️ Balanced",
      emoji: "⚖️",
      short: "Balanced",
      tone: "border-[color-mix(in_oklab,var(--color-muted)_50%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-elevated)_80%,transparent)] text-[var(--color-fg)]",
    };
  if (tier === "think")
    return {
      label: "🧠 Think",
      emoji: "🧠",
      short: "Think",
      tone: "border-[color-mix(in_oklab,var(--color-info)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-info)_14%,transparent)] text-[var(--color-info)]",
    };
  if (tier === "deep")
    return {
      label: "🔬 Deep",
      emoji: "🔬",
      short: "Deep",
      tone: "border-[color-mix(in_oklab,var(--color-accent)_50%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-accent)_16%,transparent)] text-[var(--color-accent)]",
    };
  if (tier === "build")
    return {
      label: "🛠️ Build",
      emoji: "🛠️",
      short: "Build",
      tone: "border-[color-mix(in_oklab,var(--color-warn)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_14%,transparent)] text-[var(--color-warn)]",
    };
  return {
    label: "🎨 Imagine",
    emoji: "🎨",
    short: "Imagine",
    tone: "border-[color-mix(in_oklab,#c084fc_45%,var(--color-border))] bg-[color-mix(in_oklab,#c084fc_14%,transparent)] text-[#c084fc]",
  };
}

type PromptSignals = {
  words: number;
  complexity: number;
  analytical: number;
  code: number;
  creative: number;
  simple: number;
  codeHit: boolean;
  archHit: boolean;
  smartHit: boolean;
  judgmentHit: boolean;
  lightJudgment: boolean;
  researchHit: boolean;
  teamHit: boolean;
  pureFast: boolean;
  followUp: boolean;
  creativeHit: boolean;
  uxHit: boolean;
  toolHit: boolean;
  debugHit: boolean;
  hasCodeFence: boolean;
  imageHit: boolean;
  heavyCode: boolean;
  stickyThink: boolean;
  stickyDeep: boolean;
  longReplyContext: boolean;
  codingSession: boolean;
  debugSession: boolean;
  chatSession: boolean;
};

function scorePrompt(prompt: string, ctx: RouteContext = {}): PromptSignals {
  const p = prompt.trim();
  const lower = p.toLowerCase();
  const words = lower.split(/\s+/).filter(Boolean).length;
  const hist = ctx.historyTurns ?? 0;
  const recent = `${ctx.recentUserText || ""}\n${ctx.recentAssistantText || ""}`;
  const blob = `${p}\n${recent}`;

  const hasCodeFence = /```[\s\S]{12,}/.test(blob) || /```[\s\S]{12,}/.test(p);
  const codeHit = CODE_RE.test(p) || CODE_RE.test(recent) || hasCodeFence;
  const archHit = ARCH_RE.test(p) || ARCH_RE.test(recent);
  const smartHit = SMART_RE.test(p);
  const judgmentHit = JUDGMENT_RE.test(p);
  const lightJudgment = LIGHT_JUDGMENT_RE.test(p.trim()) || (judgmentHit && words <= 8);
  const researchHit = RESEARCH_RE.test(p);
  const teamHit = TEAM_RE.test(p);
  const pureFast = FAST_RE.test(p.trim());
  const followUp =
    FOLLOW_UP_RE.test(p.trim()) ||
    (words <= 12 && FOLLOW_UP_SOFT_RE.test(p) && Boolean(ctx.lastRouteTier));
  const creativeHit = CREATIVE_RE.test(p);
  const uxHit = UX_RE.test(p);
  const toolHit =
    TOOL_RE.test(p) ||
    Boolean(ctx.hasAttachments) ||
    Boolean(ctx.usedHostRecently) ||
    TOOL_RE.test(recent);
  const debugHit = DEBUG_SESSION_RE.test(p) || DEBUG_SESSION_RE.test(recent);
  const heavyCode = HEAVY_CODE_RE.test(p);
  const longReplyContext = (ctx.recentAssistantText || "").length > 800;
  const stickyDeep =
    ctx.lastRouteTier === "deep" ||
    ctx.lastRouteTier === "build" ||
    ctx.lastRoutedMode === "heavy" ||
    ctx.lastRoutedMode === "build";
  const stickyThink =
    stickyDeep ||
    ctx.lastRouteTier === "think" ||
    ctx.lastRouteTier === "balanced" ||
    ctx.lastRoutedMode === "expert" ||
    ctx.lastRoutedMode === "balanced";

  // Session roles from recent turns
  const codingSession =
    (codeHit && hist >= 2) ||
    /\b(HOST_CMD|```|typecheck|npm |git )/i.test(recent);
  const debugSession =
    debugHit && (hist >= 1 || /error|fail|stack|broken/i.test(recent));
  const chatSession = !codingSession && !debugSession && hist >= 1;

  let complexity = 0;
  if (words <= 5) complexity += 4;
  else if (words <= 12) complexity += 16;
  else if (words <= 24) complexity += 28;
  else if (words <= 50) complexity += 42;
  else if (words <= 90) complexity += 58;
  else complexity += 78;
  if (p.length > 400) complexity += 10;
  if (p.length > 900) complexity += 12;
  if (/\?/.test(p) && words > 8) complexity += 10;
  if (/(1\)|2\)|3\)|first,|second,|then )/i.test(p)) complexity += 12;
  if (hist >= 4) complexity += 6;
  if (hist >= 10) complexity += 8;
  if (longReplyContext) complexity += 12;
  if (stickyThink) complexity += 6;
  if (debugHit) complexity += 14;

  let analytical = 0;
  if (judgmentHit && !lightJudgment) analytical += 36;
  if (lightJudgment) analytical += 12;
  if (smartHit) analytical += 32;
  if (archHit) analytical += 38;
  if (researchHit) analytical += 36;
  if (uxHit) analytical += 18;
  if (debugHit) analytical += 24;
  if (/\bwhy\b|\bhow\s+should\b|\btrade|\bfeel|\bthink\b/.test(lower)) analytical += 14;
  if (/\bcompare\b|\bvs\.?\b|\bdifference\b|\bshould\s+i\b/.test(lower)) analytical += 18;
  if (/\bimprove\b|\bbetter\b|\bpolish\b|\breview\b/.test(lower)) analytical += 16;
  if (stickyThink && words >= 3 && !pureFast) analytical += 10;

  let code = 0;
  if (codeHit) code += 38;
  if (hasCodeFence) code += 28;
  if (heavyCode) code += 26;
  if (/\b(one-?liner|snippet|regex|rename\s+var)\b/i.test(p)) code -= 12;
  if (toolHit && codeHit) code += 12;
  if (debugHit && codeHit) code += 12;
  if (codingSession) code += 14;

  let creative = 0;
  if (creativeHit) creative += 40;
  if (/\bstory\b|\bjoke\b|\bpoem\b/.test(lower)) creative += 20;

  let simple = 0;
  if (pureFast) simple += 70;
  if (words <= 3 && !judgmentHit && !codeHit && !smartHit && !uxHit) simple += 30;
  if (/^(yes|no|ok|thanks|thank you|continue|go on)[.!]?$/i.test(p.trim())) simple += 45;
  if (judgmentHit && !lightJudgment) simple = 0;

  const bias = ctx.learningBias || {};
  if (bias.fast) simple += bias.fast * 40;
  if (bias.balanced) complexity += (bias.balanced || 0) * 12;
  if (bias.think) analytical += (bias.think || 0) * 35;
  if (bias.deep) {
    analytical += bias.deep * 25;
    complexity += bias.deep * 20;
  }
  if (bias.build) code += bias.build * 40;

  const clamp = (n: number) => Math.max(0, Math.min(100, n));
  return {
    words,
    complexity: clamp(complexity),
    analytical: clamp(analytical),
    code: clamp(code),
    creative: clamp(creative),
    simple: clamp(simple),
    codeHit,
    archHit,
    smartHit,
    judgmentHit,
    lightJudgment,
    researchHit,
    teamHit,
    pureFast,
    followUp,
    creativeHit,
    uxHit,
    toolHit,
    debugHit,
    hasCodeFence,
    imageHit: IMAGE_RE.test(p) && !codeHit,
    heavyCode,
    stickyThink,
    stickyDeep,
    longReplyContext,
    codingSession,
    debugSession,
    chatSession,
  };
}

/** Per-tier raw scores before hysteresis / usage pressure. */
export function scoreTiers(
  s: PromptSignals,
  ctx: RouteContext = {},
): Record<RouteTier, number> {
  const scores: Record<RouteTier, number> = {
    fast: 8,
    balanced: 18,
    think: 6,
    build: 4,
    deep: 2,
    imagine: 0,
  };

  // Fast
  scores.fast += s.simple * 0.9;
  if (s.pureFast) scores.fast += 40;
  if (s.words <= 4 && !s.judgmentHit && !s.codeHit) scores.fast += 22;
  if (s.creativeHit && s.complexity < 30) scores.fast += 18;
  if (s.words <= 10 && !s.stickyThink && !s.debugHit && !s.judgmentHit) scores.fast += 12;
  if (s.codeHit && s.words <= 10 && !s.hasCodeFence && !s.heavyCode && !s.debugHit) {
    scores.fast += 16;
  }

  // Balanced — everyday default
  scores.balanced += 22;
  scores.balanced += Math.min(30, s.complexity * 0.35);
  if (s.uxHit) scores.balanced += 22;
  if (s.lightJudgment) scores.balanced += 28;
  if (s.judgmentHit && !s.lightJudgment && s.words <= 14) scores.balanced += 10;
  if (s.chatSession) scores.balanced += 10;
  if (s.words > 8 && s.words <= 40 && !s.heavyCode && !s.archHit) scores.balanced += 14;
  if (s.creativeHit && s.complexity >= 20) scores.balanced += 8;

  // Think — real reasoning (cap so Deep can win on architecture)
  scores.think += Math.min(40, s.analytical * 0.45);
  if (s.smartHit) scores.think += 18;
  if (s.judgmentHit && !s.lightJudgment) scores.think += 22;
  if (s.debugHit && !s.codingSession) scores.think += 16;
  if (s.analytical >= 34 || s.complexity >= 48) scores.think += 12;
  if (s.archHit && s.words < 12) scores.think += 10; // short arch note → think

  // Build — code / host
  scores.build += s.code * 0.7;
  if (s.hasCodeFence) scores.build += 20;
  if (s.heavyCode) scores.build += 32;
  if (s.codingSession) scores.build += 16;
  if (s.toolHit && s.codeHit) scores.build += 18;
  if (s.toolHit && s.debugHit) scores.build += 12;
  if (s.codeHit && (s.words > 20 || s.complexity >= 38)) scores.build += 14;

  // Deep — hard / multi-angle / architecture
  scores.deep += Math.min(48, (s.analytical + s.complexity) * 0.32);
  if (s.teamHit) scores.deep += 40;
  if (s.researchHit) scores.deep += 34;
  if (s.archHit) scores.deep += s.words >= 12 ? 42 : 18;
  if (s.debugHit && s.complexity >= 32 && s.analytical >= 22) scores.deep += 26;
  if (s.words > 55 || s.complexity >= 55) scores.deep += 20;
  if (s.debugSession && s.toolHit) scores.deep += 16;
  if ((ctx.historyTurns || 0) >= 8 && s.debugHit) scores.deep += 12;
  // multi-region / trade-off language is a Deep signal even mid-length
  if (s.archHit && s.analytical >= 30) scores.deep += 16;

  // Imagine
  if (s.imageHit) scores.imagine += 80;
  if (ctx.hasAttachments && s.imageHit) scores.imagine += 15;
  if (ctx.hasAttachments && s.codeHit) scores.build += 10;

  // Learning bias (direct on tiers)
  const bias = ctx.learningBias || {};
  for (const tier of Object.keys(scores) as RouteTier[]) {
    if (bias[tier]) scores[tier] += bias[tier]! * 25;
  }

  // Last-route failure → demote previous, boost one step down
  if (ctx.lastRouteFailed && ctx.lastRouteTier && ctx.lastRouteTier !== "imagine") {
    scores[ctx.lastRouteTier] -= 20;
    const r = TIER_RANK[ctx.lastRouteTier];
    if (r > 0) {
      const lower = TIER_LADDER[r - 1]!;
      scores[lower] += 14;
    }
  }

  // Usage pressure: prefer cheaper tiers when weekly usage high or free-prefer
  const pressure = Math.max(
    0,
    Math.min(1, ctx.usagePressure ?? 0),
  );
  if (pressure > 0.55 || ctx.preferFree) {
    const scale = ctx.preferFree ? 1 : (pressure - 0.55) / 0.45;
    scores.deep -= 18 * scale + (ctx.preferFree ? 12 : 0);
    scores.think -= 10 * scale + (ctx.preferFree ? 6 : 0);
    scores.build -= 6 * scale;
    scores.balanced += 8 * scale + (ctx.preferFree ? 6 : 0);
    scores.fast += 10 * scale + (ctx.preferFree ? 8 : 0);
  }

  // Sticky bonus toward last tier (soft)
  if (ctx.lastRouteTier && ctx.lastRouteTier !== "imagine") {
    scores[ctx.lastRouteTier] += STICKY_BONUS;
  }

  return scores;
}

// fix PromptSignals - I used s.historyLong which doesn't exist - remove that dead branch in scoreTiers

function pickWinner(
  scores: Record<RouteTier, number>,
  s: PromptSignals,
  ctx: RouteContext,
): { tier: RouteTier; why: string } {
  // Hard Imagine gate
  if (s.imageHit && scores.imagine >= Math.max(scores.build, scores.think, scores.balanced) - 5) {
    return { tier: "imagine", why: "image/media request" };
  }

  // Follow-up: hard hold previous (except pure greetings)
  if (s.followUp && ctx.lastRouteTier && ctx.lastRouteTier !== "imagine" && !s.pureFast) {
    return {
      tier: ctx.lastRouteTier,
      why: "short follow-up — held prior tier",
    };
  }

  // Pure fast acks (skip if mid-thread after heavy work unless pure greeting)
  if (s.pureFast) {
    return { tier: "fast", why: "greeting/ack — save tokens" };
  }
  if (
    s.simple >= 55 &&
    s.words <= 4 &&
    !s.judgmentHit &&
    !s.debugHit &&
    !s.stickyDeep &&
    !(ctx.lastRouteTier && TIER_RANK[ctx.lastRouteTier] >= 2)
  ) {
    return { tier: "fast", why: "short casual — save tokens" };
  }

  // argmax over ladder + imagine only if high
  let best: RouteTier = "balanced";
  let bestScore = -1e9;
  for (const tier of [...TIER_LADDER, "imagine"] as RouteTier[]) {
    if (tier === "imagine" && scores.imagine < 40) continue;
    const v = scores[tier];
    if (v > bestScore) {
      bestScore = v;
      best = tier;
    }
  }

  // Soft Deep promotion for architecture / research when close
  if (
    best === "think" &&
    (s.archHit || s.researchHit || s.teamHit) &&
    scores.deep >= scores.think - 12 &&
    !(s.codeHit && s.heavyCode)
  ) {
    best = "deep";
    bestScore = scores.deep;
  }

  // Code/implement sessions prefer Build over Deep when both strong
  if (
    (best === "deep" || best === "think") &&
    s.codeHit &&
    (s.heavyCode || s.hasCodeFence || s.codingSession) &&
    scores.build >= scores.deep - 15
  ) {
    best = "build";
    bestScore = scores.build;
  }

  // Hysteresis: max one ladder step unless score gap large
  const last = ctx.lastRouteTier;
  if (last && last !== "imagine" && best !== "imagine" && !s.pureFast) {
    const from = TIER_RANK[last];
    const to = TIER_RANK[best];
    const gap = Math.abs(to - from);
    const lastScore = scores[last] ?? 0;
    const delta = bestScore - lastScore;
    // Require larger score gaps to leap multiple ladder steps
    if (gap > 1 && delta < HYSTERESIS_JUMP * gap) {
      const step = to > from ? 1 : -1;
      const mid = TIER_LADDER[from + step];
      if (mid) {
        return {
          tier: mid,
          why: `hysteresis (was ${tierMeta(last).short} → ${tierMeta(best).short}; stepped to ${tierMeta(mid).short})`,
        };
      }
    }
    // If winner barely beats last, stay
    if (best !== last && delta < 6 && gap >= 1 && (s.stickyThink || (ctx.historyTurns || 0) >= 2)) {
      return {
        tier: last,
        why: `held ${tierMeta(last).short} — weak score gap`,
      };
    }
  }

  // Reasons
  let why = "best overall score";
  if (best === "deep") {
    if (s.teamHit) why = "multi-angle / team-style work";
    else if (s.researchHit) why = "research-style analysis";
    else if (s.archHit) why = "architecture / system design";
    else if (s.debugHit) why = "hard multi-step debug";
    else why = "high complexity";
  } else if (best === "build") {
    why = s.toolHit ? "coding + host/tool work" : "coding / implementation";
  } else if (best === "think") {
    if (s.judgmentHit && !s.lightJudgment) why = "substantive judgment / analysis";
    else if (s.debugHit) why = "debugging needs reasoning";
    else why = "analytical prompt";
  } else if (best === "balanced") {
    if (s.lightJudgment) why = "light opinion — everyday model";
    else if (s.uxHit) why = "UX / product polish";
    else why = "solid everyday chat";
  } else if (best === "fast") {
    why = "short / low-cost turn";
  }

  if (ctx.preferFree || (ctx.usagePressure ?? 0) > 0.7) {
    if (best === "fast" || best === "balanced") {
      why += ctx.preferFree ? " · free-tier bias" : " · usage pressure";
    }
  }

  return { tier: best, why };
}

function tierToRoute(
  tier: RouteTier,
  slots: ResolvedCatalog["slots"],
  s: PromptSignals,
): {
  routedMode: AutoRouteResult["routedMode"];
  modelId: string;
  intent: RouteIntent;
} {
  if (tier === "imagine") {
    return { routedMode: "imagine", modelId: slots.imagine, intent: "image" };
  }
  if (tier === "fast") {
    return { routedMode: "fast", modelId: slots.fast, intent: "chat_fast" };
  }
  if (tier === "balanced") {
    return {
      routedMode: "balanced",
      modelId: slots.balanced,
      intent: "chat_balanced",
    };
  }
  if (tier === "think") {
    return {
      routedMode: "expert",
      modelId: slots.smart,
      intent: "chat_smart",
    };
  }
  if (tier === "build") {
    return { routedMode: "build", modelId: slots.build, intent: "code" };
  }
  // deep
  return {
    routedMode: "heavy",
    modelId: slots.heavy,
    intent: s.researchHit ? "research" : s.teamHit ? "team" : "chat_smart",
  };
}

function formatScoreLine(scores: Record<RouteTier, number>): string {
  const keys: RouteTier[] = ["fast", "balanced", "think", "build", "deep"];
  return keys
    .map((k) => `${k[0]!.toUpperCase()}${Math.round(scores[k])}`)
    .join(" ");
}

export function routeAuto(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx: RouteContext = {},
): AutoRouteResult {
  const p = prompt.trim();
  const slots = catalog.slots;
  const s = scorePrompt(p, ctx);
  // Fix accidental historyLong reference - scoreTiers rewritten cleanly without it

  const tierScores = scoreTiers(s, ctx);
  // Soft boost explain/summarize → balanced (can't regex empty string earlier)
  if (/\b(explain|summarize|checklist|plain\s+english|how\s+do\s+i)\b/i.test(p) && !s.heavyCode) {
    tierScores.balanced += 12;
  }
  if (s.words > 70 || p.length > 650) {
    tierScores.deep += 20;
  }

  const { tier, why } = pickWinner(tierScores, s, ctx);
  const mapped = tierToRoute(tier, slots, s);
  const tm = tierMeta(tier);
  const scoreLine = formatScoreLine(tierScores);
  const detail = `Adaptive chose ${tm.label} — ${why}.`;

  return {
    routedMode: mapped.routedMode,
    modelId: mapped.modelId,
    intent: mapped.intent,
    tier,
    tierLabel: tm.label,
    reason: `${tm.label} · ${friendlyModelName(mapped.modelId)}`,
    reasonDetail: `${detail} · scores ${scoreLine}`,
    openImagine: tier === "imagine",
    scores: {
      complexity: s.complexity,
      analytical: s.analytical,
      code: s.code,
      creative: s.creative,
      simple: s.simple,
      ...Object.fromEntries(
        Object.entries(tierScores).map(([k, v]) => [`tier_${k}`, Math.round(v)]),
      ),
    },
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
