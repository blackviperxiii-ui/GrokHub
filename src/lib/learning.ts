/**
 * Learning & self-improvement engine.
 * Collects turn outcomes, user feedback, and tool patterns;
 * distills durable insights that pin into context and bias Adaptive routing.
 */
import type { GrokModeId } from "./types";
import type { RouteTier } from "./models-catalog";

export type LearningKind =
  | "route"
  | "tool"
  | "feedback"
  | "pref"
  | "reflection"
  | "correction"
  | "skill"
  | "chip";

export type LearningEvent = {
  id: string;
  ts: number;
  kind: LearningKind;
  /** Short human line */
  summary: string;
  detail?: string;
  /** +1 good / -1 bad / 0 neutral */
  polarity: -1 | 0 | 1;
  tags?: string[];
  threadId?: string;
  mode?: GrokModeId;
  routeTier?: RouteTier;
  modelId?: string;
};

export type LearningInsight = {
  id: string;
  ts: number;
  /** Stable category key for merge */
  key: string;
  text: string;
  /** Confidence 0–1 */
  confidence: number;
  hits: number;
  lastSeenAt: number;
  source: "distill" | "user" | "feedback" | "route";
};

export type LearningState = {
  version: 1;
  events: LearningEvent[];
  insights: LearningInsight[];
  /** Adaptive tier success/fail counters */
  routeStats: Partial<
    Record<RouteTier, { success: number; fail: number; lastAt: number }>
  >;
  /** Free-text preference counts */
  prefHits: Record<string, number>;
  totalTurns: number;
  totalFeedback: number;
  lastReflectionAt: number | null;
  updatedAt: number;
};

const MAX_EVENTS = 200;
const MAX_INSIGHTS = 40;
const MAX_PREF_KEYS = 60;

export function emptyLearning(): LearningState {
  return {
    version: 1,
    events: [],
    insights: [],
    routeStats: {},
    prefHits: {},
    totalTurns: 0,
    totalFeedback: 0,
    lastReflectionAt: null,
    updatedAt: Date.now(),
  };
}

export function normalizeLearning(raw: unknown): LearningState {
  const empty = emptyLearning();
  if (!raw || typeof raw !== "object") return empty;
  const m = raw as Partial<LearningState>;
  if (m.version !== 1) return empty;
  return {
    version: 1,
    events: Array.isArray(m.events)
      ? m.events
          .filter((e) => e && typeof e.id === "string" && e.summary)
          .slice(-MAX_EVENTS)
          .map((e) => ({
            id: e.id,
            ts: Number(e.ts) || 0,
            kind: (e.kind || "pref") as LearningKind,
            summary: String(e.summary).slice(0, 240),
            detail: e.detail ? String(e.detail).slice(0, 800) : undefined,
            polarity: (e.polarity === -1 || e.polarity === 1 ? e.polarity : 0) as
              | -1
              | 0
              | 1,
            tags: Array.isArray(e.tags)
              ? e.tags.map(String).slice(0, 8)
              : undefined,
            threadId: e.threadId,
            mode: e.mode,
            routeTier: e.routeTier,
            modelId: e.modelId,
          }))
      : [],
    insights: Array.isArray(m.insights)
      ? m.insights
          .filter((i) => i && typeof i.key === "string" && i.text)
          .slice(0, MAX_INSIGHTS)
          .map((i) => ({
            id: i.id || i.key,
            ts: Number(i.ts) || 0,
            key: String(i.key).slice(0, 80),
            text: String(i.text).slice(0, 280),
            confidence: Math.min(1, Math.max(0, Number(i.confidence) || 0.4)),
            hits: Math.max(1, Number(i.hits) || 1),
            lastSeenAt: Number(i.lastSeenAt) || Date.now(),
            source: (i.source || "distill") as LearningInsight["source"],
          }))
      : [],
    routeStats:
      m.routeStats && typeof m.routeStats === "object" ? m.routeStats : {},
    prefHits: m.prefHits && typeof m.prefHits === "object" ? m.prefHits : {},
    totalTurns: Math.max(0, Number(m.totalTurns) || 0),
    totalFeedback: Math.max(0, Number(m.totalFeedback) || 0),
    lastReflectionAt:
      typeof m.lastReflectionAt === "number" ? m.lastReflectionAt : null,
    updatedAt: Number(m.updatedAt) || Date.now(),
  };
}

function uid(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}

export function pushLearningEvent(
  state: LearningState,
  event: Omit<LearningEvent, "id" | "ts"> & { id?: string; ts?: number },
): LearningState {
  const row: LearningEvent = {
    id: event.id || uid("learn"),
    ts: event.ts || Date.now(),
    kind: event.kind,
    summary: event.summary.slice(0, 240),
    detail: event.detail?.slice(0, 800),
    polarity: event.polarity,
    tags: event.tags?.slice(0, 8),
    threadId: event.threadId,
    mode: event.mode,
    routeTier: event.routeTier,
    modelId: event.modelId,
  };
  const events = [...state.events, row].slice(-MAX_EVENTS);
  let routeStats = { ...state.routeStats };
  let totalTurns = state.totalTurns;
  let totalFeedback = state.totalFeedback;
  let prefHits = { ...state.prefHits };

  if (row.kind === "route" && row.routeTier) {
    const cur = routeStats[row.routeTier] || { success: 0, fail: 0, lastAt: 0 };
    routeStats = {
      ...routeStats,
      [row.routeTier]: {
        success: cur.success + (row.polarity > 0 ? 1 : 0),
        fail: cur.fail + (row.polarity < 0 ? 1 : 0),
        lastAt: row.ts,
      },
    };
    totalTurns += 1;
  }
  if (row.kind === "feedback") totalFeedback += 1;
  if (row.kind === "pref" || row.kind === "correction") {
    const key = row.summary.toLowerCase().slice(0, 80);
    prefHits[key] = (prefHits[key] || 0) + 1;
    // prune
    const keys = Object.keys(prefHits);
    if (keys.length > MAX_PREF_KEYS) {
      const sorted = keys.sort((a, b) => (prefHits[a] || 0) - (prefHits[b] || 0));
      for (const k of sorted.slice(0, keys.length - MAX_PREF_KEYS)) {
        delete prefHits[k];
      }
    }
  }

  return {
    ...state,
    events,
    routeStats,
    prefHits,
    totalTurns,
    totalFeedback,
    updatedAt: Date.now(),
  };
}

export function upsertInsight(
  state: LearningState,
  insight: Omit<LearningInsight, "id" | "ts" | "hits" | "lastSeenAt"> & {
    id?: string;
    hits?: number;
  },
): LearningState {
  const key = insight.key.slice(0, 80);
  const existing = state.insights.find((i) => i.key === key);
  let insights: LearningInsight[];
  if (existing) {
    insights = state.insights.map((i) =>
      i.key === key
        ? {
            ...i,
            text: insight.text.slice(0, 280),
            confidence: Math.min(
              1,
              Math.max(i.confidence, insight.confidence) + 0.05,
            ),
            hits: i.hits + (insight.hits || 1),
            lastSeenAt: Date.now(),
            source: insight.source || i.source,
          }
        : i,
    );
  } else {
    insights = [
      {
        id: insight.id || uid("ins"),
        ts: Date.now(),
        key,
        text: insight.text.slice(0, 280),
        confidence: Math.min(1, Math.max(0.2, insight.confidence)),
        hits: insight.hits || 1,
        lastSeenAt: Date.now(),
        source: insight.source,
      },
      ...state.insights,
    ].slice(0, MAX_INSIGHTS);
  }
  return { ...state, insights, updatedAt: Date.now() };
}

/** Heuristic: pull prefs/corrections from a user message */
export function extractUserPrefs(text: string): string[] {
  const t = String(text || "");
  const out: string[] = [];
  const patterns = [
    /\b(?:always|never|prefer|don't|do not|please)\s+[^\n.!?]{6,100}/gi,
    /\b(?:from now on|remember that|note that)\s+[^\n.!?]{6,120}/gi,
    /\bI (?:like|hate|want|need|use)\s+[^\n.!?]{4,80}/gi,
  ];
  for (const re of patterns) {
    const m = t.match(re);
    if (!m) continue;
    for (const hit of m) {
      const clean = hit.replace(/\s+/g, " ").trim();
      if (clean.length >= 10 && !out.includes(clean)) out.push(clean.slice(0, 140));
      if (out.length >= 4) return out;
    }
  }
  return out;
}

/** Record end-of-turn learning from a completed chat */
export function learnFromTurn(
  state: LearningState,
  opts: {
    ok: boolean;
    mode: GrokModeId;
    routeTier?: RouteTier;
    modelId?: string;
    userText: string;
    assistantText: string;
    threadId?: string;
    usedHostTools?: boolean;
    usedConnectors?: boolean;
  },
): LearningState {
  let next = pushLearningEvent(state, {
    kind: "route",
    summary: opts.ok
      ? `Turn ok · ${opts.routeTier || opts.mode}`
      : `Turn failed · ${opts.routeTier || opts.mode}`,
    detail: opts.userText.slice(0, 200),
    polarity: opts.ok ? 1 : -1,
    mode: opts.mode,
    routeTier: opts.routeTier,
    modelId: opts.modelId,
    threadId: opts.threadId,
    tags: [
      opts.ok ? "success" : "fail",
      opts.usedHostTools ? "host" : "",
      opts.usedConnectors ? "connector" : "",
    ].filter(Boolean),
  });

  if (opts.usedHostTools && opts.ok) {
    next = pushLearningEvent(next, {
      kind: "tool",
      summary: "Host tools helped complete the task",
      polarity: 1,
      tags: ["host"],
      threadId: opts.threadId,
    });
  }

  for (const pref of extractUserPrefs(opts.userText)) {
    next = pushLearningEvent(next, {
      kind: "pref",
      summary: pref,
      polarity: 1,
      tags: ["user-pref"],
      threadId: opts.threadId,
    });
    next = upsertInsight(next, {
      key: `pref:${pref.toLowerCase().slice(0, 48)}`,
      text: pref,
      confidence: 0.55,
      source: "user",
    });
  }

  // Soft auto-insight from repeated successful tiers
  if (opts.ok && opts.routeTier) {
    const st = next.routeStats[opts.routeTier];
    if (st && st.success >= 3 && st.success > st.fail * 2) {
      next = upsertInsight(next, {
        key: `route-good:${opts.routeTier}`,
        text: `Adaptive tier “${opts.routeTier}” has been working well for this user — prefer it when similar tasks appear.`,
        confidence: Math.min(0.85, 0.4 + st.success * 0.05),
        source: "route",
      });
    }
  }

  return next;
}

export function learnFromFeedback(
  state: LearningState,
  opts: {
    positive: boolean;
    messagePreview: string;
    routeTier?: RouteTier;
    mode?: GrokModeId;
    threadId?: string;
  },
): LearningState {
  let next = pushLearningEvent(state, {
    kind: "feedback",
    summary: opts.positive ? "User liked this reply" : "User disliked this reply",
    detail: opts.messagePreview.slice(0, 200),
    polarity: opts.positive ? 1 : -1,
    routeTier: opts.routeTier,
    mode: opts.mode,
    threadId: opts.threadId,
    tags: [opts.positive ? "up" : "down"],
  });
  if (opts.routeTier) {
    const cur = next.routeStats[opts.routeTier] || {
      success: 0,
      fail: 0,
      lastAt: 0,
    };
    next = {
      ...next,
      routeStats: {
        ...next.routeStats,
        [opts.routeTier]: {
          success: cur.success + (opts.positive ? 1 : 0),
          fail: cur.fail + (opts.positive ? 0 : 1),
          lastAt: Date.now(),
        },
      },
    };
    if (!opts.positive) {
      next = upsertInsight(next, {
        key: `route-careful:${opts.routeTier}`,
        text: `User has disliked some “${opts.routeTier}” replies — be more careful or offer alternatives when using that tier.`,
        confidence: 0.5,
        source: "feedback",
      });
    }
  }
  return next;
}

/**
 * Distill recent events into insights + markdown for MEMORY/LEARNINGS.
 * Runs offline (no model call).
 */
export function reflectLearning(state: LearningState): {
  state: LearningState;
  markdown: string;
  newInsights: LearningInsight[];
} {
  const recent = state.events.slice(-80);
  const newInsights: LearningInsight[] = [];
  let next = { ...state };

  // Pref frequency
  const prefCount: Record<string, number> = {};
  for (const e of recent.filter((x) => x.kind === "pref" || x.kind === "correction")) {
    const k = e.summary.toLowerCase().slice(0, 80);
    prefCount[k] = (prefCount[k] || 0) + 1;
  }
  for (const [k, n] of Object.entries(prefCount)) {
    if (n < 1) continue;
    const text = recent.find((e) => e.summary.toLowerCase().startsWith(k.slice(0, 20)))
      ?.summary;
    if (!text) continue;
    const ins = {
      key: `reflect-pref:${k.slice(0, 40)}`,
      text: `User preference: ${text}`,
      confidence: Math.min(0.9, 0.45 + n * 0.1),
      source: "distill" as const,
    };
    next = upsertInsight(next, ins);
    const found = next.insights.find((i) => i.key === ins.key);
    if (found) newInsights.push(found);
  }

  // Route performance
  for (const [tier, st] of Object.entries(next.routeStats)) {
    if (!st) continue;
    const total = st.success + st.fail;
    if (total < 3) continue;
    const rate = st.success / total;
    if (rate >= 0.7) {
      const ins = {
        key: `reflect-route-up:${tier}`,
        text: `Tier “${tier}” success rate ~${Math.round(rate * 100)}% (${st.success}/${total}) — good default for similar work.`,
        confidence: Math.min(0.9, rate),
        source: "distill" as const,
      };
      next = upsertInsight(next, ins);
    } else if (rate <= 0.4 && st.fail >= 2) {
      const ins = {
        key: `reflect-route-down:${tier}`,
        text: `Tier “${tier}” has been weak (~${Math.round(rate * 100)}% ok). Prefer another tier or ask clarifying questions.`,
        confidence: 0.55,
        source: "distill" as const,
      };
      next = upsertInsight(next, ins);
    }
  }

  // Host tool affinity
  const hostOk = recent.filter(
    (e) => e.kind === "tool" && e.tags?.includes("host") && e.polarity > 0,
  ).length;
  if (hostOk >= 2) {
    next = upsertInsight(next, {
      key: "reflect-host",
      text: "Desktop host tools are valuable for this user — prefer HOST_CMD for system/file questions over guessing.",
      confidence: 0.7,
      source: "distill",
    });
  }

  next = {
    ...next,
    lastReflectionAt: Date.now(),
    updatedAt: Date.now(),
  };
  next = pushLearningEvent(next, {
    kind: "reflection",
    summary: `Self-improve reflection · ${next.insights.length} insights`,
    polarity: 1,
    tags: ["reflection"],
  });

  const top = [...next.insights]
    .sort((a, b) => b.confidence * b.hits - a.confidence * a.hits)
    .slice(0, 16);

  const md = [
    "# GrokHub learnings",
    "",
    `_Reflected ${new Date().toISOString().slice(0, 16).replace("T", " ")} · ${next.totalTurns} turns · ${next.totalFeedback} feedback_`,
    "",
    "## Insights",
    ...top.map(
      (i) =>
        `- (${Math.round(i.confidence * 100)}%) ${i.text}`,
    ),
    "",
    "## Route stats",
    ...Object.entries(next.routeStats).map(([tier, st]) => {
      if (!st) return "";
      return `- ${tier}: ${st.success} ok / ${st.fail} fail`;
    }).filter(Boolean),
    "",
  ].join("\n");

  return { state: next, markdown: md, newInsights };
}

/** Pin block for context builder */
export function learningPinBundle(state: LearningState, maxChars = 3_500): string {
  const top = [...state.insights]
    .sort((a, b) => b.confidence * 10 + b.hits - (a.confidence * 10 + a.hits))
    .slice(0, 12);
  if (!top.length && !Object.keys(state.routeStats).length) return "";

  const lines = [
    "## Learned preferences & self-improvement",
    "Apply these when relevant. Do not recite this list unless asked.",
    "",
  ];
  for (const i of top) {
    lines.push(`- ${i.text}`);
  }
  const routeBits = Object.entries(state.routeStats)
    .map(([tier, st]) => {
      if (!st || st.success + st.fail < 2) return null;
      const rate = st.success / (st.success + st.fail);
      return `${tier}:${Math.round(rate * 100)}%`;
    })
    .filter(Boolean);
  if (routeBits.length) {
    lines.push("", `Route track record: ${routeBits.join(" · ")}`);
  }
  let body = lines.join("\n");
  if (body.length > maxChars) body = body.slice(0, maxChars) + "\n…";
  return body;
}

/**
 * Soft score bias for Adaptive routing from learning stats.
 * Positive = prefer tier; negative = avoid.
 */
export function routeLearningBias(state: LearningState): Partial<Record<RouteTier, number>> {
  const bias: Partial<Record<RouteTier, number>> = {};
  for (const [tier, st] of Object.entries(state.routeStats)) {
    if (!st) continue;
    const total = st.success + st.fail;
    if (total < 2) continue;
    const rate = st.success / total;
    // map 0–1 rate to about -0.15 … +0.2
    bias[tier as RouteTier] = (rate - 0.5) * 0.4;
  }
  return bias;
}

export function learningSummaryLine(state: LearningState): string {
  const n = state.insights.length;
  const t = state.totalTurns;
  const f = state.totalFeedback;
  if (!n && !t) return "No learnings yet — use the app and rate replies";
  return `${n} insights · ${t} turns · ${f} ratings`;
}
