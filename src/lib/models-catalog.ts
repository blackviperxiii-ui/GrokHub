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
export type RouteTier = "fast" | "think" | "deep" | "build" | "imagine";

export type AutoRouteResult = {
  routedMode: "fast" | "expert" | "heavy" | "build" | "imagine";
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
  lastRoutedMode?: "fast" | "expert" | "heavy" | "max" | "build" | "imagine";

  /** Soft bias from learning engine: positive = prefer tier */
  learningBias?: Partial<Record<RouteTier, number>>;
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
- smart: hard reasoning / Think mode (prefer grok-4.20-reasoning, then 4.5-class flagship)
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

// ─── Adaptive router (stable, judgment-aware) ───────────────────────────────

const IMAGE_RE =
  /\b(imagine|image|picture|photo|draw|render|generate\s+(an?\s+)?(image|pic|art|logo|icon|wallpaper)|illustration|visuali[sz]e)\b/i;

const CODE_RE =
  /\b(code|coding|implement|refactor|typescript|javascript|python|rust|golang|react|component|function|class|bugfix|compile|lint|docker|kubernetes|pkgbuild|aur|api\s+route|pull\s+request|unit\s+test|css|html|sql|bash|shell\s+script|write\s+(me\s+)?(a\s+)?(script|app|site|page|endpoint)|typecheck|stack.?trace|PR\b|merge\s+conflict)\b/i;

const ARCH_RE =
  /\b(architect(?:ure)?|system\s+design|design\s+system|trade-?offs?|scalability|data\s+model|schema|migration\s+plan|service\s+boundary|microservice|event-?driven|end-?to-?end|production\s+ready)\b/i;

const SMART_RE =
  /\b(root\s+cause|debug|why\s+is|compare|evaluate|critique|security|threat|prove|theorem|math|optimiz|complex|multi-?step|deep\s+dive|analyze\s+carefully|reason\s+about|pros?\s+and\s+cons?|step\s+by\s+step)\b/i;

const JUDGMENT_RE =
  /\b(what\s+do\s+you\s+think|how\s+do\s+(you\s+)?(feel|see)|feels?\s+off|doesn'?t\s+feel|something'?s\s+off|seems?\s+(off|wrong|weird)|how\s+(can|do)\s+i\s+improve|improve\s+(this|it|the)|make\s+(this|it)\s+better|thoughts\s+on|your\s+(take|opinion)|review\s+(this|my)|is\s+this\s+(good|ok|right|wrong)|what'?s\s+wrong|why\s+is\s+this|honest\s+feedback|feedback\s+on|rate\s+this|look\s+(right|wrong)|still\s+broken|not\s+working\s+right|help\s+me\s+(decide|choose|pick)|should\s+i)\b/i;

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
  /\b(\$\s|HOST_CMD|shell|cli|run\s+(this\s+)?command|on\s+my\s+(machine|desktop)|list\s+files|read\s+file|edit\s+file)\b/i;

const DEBUG_SESSION_RE =
  /\b(bug|broken|error|crash|fix|debug|stack|trace|failing|regression|doesn'?t\s+work|not\s+working)\b/i;

const FOLLOW_UP_RE =
  /^(yes|yeah|yep|yup|sure|please|do\s+it|do\s+that|go\s+ahead|continue|proceed|try\s+(it|that|again)|fix\s+it|same|also|and\s+(also|then)|ok\s+(do|go|try|please)|sounds\s+good|that\s+one|this\s+one|again|more|keep\s+going)[.!]*$/i;

const FOLLOW_UP_SOFT_RE =
  /\b(also|and\s+then|same\s+for|do\s+the\s+same|one\s+more|instead|rather|actually|wait)\b/i;

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

function scorePrompt(prompt: string, ctx: RouteContext = {}) {
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
  const researchHit = RESEARCH_RE.test(p);
  const teamHit = TEAM_RE.test(p);
  const pureFast = FAST_RE.test(p.trim());
  const followUp =
    FOLLOW_UP_RE.test(p.trim()) ||
    (words <= 12 && FOLLOW_UP_SOFT_RE.test(p) && Boolean(ctx.lastRouteTier));
  const creativeHit = CREATIVE_RE.test(p);
  const uxHit = UX_RE.test(p);
  const toolHit = TOOL_RE.test(p) || Boolean(ctx.hasAttachments);
  const debugHit = DEBUG_SESSION_RE.test(p) || DEBUG_SESSION_RE.test(recent);
  const longReplyContext = (ctx.recentAssistantText || "").length > 800;
  const stickyDeep =
    ctx.lastRouteTier === "deep" ||
    ctx.lastRouteTier === "build" ||
    ctx.lastRoutedMode === "heavy" ||
    ctx.lastRoutedMode === "build";
  const stickyThink =
    stickyDeep || ctx.lastRouteTier === "think" || ctx.lastRoutedMode === "expert";

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
  if (stickyThink) complexity += 8;
  if (debugHit) complexity += 14;

  let analytical = 0;
  if (judgmentHit) analytical += 42;
  if (smartHit) analytical += 32;
  if (archHit) analytical += 38;
  if (researchHit) analytical += 36;
  if (uxHit) analytical += 24;
  if (debugHit) analytical += 28;
  if (/\bwhy\b|\bhow\s+should\b|\btrade|\bfeel|\bthink\b/.test(lower)) analytical += 18;
  if (/\bcompare\b|\bvs\.?\b|\bdifference\b|\bshould\s+i\b/.test(lower)) analytical += 20;
  if (/\bimprove\b|\bbetter\b|\bpolish\b|\breview\b/.test(lower)) analytical += 22;
  if (stickyThink && words >= 3 && !pureFast) analytical += 15;

  let code = 0;
  if (codeHit) code += 38;
  if (hasCodeFence) code += 28;
  if (/\b(implement|scaffold|rewrite|migrate|refactor|full\s+app)\b/i.test(p)) code += 26;
  if (/\b(one-?liner|snippet|regex|rename\s+var)\b/i.test(p)) code -= 12;
  if (toolHit && codeHit) code += 10;
  if (debugHit && codeHit) code += 12;

  let creative = 0;
  if (creativeHit) creative += 40;
  if (/\bstory\b|\bjoke\b|\bpoem\b/.test(lower)) creative += 20;

  let simple = 0;
  if (pureFast) simple += 70;
  if (words <= 3 && !judgmentHit && !codeHit && !smartHit && !uxHit) simple += 30;
  if (/^(yes|no|ok|thanks|thank you|continue|go on)[.!]?$/i.test(p.trim())) simple += 45;
  if (judgmentHit) simple = 0;

  // Learning bias: nudge complexity/analytical/code/simple from past outcomes
  const bias = ctx.learningBias || {};
  if (bias.fast) simple += bias.fast * 40;
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
    stickyThink,
    stickyDeep,
    longReplyContext,
  };
}

const TIER_RANK: Record<RouteTier, number> = {
  fast: 0,
  think: 1,
  build: 2,
  deep: 3,
  imagine: 1,
};

export function routeAuto(
  prompt: string,
  catalog: ResolvedCatalog = emptyCatalog(),
  ctx: RouteContext = {},
): AutoRouteResult {
  const p = prompt.trim();
  const slots = catalog.slots;
  const s = scorePrompt(p, ctx);
  const scores = {
    complexity: s.complexity,
    analytical: s.analytical,
    code: s.code,
    creative: s.creative,
    simple: s.simple,
  };

  const finish = (
    routedMode: AutoRouteResult["routedMode"],
    modelId: string,
    intent: RouteIntent,
    tier: RouteTier,
    why: string,
    openImagine?: boolean,
  ): AutoRouteResult => {
    let finalTier = tier;
    let finalMode = routedMode;
    let finalModel = modelId;
    let finalWhy = why;

    if (!openImagine && s.followUp && ctx.lastRouteTier && ctx.lastRouteTier !== "imagine") {
      const prev = ctx.lastRouteTier;
      if (prev === "deep") {
        finalTier = "deep";
        finalMode = "heavy";
        finalModel = slots.heavy;
      } else if (prev === "build") {
        finalTier = "build";
        finalMode = "build";
        finalModel = slots.build;
      } else if (prev === "think") {
        finalTier = "think";
        finalMode = "expert";
        finalModel = slots.balanced;
      } else {
        finalTier = "fast";
        finalMode = "fast";
        finalModel = slots.fast;
      }
      finalWhy = `Adaptive held ${tierMeta(finalTier).label} — short follow-up in the same thread.`;
      const tm0 = tierMeta(finalTier);
      return {
        routedMode: finalMode,
        modelId: finalModel,
        intent,
        tier: finalTier,
        tierLabel: tm0.label,
        reason: `${tm0.label} · ${friendlyModelName(finalModel)}`,
        reasonDetail: finalWhy,
        openImagine,
        scores,
      };
    }

    if (
      !openImagine &&
      !s.pureFast &&
      ctx.lastRouteTier &&
      TIER_RANK[ctx.lastRouteTier] > TIER_RANK[tier] &&
      (s.stickyThink || s.debugHit || s.judgmentHit || (ctx.historyTurns || 0) >= 2)
    ) {
      if (ctx.lastRouteTier === "deep" && tier === "fast") {
        finalTier = "think";
        finalMode = "expert";
        finalModel = slots.balanced;
        finalWhy = `${why} · held 🧠 Think (was Deep).`;
      } else if (ctx.lastRouteTier === "deep" && tier === "think") {
        if (s.debugHit && s.analytical >= 28) {
          finalTier = "deep";
          finalMode = "heavy";
          finalModel = slots.heavy;
          finalWhy = `${why} · stayed 🔬 Deep (active debug).`;
        } else {
          finalTier = "think";
          finalMode = "expert";
          finalModel = slots.balanced;
          finalWhy = `${why} · eased to 🧠 Think (was Deep).`;
        }
      } else if (ctx.lastRouteTier === "build" && tier === "fast") {
        if (s.codeHit || s.debugHit) {
          finalTier = "build";
          finalMode = "build";
          finalModel = slots.build;
          finalWhy = `${why} · stayed 🛠️ Build (coding thread).`;
        } else {
          finalTier = "think";
          finalMode = "expert";
          finalModel = slots.balanced;
          finalWhy = `${why} · moved to 🧠 Think (no longer pure code).`;
        }
      } else if (ctx.lastRouteTier === "think" && tier === "fast" && s.words > 4) {
        finalTier = "think";
        finalMode = "expert";
        finalModel = slots.balanced;
        finalWhy = `${why} · held 🧠 Think (avoid Fast bounce).`;
      } else if (ctx.lastRouteTier === "build" && tier === "think" && s.codeHit) {
        finalTier = "build";
        finalMode = "build";
        finalModel = slots.build;
        finalWhy = `${why} · stayed 🛠️ Build (code still in play).`;
      }
    }

    const tm = tierMeta(finalTier);
    return {
      routedMode: finalMode,
      modelId: finalModel,
      intent,
      tier: finalTier,
      tierLabel: tm.label,
      reason: `${tm.label} · ${friendlyModelName(finalModel)}`,
      reasonDetail: finalWhy,
      openImagine,
      scores,
    };
  };

  if (s.imageHit) {
    return finish(
      "imagine",
      slots.imagine,
      "image",
      "imagine",
      "Adaptive chose Imagine — looks like an image/media request.",
      true,
    );
  }

  if (s.pureFast || (s.simple >= 55 && s.words <= 4 && !s.judgmentHit && !s.debugHit && !s.followUp)) {
    return finish(
      "fast",
      slots.fast,
      "chat_fast",
      "fast",
      "Adaptive chose Fast — short greeting/ack; save tokens.",
    );
  }

  if (
    s.teamHit ||
    (s.analytical >= 40 && s.complexity >= 42) ||
    (s.archHit && s.words >= 14 && s.complexity >= 30) ||
    (s.researchHit && s.words > 18) ||
    s.words > 70 ||
    p.length > 650 ||
    (s.debugHit && s.complexity >= 42 && s.analytical >= 30 && s.words >= 16)
  ) {
    return finish(
      "heavy",
      slots.heavy,
      s.researchHit ? "research" : s.teamHit ? "team" : "chat_smart",
      "deep",
      s.archHit
        ? "Adaptive chose Deep — architecture / system design."
        : s.researchHit
          ? "Adaptive chose Deep — research-style analysis."
          : s.debugHit
            ? "Adaptive chose Deep — hard multi-step debug."
            : "Adaptive chose Deep — high complexity work.",
    );
  }

  if (s.code >= 48 || (s.codeHit && (s.hasCodeFence || s.words > 20 || s.complexity >= 38))) {
    const heavyCodeVerb =
      /\b(implement|refactor|rewrite|migrate|scaffold|architect|full\s+app|end-?to-?end|production)\b/i.test(
        p,
      );
    if (
      s.codeHit &&
      s.words <= 10 &&
      !s.hasCodeFence &&
      s.complexity < 25 &&
      !heavyCodeVerb &&
      !s.debugHit
    ) {
      return finish(
        "fast",
        slots.fast,
        "chat_fast",
        "fast",
        "Adaptive chose Fast — tiny code question.",
      );
    }
    return finish(
      "build",
      slots.build,
      "code",
      "build",
      "Adaptive chose Build — coding / implementation session.",
    );
  }

  if (
    s.codeHit &&
    /\b(implement|refactor|rewrite|migrate|scaffold|fix\s+the\s+bug|add\s+tests?)\b/i.test(p)
  ) {
    return finish(
      "build",
      slots.build,
      "code",
      "build",
      "Adaptive chose Build — implementation / refactor request.",
    );
  }

  if (
    s.judgmentHit ||
    s.uxHit ||
    s.smartHit ||
    s.debugHit ||
    s.analytical >= 18 ||
    s.complexity >= 24 ||
    s.words > 14 ||
    /\b(plan|explain|help\s+me|walk\s+through|improve|fix|design|review|thoughts|how\s+do\s+i)\b/i.test(
      p,
    )
  ) {
    const useSmart =
      s.analytical >= 34 || s.complexity >= 48 || s.archHit || (s.judgmentHit && s.words > 8);
    return finish(
      "expert",
      useSmart ? slots.smart : slots.balanced,
      useSmart ? "chat_smart" : "chat_balanced",
      "think",
      s.judgmentHit
        ? "Adaptive chose Think — judgment / feedback needs real reasoning."
        : s.debugHit
          ? "Adaptive chose Think — debugging / something’s off."
          : s.uxHit
            ? "Adaptive chose Think — UX / product polish."
            : useSmart
              ? "Adaptive chose Think — analytical prompt; stronger model."
              : "Adaptive chose Think — more than a quick chat.",
    );
  }

  if (s.creativeHit && s.complexity < 35) {
    return finish(
      "fast",
      slots.fast,
      "chat_fast",
      "fast",
      "Adaptive chose Fast — light creative request.",
    );
  }

  if (s.words <= 10 && !s.stickyThink) {
    return finish(
      "fast",
      slots.fast,
      "chat_fast",
      "fast",
      "Adaptive chose Fast — short casual prompt.",
    );
  }

  return finish(
    "expert",
    slots.balanced,
    "chat_balanced",
    "think",
    "Adaptive chose Think — default for non-trivial prompts.",
  );
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
