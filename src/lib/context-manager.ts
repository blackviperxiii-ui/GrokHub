/**
 * Option A context management: budget, pins, tool trim, compaction summary.
 * Compaction affects the *API window* only — full chat stays in the UI.
 */
import type { ChatMessage, ChatThread } from "./types";

/** Soft budget for model input (approx tokens). Leave headroom for reply + tools. */
export const CONTEXT_BUDGET_TOKENS = 96_000;
/** Auto-compact when estimated input exceeds this fraction of budget */
export const COMPACT_THRESHOLD = 0.72;
/** Max tokens for pinned blocks (memory + openclaw slice + caps) */
export const PIN_BUDGET_TOKENS = 10_000;
/** Always keep at least this many recent messages unsummarized */
export const RECENT_MIN_MESSAGES = 8;
/** Cap recent messages even if under budget */
export const RECENT_MAX_MESSAGES = 40;

export type ContextLayer = {
  id: string;
  label: string;
  tokens: number;
  truncated?: boolean;
};

export type ContextBuildResult = {
  /** Messages to send (user/assistant only, no system) */
  messages: Array<{ role: "user" | "assistant"; content: string }>;
  /** Extra system/workspace blob */
  workspaceContext: string;
  /** Estimated tokens for the whole request (rough) */
  tokensEst: number;
  budget: number;
  /** 0–100 */
  percent: number;
  layers: ContextLayer[];
  /** Whether a thread summary was injected */
  usedSummary: boolean;
  /** Whether auto-compact should run before next send */
  shouldCompact: boolean;
  droppedMessages: number;
};

/** ~tokens: English ≈ 4 chars/token; code a bit denser */
export function estimateTokens(text: string): number {
  const s = String(text || "");
  if (!s) return 0;
  const code = (s.match(/```[\s\S]*?```/g) || []).join("").length;
  const rest = s.length - code;
  return Math.max(1, Math.ceil(rest / 4 + code / 3.2));
}

export function estimateMessages(
  messages: Array<{ role: string; content: string }>,
): number {
  let n = 0;
  for (const m of messages) {
    n += 4;
    n += estimateTokens(m.content);
  }
  return n;
}

export function trimToolHeavyContent(content: string, maxChars = 8_000): string {
  let s = String(content || "");
  if (s.length <= maxChars) return s;

  s = s.replace(/### (?:🖥️|🔌|🛠️)[\s\S]*?---\s*\n/g, (block) => {
    if (block.length < 800) return block;
    const first = block.slice(0, 320).replace(/\n+/g, " ");
    return `### (tool card trimmed)\n> ${first}…\n\n`;
  });

  s = s.replace(/```(\w*)\n([\s\S]*?)```/g, (_m, lang, body) => {
    const b = String(body);
    if (b.length <= 4_000) return _m;
    const head = b.slice(0, 2_000);
    const tail = b.slice(-1_000);
    return (
      "```" +
      (lang || "") +
      "\n" +
      head +
      "\n\n… [" +
      (b.length - 3_000) +
      " chars trimmed] …\n\n" +
      tail +
      "\n```"
    );
  });

  if (s.length > maxChars) {
    s =
      s.slice(0, Math.floor(maxChars * 0.65)) +
      "\n\n…[message trimmed for context]…\n\n" +
      s.slice(-Math.floor(maxChars * 0.25));
  }
  return s;
}

export function extractFlushFacts(messages: ChatMessage[], maxFacts = 8): string[] {
  const facts: string[] = [];
  const blob = messages
    .filter((m) => m.role === "user" || m.role === "assistant")
    .map((m) => m.content)
    .join("\n");

  const patterns: RegExp[] = [
    /(?:prefer|always|never|don't|do not)\s+[^\n.!?]{8,80}/gi,
    /(?:path|install|root|home)[:\s]+([~/][^\s\n,]{4,80})/gi,
    /(?:decided|decision|we'll|we will|going with)\s+[^\n.!?]{8,100}/gi,
    /(?:version|v\d+\.\d+[.\d]*)\b[^\n]{0,40}/gi,
    /(?:bug|fixed|fix)[:\s]+[^\n.!?]{8,100}/gi,
  ];

  for (const re of patterns) {
    const m = blob.match(re);
    if (!m) continue;
    for (const hit of m) {
      const clean = hit.replace(/\s+/g, " ").trim().slice(0, 140);
      if (clean.length < 12) continue;
      if (facts.some((f) => f.toLowerCase() === clean.toLowerCase())) continue;
      facts.push(clean);
      if (facts.length >= maxFacts) return facts;
    }
  }
  return facts;
}

export function heuristicCompactSummary(
  messages: ChatMessage[],
  title?: string,
): string {
  const users = messages
    .filter((m) => m.role === "user")
    .map((m) =>
      m.content
        .replace(/\[attachment:[^\]]+\]/gi, "")
        .replace(/\s+/g, " ")
        .trim(),
    )
    .filter((s) => s.length > 8);
  const asst = messages
    .filter((m) => m.role === "assistant")
    .map((m) =>
      m.content
        .replace(/```[\s\S]*?```/g, "[code]")
        .replace(/\s+/g, " ")
        .trim()
        .slice(0, 200),
    )
    .filter(Boolean);

  const goals = users.slice(0, 5).map((u, i) => `${i + 1}. ${u.slice(0, 160)}`);
  const outcomes = asst.slice(-4).map((a) => `- ${a.slice(0, 160)}`);
  const facts = extractFlushFacts(messages, 6);

  return [
    `# Thread summary${title ? `: ${title}` : ""}`,
    "",
    "## Goals / asks",
    goals.length ? goals.join("\n") : "- (none captured)",
    "",
    "## Recent outcomes",
    outcomes.length ? outcomes.join("\n") : "- (none)",
    "",
    "## Facts / prefs",
    facts.length ? facts.map((f) => `- ${f}`).join("\n") : "- (none)",
    "",
    `_(Compacted ${messages.length} messages · heuristic)_`,
  ].join("\n");
}

export type CompactResult = {
  summary: string;
  summaryUpToId: string | null;
  compactedAt: number;
  messageCount: number;
  flushFacts: string[];
  tokensEst: number;
};

export function compactMessages(
  messages: ChatMessage[],
  opts?: { keepRecent?: number; title?: string },
): CompactResult | null {
  const keep = Math.max(RECENT_MIN_MESSAGES, opts?.keepRecent ?? 10);
  const usable = messages.filter(
    (m) => (m.role === "user" || m.role === "assistant") && m.content?.trim(),
  );
  if (usable.length <= keep + 2) return null;

  const older = usable.slice(0, usable.length - keep);
  const lastOlder = older[older.length - 1];
  const summary = heuristicCompactSummary(older, opts?.title);
  const flushFacts = extractFlushFacts(older, 8);

  return {
    summary,
    summaryUpToId: lastOlder?.id || null,
    compactedAt: Date.now(),
    messageCount: older.length,
    flushFacts,
    tokensEst: estimateTokens(summary),
  };
}

function messagesAfterSummary(
  messages: ChatMessage[],
  summaryUpToId: string | null | undefined,
): ChatMessage[] {
  if (!summaryUpToId) return messages;
  const idx = messages.findIndex((m) => m.id === summaryUpToId);
  if (idx < 0) return messages;
  return messages.slice(idx + 1);
}

export type BuildContextInput = {
  messages: ChatMessage[];
  thread?: Pick<ChatThread, "title" | "summary" | "summaryUpToId" | "compactedAt"> | null;
  memoryNotes?: string;
  fileMemoryBundle?: string;
  /** Distilled learning / self-improve insights */
  learningBundle?: string;
  projectBundle?: string;
  workboardBundle?: string;
  openClawBundle?: string;
  connectorBlock?: string;
  capabilityBlock?: string;
  budget?: number;
  trimTools?: boolean;
};

export function buildContext(input: BuildContextInput): ContextBuildResult {
  const budget = input.budget ?? CONTEXT_BUDGET_TOKENS;
  const layers: ContextLayer[] = [];

  let pinBudget = PIN_BUDGET_TOKENS;
  const pinParts: string[] = [];

  const pushPin = (id: string, label: string, raw: string, maxTok: number) => {
    const text = String(raw || "").trim();
    if (!text) return;
    let body = text;
    let truncated = false;
    let tok = estimateTokens(body);
    if (tok > maxTok) {
      const maxChars = maxTok * 4;
      body = body.slice(0, maxChars) + "\n…[truncated]…";
      tok = estimateTokens(body);
      truncated = true;
    }
    if (tok > pinBudget) {
      const maxChars = Math.max(200, pinBudget * 4);
      body = body.slice(0, maxChars) + "\n…[truncated]…";
      tok = estimateTokens(body);
      truncated = true;
    }
    pinBudget -= tok;
    pinParts.push(body);
    layers.push({ id, label, tokens: tok, truncated });
  };

  pushPin(
    "file-memory",
    "File memory",
    input.fileMemoryBundle
      ? `## File memory (USER / MEMORY / daily)\n${input.fileMemoryBundle}`
      : "",
    6_000,
  );
  pushPin(
    "learning",
    "Learnings",
    input.learningBundle
      ? input.learningBundle
      : "",
    2_000,
  );
  pushPin(
    "project",
    "Project",
    input.projectBundle || "",
    3_500,
  );
  pushPin(
    "workboard",
    "Workboard",
    input.workboardBundle || "",
    1_500,
  );
  pushPin(
    "memory",
    "Memory notes",
    input.memoryNotes
      ? `## User persistent memory notes\n${input.memoryNotes}`
      : "",
    2_500,
  );
  pushPin(
    "openclaw",
    "OpenClaw workspace",
    input.openClawBundle
      ? `## Imported OpenClaw workspace context\n${input.openClawBundle}`
      : "",
    3_500,
  );
  pushPin("connectors", "Connectors", input.connectorBlock || "", 1_200);
  pushPin("caps", "Capabilities", input.capabilityBlock || "", 800);

  const workspaceContext = pinParts.join("\n\n").trim();
  const pinTokens = layers.reduce((a, l) => a + l.tokens, 0);

  const thread = input.thread;
  const usedSummary = Boolean(thread?.summary?.trim());
  let summaryBlock = "";
  let summaryTokens = 0;
  if (usedSummary && thread?.summary) {
    summaryBlock = [
      "[Prior conversation summary — authoritative for older turns]",
      thread.summary.trim(),
      "[End summary — newer messages follow]",
    ].join("\n");
    summaryTokens = estimateTokens(summaryBlock);
    layers.push({
      id: "summary",
      label: "Thread summary",
      tokens: summaryTokens,
    });
  }

  let recentSource = messagesAfterSummary(
    input.messages.filter((m) => m.role === "user" || m.role === "assistant"),
    thread?.summaryUpToId,
  );
  recentSource = recentSource.filter((m) => m.content && m.content.trim().length > 0);

  const replyBudget = Math.max(8_000, budget - pinTokens - summaryTokens - 6_000);

  const selected: Array<{ role: "user" | "assistant"; content: string }> = [];
  let used = 0;
  let dropped = 0;

  const reversed = [...recentSource].reverse();
  const keptRev: typeof selected = [];
  for (let i = 0; i < reversed.length; i++) {
    const m = reversed[i]!;
    const isRecentTail = i < RECENT_MIN_MESSAGES;
    let content = m.content;
    if (input.trimTools !== false && !isRecentTail) {
      content = trimToolHeavyContent(content, 6_000);
    } else if (input.trimTools !== false && content.length > 24_000) {
      content = trimToolHeavyContent(content, 16_000);
    }
    const tok = estimateTokens(content) + 4;
    if (!isRecentTail && used + tok > replyBudget) {
      dropped += 1;
      continue;
    }
    if (!isRecentTail && keptRev.length >= RECENT_MAX_MESSAGES) {
      dropped += 1;
      continue;
    }
    keptRev.push({
      role: m.role as "user" | "assistant",
      content,
    });
    used += tok;
  }
  selected.push(...keptRev.reverse());

  if (summaryBlock) {
    selected.unshift({
      role: "user",
      content: summaryBlock,
    });
    selected.splice(1, 0, {
      role: "assistant",
      content: "Understood — I have the prior summary and will use it.",
    });
  }

  const msgTokens = estimateMessages(selected);
  const tokensEst = pinTokens + msgTokens + 800;
  layers.push({
    id: "messages",
    label: `Messages (${selected.length})`,
    tokens: msgTokens,
  });

  const percent = Math.min(100, Math.round((tokensEst / budget) * 100));
  const shouldCompact =
    percent >= COMPACT_THRESHOLD * 100 ||
    dropped > 0 ||
    recentSource.length > RECENT_MAX_MESSAGES + 4;

  return {
    messages: selected,
    workspaceContext,
    tokensEst,
    budget,
    percent,
    layers,
    usedSummary,
    shouldCompact,
    droppedMessages: dropped,
  };
}

export function estimateThreadContextPercent(
  messages: ChatMessage[],
  thread?: Pick<ChatThread, "summary" | "summaryUpToId" | "title" | "compactedAt"> | null,
  memoryNotes?: string,
): { percent: number; tokensEst: number; budget: number; shouldCompact: boolean } {
  const built = buildContext({
    messages,
    thread: thread || null,
    memoryNotes,
    trimTools: true,
  });
  return {
    percent: built.percent,
    tokensEst: built.tokensEst,
    budget: built.budget,
    shouldCompact: built.shouldCompact,
  };
}

export function formatContextReport(built: ContextBuildResult): string {
  const lines = [
    `**Context** · ~${built.tokensEst.toLocaleString()} / ${built.budget.toLocaleString()} tokens (**${built.percent}%**)`,
    "",
    ...built.layers.map(
      (l) =>
        `- ${l.label}: ~${l.tokens.toLocaleString()} tok${l.truncated ? " _(truncated)_" : ""}`,
    ),
  ];
  if (built.usedSummary) lines.push("- Thread summary: **active**");
  if (built.droppedMessages)
    lines.push(`- Dropped from window: **${built.droppedMessages}** older msgs`);
  if (built.shouldCompact)
    lines.push("- Status: **over budget** — run `/compact` or send to auto-compact");
  else lines.push("- Status: within budget");
  lines.push("", "_Compaction trims the API window only; full chat stays visible._");
  lines.push("_File memory (USER.md / MEMORY.md / daily) is pinned when present._");
  return lines.join("\n");
}

export function mergeFlushIntoMemory(
  existing: string,
  facts: string[],
  maxLines = 80,
): string {
  if (!facts.length) return existing || "";
  const date = new Date().toISOString().slice(0, 10);
  const lines = (existing || "")
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  const lower = new Set(lines.map((l) => l.toLowerCase()));
  for (const f of facts) {
    const line = `- ${date}: ${f}`;
    if (lower.has(line.toLowerCase())) continue;
    if ([...lower].some((l) => l.includes(f.toLowerCase().slice(0, 40)))) continue;
    lines.push(line);
    lower.add(line.toLowerCase());
  }
  return lines.slice(-maxLines).join("\n");
}
