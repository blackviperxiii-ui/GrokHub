/**
 * End-of-turn session learning: extract concrete notes and drive disk writes.
 * This is the active loop — not just static templates on disk.
 */
import {
  extractUserPrefs,
  learnFromTurn,
  reflectLearning,
  upsertInsight,
  pushLearningEvent,
  learningStatusMarkdown,
  learningSnapshotMarkdown,
  type LearningState,
} from "./learning";
import type { GrokModeId } from "./types";
import type { RouteTier } from "./models-catalog";
import {
  memoryAppend,
  memoryAppendFacts,
  memoryRead,
  memoryWrite,
  ensureFileMemory,
  syncLearningToDisk,
  memoryFsInfo,
} from "./file-memory";

/** Skip meta complaints that bloat MEMORY and re-trigger mid-session. */
const NOISE_TOPIC =
  /\b(stream(ing)?|stuck|check again|you keep|breaking|self-?improv|learning loop|placeholder|still broken|look again)\b/i;

function isNoisyTopic(s0: string): boolean {
  const s = s0.trim();
  if (s.length < 8) return true;
  if (NOISE_TOPIC.test(s) && s.length < 160) return true;
  if (/^(fix|check|look|update|push)\b/i.test(s) && s.length < 48) return true;
  return false;
}

export type TurnLearnInput = {
  ok: boolean;
  mode: GrokModeId;
  routeTier?: RouteTier;
  modelId?: string;
  userText: string;
  assistantText: string;
  threadId?: string;
  threadTitle?: string;
  usedHostTools?: boolean;
  usedConnectors?: boolean;
  online?: boolean;
};

export type TurnLearnResult = {
  learning: LearningState;
  dailyLine: string;
  memoryFacts: string[];
  userFacts: string[];
  didReflect: boolean;
  diskRoot?: string;
};

/** Broader extraction than pref-only regexes. */
export function extractSessionSignals(userText: string, assistantText: string): {
  prefs: string[];
  facts: string[];
  topics: string[];
} {
  const user = String(userText || "").replace(/\s+/g, " ").trim();
  const asst = String(assistantText || "").replace(/\s+/g, " ").trim();
  const prefs = extractUserPrefs(user);
  const facts: string[] = [];
  const topics: string[] = [];

  // Topic = first substantial user ask
  if (user.length >= 12) {
    const topic = user
      .replace(/^\/\w+\s*/, "")
      .slice(0, 120)
      .trim();
    if (topic && !isNoisyTopic(topic)) topics.push(topic);
  }

  // Paths
  const pathRe = /(?:^|[\s"`'])(\/(?:home|usr|var|etc|tmp|opt)[^\s"'`]{2,80}|~\/[^\s"'`]{2,80})/g;
  for (const blob of [user, asst]) {
    let m: RegExpExecArray | null;
    const re = new RegExp(pathRe.source, "g");
    while ((m = re.exec(blob))) {
      const p = m[1];
      if (p && !facts.includes(`Path: ${p}`)) facts.push(`Path: ${p}`);
      if (facts.length >= 6) break;
    }
  }

  // Versions
  const ver = blobMatch(user + "\n" + asst, /\b(?:v?\d+\.\d+(?:\.\d+)?|Electron\s+\d+)\b/gi, 4);
  for (const v of ver) {
    if (!facts.some((f) => f.includes(v))) facts.push(`Version mention: ${v}`);
  }

  // Style / length preferences (soft)
  if (/\b(short|concise|brief|bullets?|no fluff|tl;?dr)\b/i.test(user)) {
    prefs.push("User prefers short structured answers with bullets");
  }
  if (/\b(deep dive|thorough|detailed|comprehensive)\b/i.test(user)) {
    prefs.push("User sometimes wants thorough / deep-dive answers");
  }
  if (/\b(fix|bug|broken|doesn't work|not working)\b/i.test(user)) {
    if (!isNoisyTopic(user)) topics.push("Debugging / fixing something broken");
  }
  if (/\b(self-?improv|learning|memory)\b/i.test(user)) {
    // Don't re-log learning meta every turn — only once as a soft fact later via prefs
  }
  // Skip host/agent-action spam — those bloated MEMORY.md every turn

  // Dedupe prefs
  const prefOut: string[] = [];
  for (const p of prefs) {
    if (!prefOut.some((x) => x.toLowerCase() === p.toLowerCase())) prefOut.push(p);
  }

  return {
    prefs: prefOut.slice(0, 6),
    facts: facts.slice(0, 10),
    topics: topics.slice(0, 4),
  };
}

function blobMatch(blob: string, re: RegExp, max: number): string[] {
  const out: string[] = [];
  const m = blob.match(re);
  if (!m) return out;
  for (const hit of m) {
    if (!out.includes(hit)) out.push(hit);
    if (out.length >= max) break;
  }
  return out;
}

/**
 * Full turn pipeline: engine state + disk writes (daily, MEMORY, USER, STATUS, LEARNINGS).
 */
export async function applyTurnLearning(
  prev: LearningState,
  input: TurnLearnInput,
): Promise<TurnLearnResult> {
  await ensureFileMemory();
  const signals = extractSessionSignals(input.userText, input.assistantText);

  let learning = learnFromTurn(prev, {
    ok: input.ok,
    mode: input.mode,
    routeTier: input.routeTier,
    modelId: input.modelId,
    userText: input.userText,
    assistantText: input.assistantText,
    threadId: input.threadId,
    usedHostTools: input.usedHostTools,
    usedConnectors: input.usedConnectors,
  });

  // Promote extracted prefs/topics/facts into insights immediately
  for (const p of signals.prefs) {
    learning = upsertInsight(learning, {
      key: `pref:${p.toLowerCase().slice(0, 48)}`,
      text: p,
      confidence: 0.6,
      source: "user",
    });
  }
  for (const t of signals.topics) {
    if (isNoisyTopic(t)) continue;
    // Skip re-logging identical focus within existing insights
    const key = `topic:${t.toLowerCase().slice(0, 40)}`;
    if (learning.insights.some((i) => i.key === key && i.hits >= 1)) continue;
    learning = upsertInsight(learning, {
      key,
      text: `Recent focus: ${t}`,
      confidence: 0.45,
      source: "distill",
    });
  }
  for (const f of signals.facts) {
    if (isNoisyTopic(f)) continue;
    learning = upsertInsight(learning, {
      key: `fact:${f.toLowerCase().slice(0, 40)}`,
      text: f,
      confidence: 0.5,
      source: "distill",
    });
  }

  // Always log the turn on daily + as event
  const stamp = new Date().toISOString().replace("T", " ").slice(0, 16);
  const dailyLine = [
    stamp,
    input.ok ? "ok" : "fail",
    input.routeTier || input.mode,
    input.usedHostTools ? "host" : null,
    input.usedConnectors ? "conn" : null,
    (input.userText || "").replace(/\s+/g, " ").trim().slice(0, 100),
  ]
    .filter(Boolean)
    .join(" · ");

  learning = pushLearningEvent(learning, {
    kind: "reflection",
    summary: `Session note: ${dailyLine.slice(0, 180)}`,
    polarity: input.ok ? 1 : -1,
    tags: ["session"],
    threadId: input.threadId,
  });

  // Light reflect every 3 turns (heuristic) so LEARNINGS.md is never forever-empty
  let didReflect = false;
  if (learning.totalTurns > 0 && learning.totalTurns % 3 === 0) {
    const r = reflectLearning(learning);
    learning = r.state;
    didReflect = true;
  }

  // --- Disk writes (best effort, always attempt) ---
  const memoryFacts = [
    ...signals.facts.filter((f) => !isNoisyTopic(f)),
    ...signals.topics.filter((t) => !isNoisyTopic(t)).map((t) => `Focus: ${t}`),
  ];
  const userFacts = signals.prefs;

  try {
    await memoryAppend("today", dailyLine);
  } catch {
    /* ignore */
  }
  if (memoryFacts.length) {
    try {
      await memoryAppendFacts(memoryFacts, { target: "MEMORY.md" });
    } catch {
      /* ignore */
    }
  }
  if (userFacts.length) {
    try {
      await memoryAppendFacts(userFacts, { target: "USER.md" });
    } catch {
      /* ignore */
    }
  }

  // Merge new bullets into MEMORY without wiping user edits
  if (didReflect) {
    try {
      const { markdown } = reflectLearning(learning);
      // Prefer snapshot that includes insights
      await memoryWrite("LEARNINGS.md", markdown);
      const tops = learning.insights.slice(0, 5).map((i) => i.text);
      if (tops.length) await memoryAppendFacts(tops, { target: "MEMORY.md" });
    } catch {
      /* ignore */
    }
  }

  let diskRoot: string | undefined;
  try {
    const info = await memoryFsInfo();
    diskRoot = info.root;
    await syncLearningToDisk({
      statusMarkdown: learningStatusMarkdown(learning, {
        root: info.root,
        userData: info.userData,
      }),
      learningsMarkdown: learningSnapshotMarkdown(learning),
    });
  } catch {
    /* ignore */
  }

  return {
    learning,
    dailyLine,
    memoryFacts,
    userFacts,
    didReflect,
    diskRoot,
  };
}

/** Parse agent MEMORY_NOTE / LEARN_NOTE lines (stripped from visible chat). */
export function extractMemoryNotes(text: string): {
  notes: string[];
  cleaned: string;
} {
  const notes: string[] = [];
  const kept: string[] = [];
  for (const line of String(text || "").split("\n")) {
    const m = line.match(/^\s*(?:MEMORY_NOTE|LEARN_NOTE):\s*(.+)$/i);
    if (m?.[1]) {
      notes.push(m[1].trim());
      continue;
    }
    kept.push(line);
  }
  return {
    notes,
    cleaned: kept.join("\n").replace(/\n{3,}/g, "\n\n").trim(),
  };
}

export async function applyAgentMemoryNotes(
  learning: LearningState,
  notes: string[],
): Promise<LearningState> {
  if (!notes.length) return learning;
  let next = learning;
  for (const n of notes) {
    next = pushLearningEvent(next, {
      kind: "pref",
      summary: n,
      polarity: 1,
      tags: ["agent-note"],
    });
    next = upsertInsight(next, {
      key: `agent:${n.toLowerCase().slice(0, 48)}`,
      text: n,
      confidence: 0.55,
      source: "distill",
    });
  }
  try {
    await memoryAppendFacts(notes, { target: "MEMORY.md" });
    await memoryAppend(
      "today",
      `Agent notes: ${notes.map((n) => n.slice(0, 60)).join(" · ")}`,
    );
  } catch {
    /* ignore */
  }
  return next;
}
