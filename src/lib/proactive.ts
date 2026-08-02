/**
 * Proactive autonomy — self-awareness & small unsolicited fixes.
 * Not a job dashboard: heal stuck UI, incomplete turns, and soft app glitches.
 */
import type { ChatMessage, ChatThread } from "./types";
import { looksLikeIncompleteAgentTurn } from "./agent-finish";
import type { AutonomyConfig } from "./agent-jobs";

export type ProactiveAction = {
  id: string;
  kind:
    | "clear_orphan_stream"
    | "finalize_stuck_stream"
    | "auto_continue"
    | "clear_empty_assistant"
    | "note";
  title: string;
  detail: string;
  /** Safe to apply without asking */
  auto: boolean;
  threadId?: string | null;
  messageId?: string;
};

export type ProactiveScanInput = {
  autonomy: AutonomyConfig;
  running: boolean;
  streamingMessageId: string | null;
  streamStatus: string | null;
  chat: ChatMessage[];
  threads: ChatThread[];
  activeThreadId: string | null;
  /** ms since stream started if known */
  streamStartedAt?: number | null;
  now?: number;
};

const STUCK_STREAM_MS = 90_000;
const AUTO_CONTINUE_COOLDOWN_MS = 5 * 60_000;
const lastAutoContinue = new Map<string, number>();

export function proactiveEnabled(cfg: AutonomyConfig): boolean {
  return !cfg.paused && cfg.level >= 1;
}

/** Level 2+ may auto-continue incomplete answers once. */
export function canAutoContinue(cfg: AutonomyConfig): boolean {
  return proactiveEnabled(cfg) && cfg.level >= 2;
}

/** Level 3+ more aggressive self-heal without prompts. */
export function canAggressiveHeal(cfg: AutonomyConfig): boolean {
  return proactiveEnabled(cfg) && cfg.level >= 3;
}

/**
 * Scan live UI/chat state for small problems the agent can fix without a user prompt.
 */
export function scanProactiveIssues(input: ProactiveScanInput): ProactiveAction[] {
  const now = input.now ?? Date.now();
  const cfg = input.autonomy;
  if (!proactiveEnabled(cfg)) return [];

  const actions: ProactiveAction[] = [];
  const chat = input.chat || [];
  const tid = input.activeThreadId;

  // Orphan streaming flags: message says streaming but turn is idle
  if (!input.running) {
    for (const m of chat) {
      if (m.role === "assistant" && m.streaming) {
        actions.push({
          id: `orphan-${m.id}`,
          kind: "clear_orphan_stream",
          title: "Clear stuck streaming indicator",
          detail: "An assistant bubble was still marked streaming after the turn ended.",
          auto: true,
          threadId: tid,
          messageId: m.id,
        });
      }
    }
    if (input.streamingMessageId) {
      actions.push({
        id: `sid-${input.streamingMessageId}`,
        kind: "finalize_stuck_stream",
        title: "Release stream lock",
        detail: "Streaming message id was left set while the agent was idle.",
        auto: true,
        threadId: tid,
        messageId: input.streamingMessageId,
      });
    }
  }

  // Long-running stream with no progress marker (best-effort)
  if (
    input.running &&
    input.streamingMessageId &&
    input.streamStartedAt &&
    now - input.streamStartedAt > STUCK_STREAM_MS &&
    canAggressiveHeal(cfg)
  ) {
    actions.push({
      id: `stuck-${input.streamingMessageId}`,
      kind: "finalize_stuck_stream",
      title: "Unstick long stream",
      detail: "Stream ran unusually long with no completion — finalize so you can keep chatting.",
      auto: true,
      threadId: tid,
      messageId: input.streamingMessageId,
    });
  }

  // Empty / hollow assistant at end of idle chat
  if (!input.running && !input.streamingMessageId) {
    const last = [...chat].reverse().find((m) => m.role === "assistant");
    const lastUser = [...chat].reverse().find((m) => m.role === "user");
    if (last && (!last.content || last.content === "(empty)" || last.content.trim() === "_Stopped._")) {
      actions.push({
        id: `empty-${last.id}`,
        kind: "clear_empty_assistant",
        title: "Clean empty reply",
        detail: "Removed a hollow assistant placeholder.",
        auto: true,
        threadId: tid,
        messageId: last.id,
      });
    }
    // Auto-continue incomplete “let me check…” once per thread (level 2+)
    if (
      last &&
      lastUser &&
      canAutoContinue(cfg) &&
      !last.streaming &&
      looksLikeIncompleteAgentTurn(last.content || "", {
        userPrompt: lastUser.content,
        hadTools: /HOST_RESULT|CONNECTOR_RESULT/i.test(last.content || ""),
      })
    ) {
      const key = tid || "active";
      const prev = lastAutoContinue.get(key) || 0;
      if (now - prev >= AUTO_CONTINUE_COOLDOWN_MS) {
        actions.push({
          id: `continue-${last.id}`,
          kind: "auto_continue",
          title: "Continue incomplete answer",
          detail: "Last reply looked unfinished — continuing without waiting for a nudge.",
          auto: true,
          threadId: tid,
          messageId: last.id,
        });
      }
    }
  }

  return actions;
}

export function markAutoContinue(threadId: string | null | undefined, now = Date.now()) {
  lastAutoContinue.set(threadId || "active", now);
}

/** Short system-prompt add-on when proactive levels are on. */
export function proactiveSystemAddon(cfg: AutonomyConfig): string {
  if (!proactiveEnabled(cfg)) return "";
  if (cfg.level >= 3) {
    return `
## Proactive mode (on)
You notice problems and act without waiting to be asked for tiny fixes:
- If a prior turn stalled (“let me check…”) or a chat/UI glitch is obvious, finish or correct it.
- Prefer real HOST_CMD for local bugs over describing plans.
- For small clear corrections (typos in files you just wrote, failed command retry once, stuck state), just do them and say what you fixed in one short line.
- Do not start large new projects unprompted. Stay on the user's current goals.
- If something needs a destructive or irreversible action, ask first.
`;
  }
  if (cfg.level >= 2) {
    return `
## Proactive mode (helpful)
Be slightly self-aware: if your last answer was incomplete or a tool failed, continue/fix it without waiting for “please continue”.
Call out brief self-corrections. Don't invent new multi-step projects unprompted.
`;
  }
  return `
## Awareness mode
If the UI or your last reply is stuck/empty, prefer finishing cleanly over meta apologies. Still wait for the user for new work.
`;
}
