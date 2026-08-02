/**
 * Predictive quick-assistant chips for the Agent composer.
 * Context-aware (code / app / host / imagine) + adaptive memory + dismiss/rotate.
 */
import type {
  ActivityItem,
  ChatMessage,
  ChatThread,
  Connector,
  GrokModeId,
  UsageSnapshot,
} from "./types";
import { PLAN_LIMITS, usagePercent } from "./usage";
import {
  applyMemoryToChips,
  type QuickAssistMemory,
} from "./quick-assist-memory";

export type QuickChipKind = "chat" | "shell" | "nav" | "mode";

export type QuickChip = {
  id: string;
  /** Short label shown on the chip */
  label: string;
  /** Full value sent / navigated */
  value: string;
  kind: QuickChipKind;
  score: number;
  /** Optional secondary hint (not always shown) */
  hint?: string;
};

export type QuickAssistantInput = {
  chat: ChatMessage[];
  activity: ActivityItem[];
  threads: ChatThread[];
  connectors: Connector[];
  mode: GrokModeId;
  grokConnected: boolean | null;
  usage: UsageSnapshot;
  /** Current composer draft — used for predictive filtering */
  draft?: string;
  hostOnline?: boolean;
  max?: number;
  /** Adaptive history from prior chip clicks + typed prompts */
  memory?: QuickAssistMemory | null;
  /** Chip values the user dismissed (hidden until rotate/suggest) */
  dismissed?: string[];
  /** Increments to rotate alternate chip packs after clicks / Suggest */
  rotation?: number;
};

const MAX_DEFAULT = 8;
const MAX_HARD = 10;

function recentUserMessages(chat: ChatMessage[], n = 12): string[] {
  return chat
    .filter((m) => m.role === "user")
    .slice(-n)
    .map((m) => m.content.trim())
    .filter(Boolean)
    .reverse();
}

function lastAssistant(chat: ChatMessage[]): string {
  for (let i = chat.length - 1; i >= 0; i--) {
    if (chat[i]?.role === "assistant" && chat[i]!.content?.trim()) {
      return chat[i]!.content;
    }
  }
  return "";
}

function uniqByValue(chips: QuickChip[]): QuickChip[] {
  const seen = new Set<string>();
  const out: QuickChip[] = [];
  for (const c of chips) {
    const key = c.value.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(c);
  }
  return out;
}

function shorten(s: string, n = 42): string {
  const t = s.replace(/\s+/g, " ").trim();
  if (t.length <= n) return t;
  return t.slice(0, n - 1) + "…";
}

function chatBlob(chat: ChatMessage[], n = 8): string {
  return chat
    .slice(-n)
    .map((m) => m.content)
    .join("\n")
    .toLowerCase();
}

/** Detect conversation context for chip packs. */
export function detectChipContext(chat: ChatMessage[]): {
  code: boolean;
  app: boolean;
  host: boolean;
  imagine: boolean;
  error: boolean;
  ui: boolean;
} {
  const users = recentUserMessages(chat, 6).join("\n");
  const asst = lastAssistant(chat);
  const blob = `${users}\n${asst}`.toLowerCase();
  const hasCodeFence = /```[\s\S]{12,}/.test(`${users}\n${asst}`);
  const code =
    hasCodeFence ||
    /\b(function|const |let |class |import |export |def |fn |package |#include|console\.|npm |cargo |rust|typescript|python|jsx|tsx|css|html)\b/i.test(
      blob,
    ) ||
    /review this code|refactor|compile|typecheck|stack.?trace/i.test(blob);
  const app =
    /\b(grokhub|this app|the app|electron|desktop app|sidebar|composer|quick assist|usage meter|oauth|imagine tab|agent chat|settings)\b/i.test(
      blob,
    ) || /improve (the )?ui|fix this bug|add feature/i.test(blob);
  const host =
    /\$ |HOST_CMD|desktop host|shell|uname|ls -|cwd|filesystem|cli/i.test(blob);
  const imagine =
    /\b(imagine|image|generate (a |an )?(pic|image|logo|icon)|draw |video)\b/i.test(
      blob,
    );
  const error =
    /\b(error|bug|fail|broken|crash|exception|doesn't work|not working|typeerror|eacces)\b/i.test(
      blob,
    );
  const ui =
    /\b(ui|layout|button|input|sidebar|theme|dark mode|spacing|scrollbar|modal|chip)\b/i.test(
      blob,
    );
  return { code, app, host, imagine, error, ui };
}

function pack(
  items: Array<Omit<QuickChip, "score"> & { score?: number }>,
  base: number,
): QuickChip[] {
  return items.map((it, i) => ({
    ...it,
    score: (it.score ?? base) - i * 0.5,
  }));
}

function codeChips(rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        { id: "code-comments", label: "Add comments", value: "Add clear comments to the code we just discussed — keep them concise.", kind: "chat" },
        { id: "code-optimize", label: "Optimize this", value: "Optimize the code we discussed for performance and readability. Show before/after notes.", kind: "chat" },
        { id: "code-bugs", label: "Find bugs", value: "Review the recent code for bugs, edge cases, and race conditions. List issues by severity.", kind: "chat" },
        { id: "code-python", label: "Convert to Python", value: "Convert the recent code to clean, idiomatic Python 3.", kind: "chat" },
        { id: "code-review", label: "Review this code", value: "Do a thorough code review of what we just worked on. Be specific.", kind: "chat" },
        { id: "code-tests", label: "Add tests", value: "Suggest unit tests for the code we discussed, including edge cases.", kind: "chat" },
      ],
      92,
    ),
    pack(
      [
        { id: "code-types", label: "Add types", value: "Add or improve TypeScript types for the code we discussed.", kind: "chat" },
        { id: "code-errors", label: "Better error handling", value: "Improve error handling in the recent code — user-friendly messages, no silent failures.", kind: "chat" },
        { id: "code-refactor", label: "Refactor cleanly", value: "Refactor the recent code for clarity without changing behavior.", kind: "chat" },
        { id: "code-secure", label: "Security pass", value: "Security review of the recent code (injection, XSS, path traversal, secrets).", kind: "chat" },
        { id: "code-docs", label: "Write docs", value: "Write short docs / README notes for the code we discussed.", kind: "chat" },
        { id: "code-mode", label: "Switch to Build", value: "__mode:build", kind: "mode", hint: "mode" },
      ],
      90,
    ),
  ];
  return packs[rotation % packs.length]!;
}

function appChips(rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        { id: "app-ui", label: "Improve this UI", value: "Improve this UI — clearer hierarchy, spacing, and less beta feel. Be concrete.", kind: "chat" },
        { id: "app-faster", label: "Make this faster", value: "Find and fix performance bottlenecks in what we just discussed. Prioritize biggest wins.", kind: "chat" },
        { id: "app-dark", label: "Add dark mode", value: "Audit dark-mode contrast and polish any hard-to-read areas (scrollbars, chips, meters).", kind: "chat" },
        { id: "app-input", label: "Fix the input box", value: "Improve the chat input box: attach, voice, resize, and keyboard UX.", kind: "chat" },
        { id: "app-errors", label: "Better error handling", value: "Replace bare error dumps with clear user-facing errors and recovery actions.", kind: "chat" },
        { id: "app-keys", label: "Add keyboard shortcuts", value: "Add useful keyboard shortcuts and document them in the UI.", kind: "chat" },
      ],
      94,
    ),
    pack(
      [
        { id: "app-feature", label: "Add feature", value: "Propose the highest-value feature we should add next to GrokHub, then implement a first slice.", kind: "chat" },
        { id: "app-bug", label: "Fix this bug", value: "Diagnose and fix the bug we were just talking about. Confirm with a clear retest checklist.", kind: "chat" },
        { id: "app-usage", label: "Fix usage meter", value: "Make the usage meter show accurate grok.com subscription limits and update every minute.", kind: "chat" },
        { id: "app-chips", label: "Smarter quick chips", value: "Improve quick-assist chips: more context-aware, actionable, and less truncation.", kind: "chat" },
        { id: "app-a11y", label: "Accessibility pass", value: "Accessibility pass on the current screen: focus, labels, contrast, keyboard.", kind: "chat" },
        { id: "app-mobile", label: "Mobile layout", value: "Tighten the layout for small windows and ~390px mobile widths.", kind: "chat" },
      ],
      93,
    ),
  ];
  return packs[rotation % packs.length]!;
}

function uiChips(): QuickChip[] {
  return pack(
    [
      { id: "ui-hierarchy", label: "Cleaner hierarchy", value: "Improve visual hierarchy of the last screen we discussed (sidebar, headers, lists).", kind: "chat" },
      { id: "ui-spacing", label: "Fix spacing", value: "Tighten inconsistent spacing and align the layout to a consistent scale.", kind: "chat" },
      { id: "ui-loading", label: "Loading states", value: "Add clear loading / progress states so nothing feels frozen.", kind: "chat" },
    ],
    86,
  );
}

function hostChips(): QuickChip[] {
  return pack(
    [
      { id: "host-status", label: "Host status", value: "$ uname -a && whoami && pwd && df -h | head -8", kind: "shell" },
      { id: "host-files", label: "List project files", value: "List the important files in this project and summarize structure.", kind: "chat" },
      { id: "host-procs", label: "Top processes", value: "$ ps aux --sort=-%mem | head -12", kind: "shell" },
    ],
    70,
  );
}

function defaultChips(planLabel: string, mode: GrokModeId, rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        { id: "def-improve-ui", label: "Improve this UI", value: "Improve this UI — clearer hierarchy, spacing, and polish. Be concrete.", kind: "chat" },
        { id: "def-faster", label: "Make this faster", value: "Find easy wins to make GrokHub feel snappier.", kind: "chat" },
        { id: "def-dark", label: "Add dark mode polish", value: "Polish dark mode contrast: scrollbars, chips, code blocks, meters.", kind: "chat" },
        { id: "def-input", label: "Fix the input box", value: "Improve the agent input box UX (attach, voice, resize, shortcuts).", kind: "chat" },
        { id: "def-errors", label: "Better error handling", value: "Improve error handling across chat and tools — clear messages, recovery tips.", kind: "chat" },
        { id: "def-keys", label: "Add keyboard shortcuts", value: "Add and surface keyboard shortcuts for power users.", kind: "chat" },
        { id: "def-review", label: "Review this code", value: "Review the code we last discussed and suggest concrete improvements.", kind: "chat" },
      ],
      22,
    ),
    pack(
      [
        { id: "def-feature", label: "Suggest a feature", value: "Suggest the next high-impact GrokHub feature and a minimal implementation plan.", kind: "chat" },
        { id: "def-usage", label: "My usage", value: `What's my usage right now? (${planLabel})`, kind: "chat" },
        { id: "def-imagine", label: "Open Imagine", value: "__nav:imagine", kind: "nav" },
        {
          id: "def-auto",
          label: mode === "auto" ? "How Auto routes" : "Use Auto mode",
          value:
            mode === "auto"
              ? "How does Auto choose models for my prompts?"
              : "__mode:auto",
          kind: mode === "auto" ? "chat" : "mode",
        },
        { id: "def-help", label: "What can you help with?", value: "What can you help me with in GrokHub right now?", kind: "chat" },
        { id: "def-modes", label: "Explain modes", value: "Explain Auto / Fast / Expert / Heavy / Build and when to use each.", kind: "chat" },
      ],
      20,
    ),
  ];
  return packs[rotation % packs.length]!;
}

/**
 * Build ranked quick-assistant chips.
 * Always returns ≤ max (default 8, hard cap 10).
 */
export function buildQuickChips(input: QuickAssistantInput): QuickChip[] {
  const max = Math.min(input.max ?? MAX_DEFAULT, MAX_HARD);
  const rotation = Math.max(0, Number(input.rotation) || 0);
  const dismissed = new Set(
    (input.dismissed || []).map((v) => v.trim().toLowerCase()).filter(Boolean),
  );
  const chips: QuickChip[] = [];
  const users = recentUserMessages(input.chat);
  const lastUser = users[0] || "";
  const pct = Math.round(usagePercent(input.usage));
  const plan = PLAN_LIMITS[input.usage.plan];
  const recentActivity = input.activity.slice(0, 12);
  const draft = (input.draft || "").trim().toLowerCase();
  const ctx = detectChipContext(input.chat);

  // ── Connection / host context ─────────────────────────────────────────
  if (!input.grokConnected) {
    chips.push({
      id: "ctx-connect",
      label: "Connect Grok",
      value: "__nav:settings",
      kind: "nav",
      score: 100,
      hint: "oauth",
    });
  }
  if (input.hostOnline === false) {
    chips.push({
      id: "ctx-host",
      label: "Connect desktop host",
      value: "__nav:settings",
      kind: "nav",
      score: 95,
    });
  }
  if (pct >= 80) {
    chips.push({
      id: "ctx-quota",
      label: `Usage ${pct}% — save units`,
      value: "What's my usage and how can I save units?",
      kind: "chat",
      score: 85,
    });
  }

  // ── Biggest win: context packs ────────────────────────────────────────
  if (ctx.code) chips.push(...codeChips(rotation));
  if (ctx.app) chips.push(...appChips(rotation));
  if (ctx.ui && !ctx.app) chips.push(...uiChips());
  if (ctx.host || input.hostOnline) chips.push(...hostChips());
  if (ctx.imagine) {
    chips.push({
      id: "ctx-imagine",
      label: "Open Imagine",
      value: "__nav:imagine",
      kind: "nav",
      score: 88,
    });
  }
  if (ctx.error && !ctx.code) {
    chips.push(
      ...pack(
        [
          {
            id: "err-diagnose",
            label: "Diagnose this error",
            value: "Diagnose the error we hit — root cause, fix, and how to verify.",
            kind: "chat",
          },
          {
            id: "err-retry",
            label: "Retry last ask",
            value: lastUser || "Try that again carefully",
            kind: "chat",
          },
        ],
        84,
      ),
    );
  }

  // ── Recent follow-ups (light) ─────────────────────────────────────────
  if (lastUser && lastUser.length < 100) {
    chips.push({
      id: "recent-continue",
      label: shorten(`Continue: ${lastUser}`, 40),
      value: `Continue from: ${lastUser}`,
      kind: "chat",
      score: 40,
      hint: "recent",
    });
  }

  // ── Activity signals ──────────────────────────────────────────────────
  for (const a of recentActivity.slice(0, 6)) {
    if (a.kind === "chat" && a.status === "failed" && lastUser) {
      chips.push({
        id: `act-retry-${a.id}`,
        label: "Retry last ask",
        value: lastUser,
        kind: "chat",
        score: 72,
      });
    }
    if (a.kind === "imagine") {
      chips.push({
        id: `act-img-${a.id}`,
        label: "Imagine again",
        value: "__nav:imagine",
        kind: "nav",
        score: 50,
      });
    }
  }

  // ── Defaults when context is thin ─────────────────────────────────────
  const hasStrongContext = ctx.code || ctx.app || ctx.ui || ctx.error;
  if (!hasStrongContext || chips.length < 4) {
    chips.push(...defaultChips(plan.label, input.mode, rotation));
  } else {
    // Still offer a couple of high-value app defaults at lower score
    chips.push(
      ...defaultChips(plan.label, input.mode, rotation).map((c) => ({
        ...c,
        score: Math.min(c.score, 18),
      })),
    );
  }

  // ── Adaptive memory ───────────────────────────────────────────────────
  let withMemory = applyMemoryToChips(chips, input.memory);

  // Dismiss filter
  withMemory = withMemory.filter(
    (c) => !dismissed.has(c.value.trim().toLowerCase()) && !dismissed.has(c.id),
  );

  // Don't suggest the exact draft text as a chip
  let ranked = uniqByValue(withMemory)
    .filter((c) => c.value.trim().toLowerCase() !== draft)
    .sort((a, b) => b.score - a.score);

  // Rotation nudge: slightly demote previously top ids when rotation > 0
  if (rotation > 0) {
    ranked = ranked
      .map((c, i) => ({
        ...c,
        score: c.score + ((i + rotation) % 5 === 0 ? 8 : 0) - (i < 2 ? 6 : 0),
      }))
      .sort((a, b) => b.score - a.score);
  }

  // ── Predictive filter from draft ──────────────────────────────────────
  if (draft.length >= 1) {
    const pred = ranked
      .map((c) => {
        const hay = `${c.label} ${c.value}`.toLowerCase();
        let boost = 0;
        if (hay.startsWith(draft)) boost += 40;
        else if (hay.includes(draft)) boost += 25;
        for (const tok of draft.split(/\s+/)) {
          if (tok.length > 2 && hay.includes(tok)) boost += 8;
        }
        if (draft.startsWith("$") && c.kind === "shell") boost += 30;
        if (/imagine|draw|image/.test(draft) && c.value.includes("imagine")) boost += 35;
        if (/bug|error|fix/.test(draft) && /bug|error|fix|diagnos/i.test(hay)) boost += 20;
        if (/code|refactor|test/.test(draft) && c.id.startsWith("code-")) boost += 22;
        return { ...c, score: c.score + boost };
      })
      .filter((c) => {
        const hay = `${c.label} ${c.value}`.toLowerCase();
        if (draft.length < 2) return true;
        return (
          c.score >= 80 ||
          hay.includes(draft) ||
          draft.split(/\s+/).some((tok) => tok.length > 2 && hay.includes(tok))
        );
      })
      .sort((a, b) => b.score - a.score);

    ranked = pred.length ? pred : ranked;
  }

  // Prefer a mix: not all shell, not all nav
  const picked: QuickChip[] = [];
  const kindCount: Record<string, number> = {};
  for (const c of ranked) {
    if (picked.length >= max) break;
    const k = c.kind;
    const n = kindCount[k] || 0;
    if (k === "shell" && n >= 2) continue;
    if (k === "nav" && n >= 2) continue;
    if (k === "mode" && n >= 1) continue;
    picked.push(c);
    kindCount[k] = n + 1;
  }

  if (picked.length < Math.min(5, max)) {
    for (const c of ranked) {
      if (picked.length >= max) break;
      if (picked.some((p) => p.id === c.id)) continue;
      picked.push(c);
    }
  }

  return picked.slice(0, max);
}

/** Generate a fresh alternate pack (for Suggest chips). */
export function suggestMoreChips(
  input: QuickAssistantInput,
  extraRotation = 1,
): QuickChip[] {
  return buildQuickChips({
    ...input,
    rotation: (input.rotation || 0) + extraRotation,
    max: input.max ?? 8,
  });
}
