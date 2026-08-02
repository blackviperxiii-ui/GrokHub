/**
 * Optional LLM-assisted thread compact + learning reflect when online.
 * Falls back to heuristics offline.
 */
import type { ChatMessage } from "./types";
import {
  compactMessages,
  extractFlushFacts,
  type CompactResult,
} from "./context-manager";
import {
  reflectLearning,
  upsertInsight,
  type LearningState,
} from "./learning";

async function tryGrokComplete(
  system: string,
  user: string,
  opts: {
    modelId?: string;
    maxTokens?: number;
    bearer?: string;
    apiKey?: string;
  },
): Promise<string | null> {
  try {
    const { callXaiChat } = await import("./grok");
    const r = await callXaiChat({
      model: opts.modelId || "grok-4-1-fast-non-reasoning",
      mode: "fast",
      workspaceContext: system,
      messages: [{ role: "user", content: user }],
      maxTokens: opts.maxTokens || 800,
      temperature: 0.3,
      accessToken: opts.bearer,
      apiKey: opts.apiKey,
    });
    if (!r.ok || !r.content) return null;
    return r.content.trim() || null;
  } catch {
    return null;
  }
}

export async function compactMessagesSmart(
  messages: ChatMessage[],
  opts: {
    keepRecent?: number;
    title?: string;
    online?: boolean;
    modelId?: string;
    bearer?: string;
    apiKey?: string;
  },
): Promise<CompactResult | null> {
  const base = compactMessages(messages, {
    keepRecent: opts.keepRecent,
    title: opts.title,
  });
  if (!base) return null;
  if (!opts.online) return base;

  const keep = Math.max(8, opts.keepRecent ?? 10);
  const usable = messages.filter(
    (m) => (m.role === "user" || m.role === "assistant") && m.content?.trim(),
  );
  const older = usable.slice(0, Math.max(0, usable.length - keep));
  const digest = older
    .map((m) => `${m.role}: ${m.content.replace(/\s+/g, " ").trim().slice(0, 400)}`)
    .join("\n")
    .slice(0, 12_000);

  const llm = await tryGrokComplete(
    "You compress chat history into a durable summary for future context. Output markdown with: Goals, Decisions, Open items, Facts/prefs, Files/paths. Max 400 words. No fluff.",
    `Thread: ${opts.title || "Chat"}\n\n${digest}`,
    {
      modelId: opts.modelId,
      maxTokens: 700,
      bearer: opts.bearer,
      apiKey: opts.apiKey,
    },
  );

  if (!llm || llm.length < 40) return base;

  return {
    ...base,
    summary: llm + "\n\n_(LLM compact)_",
    tokensEst: Math.ceil(llm.length / 4),
    flushFacts: extractFlushFacts(older, 8),
  };
}

export async function reflectLearningSmart(
  state: LearningState,
  opts?: {
    online?: boolean;
    modelId?: string;
    bearer?: string;
    apiKey?: string;
  },
): Promise<ReturnType<typeof reflectLearning>> {
  const base = reflectLearning(state);
  if (!opts?.online) return base;

  const blob = base.markdown.slice(0, 6_000);
  const llm = await tryGrokComplete(
    "Distill user preferences and agent lessons into 6-10 short bullet insights. Each line starts with '- '. No intro.",
    blob,
    {
      modelId: opts.modelId,
      maxTokens: 400,
      bearer: opts.bearer,
      apiKey: opts.apiKey,
    },
  );
  if (!llm) return base;

  let next = base.state;
  for (const line of llm.split("\n")) {
    const t = line.replace(/^[-*•]\s*/, "").trim();
    if (t.length < 12) continue;
    next = upsertInsight(next, {
      key: `llm:${t.toLowerCase().slice(0, 40)}`,
      text: t.slice(0, 280),
      confidence: 0.65,
      source: "distill",
    });
  }
  const md =
    base.markdown +
    "\n## LLM distillation\n" +
    llm
      .split("\n")
      .filter((l) => l.trim())
      .slice(0, 12)
      .join("\n") +
    "\n";
  return { state: next, markdown: md, newInsights: base.newInsights };
}
