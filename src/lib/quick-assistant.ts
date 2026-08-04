/**
 * Predictive quick-assistant chips for the Agent composer.
 * Stage-aware · last-message · draft/intent predictive · habit transitions · success learning.
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
  memoryBoostForContext,
  type QuickAssistMemory,
} from "./quick-assist-memory";
import {
  applyIntentBoost,
  applyPredictiveDraftBoost,
  collectPredictiveChips,
  topIntentLabel,
} from "./quick-assist-predict";

export type QuickChipKind = "chat" | "shell" | "nav" | "mode";

export type QuickChip = {
  id: string;
  /** Short label shown on the chip (outcome-first) */
  label: string;
  /** Full value sent / navigated */
  value: string;
  kind: QuickChipKind;
  score: number;
  /** Why this chip is shown (tooltip) */
  hint?: string;
  /** Highest-ranked “do this next” chip */
  primary?: boolean;
};

export type QuickAssistantInput = {
  chat: ChatMessage[];
  activity: ActivityItem[];
  threads: ChatThread[];
  connectors: Connector[];
  mode: GrokModeId;
  grokConnected: boolean | null;
  usage: UsageSnapshot;
  draft?: string;
  hostOnline?: boolean;
  max?: number;
  memory?: QuickAssistMemory | null;
  dismissed?: string[];
  rotation?: number;
  /** Active thread title for topic chips */
  threadTitle?: string | null;
  /** Fast-mode LLM-generated chips for this context */
  llmChips?: QuickChip[] | null;
  /** Context fingerprint for memory boost */
  contextTag?: string | null;
};

/** Visible default — keep the dock clean; hard cap still 10 for refresh packs */
const MAX_DEFAULT = 5;
const MAX_HARD = 8;

export type ChipStage =
  | "empty"
  | "mid"
  | "error"
  | "tools"
  | "long"
  | "default";

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

function shorten(s: string, n = 36): string {
  const t = s.replace(/\s+/g, " ").trim();
  if (t.length <= n) return t;
  const cut = t.slice(0, n - 1);
  const sp = cut.lastIndexOf(" ");
  return (sp > 12 ? cut.slice(0, sp) : cut).trimEnd() + "…";
}

/** Pull a short topic phrase from assistant text (for labels). */
function topicFromText(text: string, maxWords = 4): string {
  const plain = text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`[^`]+`/g, " ")
    .replace(/\[.*?\]\(.*?\)/g, " ")
    .replace(/[#>*_\-]/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  if (!plain) return "";
  // Prefer a mid sentence that isn't pure boilerplate
  const parts = plain
    .split(/[.!?\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 16)
    .filter(
      (s) =>
        !/^(here is|here'?s|sure|okay|got it|i('ll| will)|looking at)/i.test(s),
    );
  const sentence = parts[0] || plain;
  const stop = new Set(
    "a an the and or but if then so to of in on for with from at by as is are was were be i me my we you your it its this that these those here bug error crashes crash failed".split(
      " ",
    ),
  );
  const words = sentence
    .replace(/[^a-z0-9.\-_\s]/gi, " ")
    .split(/\s+/)
    .filter((w) => w.length > 2 && !stop.has(w.toLowerCase()));
  return words.slice(0, maxWords).join(" ");
}


export function detectChipContext(chat: ChatMessage[]): {
  code: boolean;
  app: boolean;
  host: boolean;
  imagine: boolean;
  error: boolean;
  ui: boolean;
  decide: boolean;
  implement: boolean;
  incomplete: boolean;
} {
  const users = recentUserMessages(chat, 6).join("\n");
  const asst = lastAssistant(chat);
  const blob = `${users}\n${asst}`.toLowerCase();
  const hasCodeFence = /```[\s\S]{12,}/.test(`${users}\n${asst}`);
  const code =
    hasCodeFence ||
    /\b(function|const |let |class |import |export |def |fn |package |typescript|python|jsx|tsx)\b/i.test(
      blob,
    ) ||
    /review this code|refactor|typecheck|stack.?trace/i.test(blob);
  const app =
    /\b(grokhub|this app|the app|electron|desktop app|sidebar|composer|quick assist|usage meter|oauth|imagine tab)\b/i.test(
      blob,
    ) || /improve (the )?ui|fix this bug|add feature/i.test(blob);
  const host =
    /\$ |HOST_CMD|desktop host|shell|uname|ls -|filesystem|cli|journalctl|ps aux/i.test(blob);
  const imagine =
    /\b(imagine|image|generate (a |an )?(pic|image|logo|icon)|draw |video)\b/i.test(blob);
  const error =
    /\b(error|bug|fail|broken|crash|exception|doesn't work|not working|typeerror|eacces|couldn't complete)\b/i.test(
      blob,
    );
  const ui =
    /\b(ui|layout|button|input|sidebar|theme|dark mode|spacing|scrollbar|modal|chip)\b/i.test(
      blob,
    );
  const decide =
    /\b(should i|which|options?|tradeoff|recommend|compare|vs\.?|better)\b/i.test(blob);
  const implement =
    /\b(implement|add |build |create |wire |ship |write the|patch|apply)\b/i.test(users);
  const incomplete =
    /\b(i('ll| will)|let me|next i|continuing|in progress|still need|want me to|shall i)\b/i.test(
      asst,
    ) ||
    (
      !/HOST_CMD\s*:/i.test(asst) &&
      /\b(i('ll| will)|let me)\b/i.test(asst) &&
      /\b(check|probe|investigate|scan|look at|run)\b/i.test(asst) &&
      asst.length > 0 &&
      asst.length < 600
    );
  return { code, app, host, imagine, error, ui, decide, implement, incomplete };
}

export function detectChipStage(
  chat: ChatMessage[],
  activity: ActivityItem[],
): ChipStage {
  const msgs = chat.filter((m) => m.role === "user" || m.role === "assistant");
  if (msgs.length === 0) return "empty";

  const asst = lastAssistant(chat);
  const recent = activity.slice(0, 8);
  const toolish = recent.some(
    (a) =>
      (a.kind === "desktop" || a.kind === "connector") &&
      (a.status === "running" || a.status === "success" || a.status === "failed"),
  );
  if (
    /\b(error|fail|broken|crash|couldn't complete|not a function|eacces)\b/i.test(asst) ||
    recent.some((a) => a.status === "failed")
  ) {
    return "error";
  }
  if (toolish || /HOST_RESULT|CONNECTOR_RESULT|```host/i.test(asst)) {
    return "tools";
  }
  if (asst.length > 900 || msgs.length >= 8) return "long";
  if (msgs.length >= 2) return "mid";
  return "default";
}

function pack(
  items: Array<Omit<QuickChip, "score"> & { score?: number }>,
  base: number,
): QuickChip[] {
  return items.map((it, i) => ({
    ...it,
    score: (it.score ?? base) - i * 0.6,
  }));
}

/** Chips aimed at the last assistant reply (biggest helpfulness win). */
export function chipsFromLastAssistant(chat: ChatMessage[]): QuickChip[] {
  const asst = lastAssistant(chat);
  if (!asst || asst.length < 24) return [];
  const topic = topicFromText(asst, 4);
  const topicBit = topic ? ` (${topic})` : "";
  const hasCode = /```[\s\S]{12,}/.test(asst);
  const hasError = /\b(error|fail|exception|couldn't complete|not a function)\b/i.test(asst);
  const hasHost = /HOST_CMD|HOST_RESULT|desktop host/i.test(asst);
  const isPlan =
    /\b(i('ll| will)|let me|would you like me to)\b.{0,40}\b(check|probe|investigate|run)\b/i.test(
      asst,
    );

  const out: QuickChip[] = [];

  if (hasError) {
    out.push({
      id: "last-diagnose",
      label: "Explain & fix error",
      value: [
        "Looking at your last reply, diagnose the error in plain English.",
        "Give: (1) root cause, (2) the exact fix, (3) how to verify it worked.",
        topic ? `Focus on: ${topic}.` : "",
      ]
        .filter(Boolean)
        .join(" "),
      kind: "chat",
      score: 96,
      hint: "Because the last reply had an error",
    });
  }

  if (hasCode) {
    out.push(
      {
        id: "last-code-bugs",
        label: shorten(`Find bugs${topicBit}`, 34),
        value: [
          "Review the code in your last message for bugs and edge cases.",
          "List issues by severity with concrete fixes.",
          topic ? `Focus area: ${topic}.` : "",
        ]
          .filter(Boolean)
          .join(" "),
        kind: "chat",
        score: 94,
        hint: "Because the last reply included code",
      },
      {
        id: "last-code-tighten",
        label: "3 concrete improvements",
        value: [
          "From the code in your last message, list exactly 3 high-impact improvements.",
          "For each: what to change, where, and how we'll know it worked.",
        ].join(" "),
        kind: "chat",
        score: 91,
        hint: "Code follow-up",
      },
    );
  }

  if (hasHost || isPlan) {
    out.push({
      id: "last-run-host",
      label: isPlan ? "Finish — run tools now" : "Run diagnostics now",
      value: [
        "Don't just plan — actually investigate my machine now.",
        "Emit HOST_CMD lines for safe read-only checks (uname, paths, processes).",
        "Summarize results after HOST_RESULT. No permission questions.",
      ].join(" "),
      kind: "chat",
      score: 97,
      hint: isPlan
        ? "Predicted: you need action, not another plan"
        : "Because the last reply planned work without tools",
    });
  }

  if (!hasCode && !hasError && asst.length > 80) {
    out.push(
      {
        id: "last-shorter",
        label: "Shorter version",
        value:
          "Rewrite your last answer in half the length. Keep only decisions, commands, and next steps.",
        kind: "chat",
        score: 78,
        hint: "Compress the last reply",
      },
      {
        id: "last-checklist",
        label: "Make a checklist",
        value:
          "Turn your last answer into a short actionable checklist I can follow step by step.",
        kind: "chat",
        score: 76,
        hint: "Actionable follow-up",
      },
    );
  }

  if (topic && !hasError) {
    out.push({
      id: "last-go-deeper",
      label: shorten(`Go deeper: ${topic}`, 34),
      value: `Go deeper on "${topic}" from your last message. Be specific and practical — no filler.`,
      kind: "chat",
      score: 74,
      hint: `Topic from last reply: ${topic}`,
    });
  }

  return out.slice(0, 3);
}

function stageChips(
  stage: ChipStage,
  lastUser: string,
  planLabel: string,
): QuickChip[] {
  switch (stage) {
    case "empty":
      return pack(
        [
          {
            id: "empty-help",
            label: "What can you do?",
            value:
              "In one short list: what can you help me with in GrokHub right now (chat, host tools, Imagine, connectors)?",
            kind: "chat",
            hint: "New chat",
          },
          {
            id: "empty-morning",
            label: "Morning brief",
            value: "/morning",
            kind: "chat",
            hint: "Skill",
          },
          {
            id: "empty-imagine",
            label: "Open Imagine",
            value: "__nav:imagine",
            kind: "nav",
            hint: "Create images / video",
          },
          {
            id: "empty-usage",
            label: "Check my usage",
            value: `What's my subscription usage right now? (plan: ${planLabel})`,
            kind: "chat",
            hint: "Quota",
          },
        ],
        70,
      );
    case "error":
      return pack(
        [
          {
            id: "stg-err-fix",
            label: "Root cause + fix",
            value:
              "Diagnose the latest error: root cause, exact fix, and a 3-step verify checklist.",
            kind: "chat",
            hint: "Error stage",
          },
          {
            id: "stg-err-retry",
            label: "Retry carefully",
            value: lastUser
              ? `Retry this carefully with a safer approach:\n${lastUser}`
              : "Retry the last request with more care and clearer steps.",
            kind: "chat",
            hint: "Error stage",
          },
          {
            id: "stg-err-host",
            label: "Check my machine",
            value:
              "Run safe HOST_CMD diagnostics related to this failure (processes, install paths, recent logs). Summarize findings.",
            kind: "chat",
            hint: "Error stage",
          },
        ],
        88,
      );
    case "tools":
      return pack(
        [
          {
            id: "stg-tool-sum",
            label: "Summarize results",
            value:
              "Summarize the latest tool/host results in plain language. Call out anything that timed out or failed.",
            kind: "chat",
            hint: "After tools",
          },
          {
            id: "stg-tool-next",
            label: "Next safe command",
            value:
              "Based on the last HOST/CONNECTOR results, emit the single best next HOST_CMD (or say we're done).",
            kind: "chat",
            hint: "After tools",
          },
          {
            id: "stg-tool-narrow",
            label: "Narrow the scan",
            value:
              "The last scan may be too broad. Suggest and run a narrower HOST_CMD with maxdepth/head bounds.",
            kind: "chat",
            hint: "After tools",
          },
        ],
        86,
      );
    case "long":
      return pack(
        [
          {
            id: "stg-long-bullets",
            label: "Bullet points only",
            value:
              "Resummarize the thread so far as tight bullets: goals, decisions, open questions, next steps.",
            kind: "chat",
            hint: "Long thread",
          },
          {
            id: "stg-long-decisions",
            label: "Just the decisions",
            value: "List only the decisions and recommendations from this chat. No preamble.",
            kind: "chat",
            hint: "Long thread",
          },
          {
            id: "stg-long-save",
            label: "Save as memory note",
            value:
              "Draft a short persistent memory note I can keep (facts, prefs, paths). Format as plain bullets.",
            kind: "chat",
            hint: "Long thread",
          },
        ],
        80,
      );
    case "mid":
      return pack(
        [
          {
            id: "stg-mid-continue",
            label: "Continue this",
            value: lastUser
              ? `Continue from where we left off. Last ask was: ${lastUser.slice(0, 200)}`
              : "Continue the current task. Be concrete; use HOST_CMD if you need machine data.",
            kind: "chat",
            hint: "Mid-task",
          },
          {
            id: "stg-mid-options",
            label: "Give 3 options",
            value:
              "Give me 3 concrete options for the next step, with tradeoffs, then recommend one.",
            kind: "chat",
            hint: "Mid-task",
          },
          {
            id: "stg-mid-simpler",
            label: "Simpler approach",
            value: "Propose a simpler approach to the current task with fewer moving parts.",
            kind: "chat",
            hint: "Mid-task",
          },
        ],
        75,
      );
    default:
      return [];
  }
}

function codeChips(rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        {
          id: "code-bugs",
          label: "Find bugs",
          value:
            "Review the recent code for bugs and edge cases. List by severity with exact fixes.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-3fix",
          label: "3 improvements",
          value:
            "List exactly 3 high-impact code improvements. For each: change, location, verification.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-tests",
          label: "Add tests",
          value: "Suggest focused unit tests for the code we discussed, including edge cases.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-optimize",
          label: "Make it faster",
          value:
            "Optimize the discussed code for performance. Call out the biggest wins first.",
          kind: "chat",
          hint: "Code context",
        },
      ],
      90,
    ),
    pack(
      [
        {
          id: "code-types",
          label: "Harden types",
          value: "Improve TypeScript types for the recent code — fewer any, clearer contracts.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-errors",
          label: "Clearer errors",
          value:
            "Improve error handling: user-friendly messages, no silent failures, recovery tips.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-refactor",
          label: "Refactor cleanly",
          value: "Refactor for clarity without changing behavior. Show the key diffs.",
          kind: "chat",
          hint: "Code context",
        },
        {
          id: "code-mode",
          label: "Use Build mode",
          value: "__mode:build",
          kind: "mode",
          hint: "Better for long coding",
        },
      ],
      88,
    ),
  ];
  return packs[rotation % packs.length]!;
}

function appChips(rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        {
          id: "app-ui",
          label: "Polish this UI",
          value:
            "Looking at the current GrokHub UI we discussed: list 3 concrete polish fixes (hierarchy, spacing, contrast) ordered by impact.",
          kind: "chat",
          hint: "App context",
        },
        {
          id: "app-faster",
          label: "Snappier feel",
          value:
            "Find the biggest performance wins for what we just discussed. Prioritize perceived latency.",
          kind: "chat",
          hint: "App context",
        },
        {
          id: "app-bug",
          label: "Fix this bug",
          value:
            "Diagnose and fix the bug we were discussing. Include a short retest checklist.",
          kind: "chat",
          hint: "App context",
        },
        {
          id: "app-keys",
          label: "Keyboard shortcuts",
          value: "Propose useful keyboard shortcuts for this screen and how to surface them in UI.",
          kind: "chat",
          hint: "App context",
        },
      ],
      92,
    ),
    pack(
      [
        {
          id: "app-usage",
          label: "Fix usage meter",
          value:
            "Make the usage meter match grok.com subscription limits. Confirm poll + display math.",
          kind: "chat",
          hint: "App context",
        },
        {
          id: "app-chips",
          label: "Smarter chips",
          value:
            "Improve quick-assist chips further: better targeting of the last message and clearer outcomes.",
          kind: "chat",
          hint: "App context",
        },
        {
          id: "app-feature",
          label: "Next feature slice",
          value:
            "Propose the highest-value next GrokHub feature and a minimal first implementation slice.",
          kind: "chat",
          hint: "App context",
        },
      ],
      90,
    ),
  ];
  return packs[rotation % packs.length]!;
}

function hostChips(): QuickChip[] {
  return pack(
    [
      {
        id: "host-diag",
        label: "System snapshot",
        value:
          "Run a quick read-only system snapshot via HOST_CMD (uname, whoami, pwd, grokhub install paths, top grokhub/electron processes). Summarize.",
        kind: "chat",
        hint: "Desktop host",
      },
      {
        id: "host-status",
        label: "Host status shell",
        value: "$ uname -a && whoami && pwd && df -h | head -8",
        kind: "shell",
        hint: "Runs on your machine",
      },
      {
        id: "host-procs",
        label: "Top processes",
        value: "$ ps aux --sort=-%mem | head -12",
        kind: "shell",
        hint: "Runs on your machine",
      },
    ],
    72,
  );
}

function defaultChips(planLabel: string, mode: GrokModeId, rotation: number): QuickChip[] {
  const packs: QuickChip[][] = [
    pack(
      [
        {
          id: "def-help",
          label: "What can you help with?",
          value:
            "What can you help me with in GrokHub right now? Keep it to a short capability list.",
          kind: "chat",
          hint: "Default",
        },
        {
          id: "def-usage",
          label: "My usage",
          value: `What's my usage right now? (${planLabel}) Explain free vs SuperGrok if relevant.`,
          kind: "chat",
          hint: "Default",
        },
        {
          id: "def-imagine",
          label: "Open Imagine",
          value: "__nav:imagine",
          kind: "nav",
          hint: "Images & video",
        },
        {
          id: "def-auto",
          label: mode === "auto" ? "How Adaptive works" : "Use Adaptive",
          value:
            mode === "auto"
              ? "How does Adaptive choose Fast / Think / Deep for my prompts? Give examples."
              : "__mode:auto",
          kind: mode === "auto" ? "chat" : "mode",
          hint: "Routing",
        },
      ],
      24,
    ),
    pack(
      [
        {
          id: "def-feature",
          label: "Suggest a feature",
          value:
            "Suggest the next high-impact GrokHub feature and a minimal plan to ship a first slice.",
          kind: "chat",
          hint: "Default",
        },
        {
          id: "def-modes",
          label: "Explain modes",
          value: "Explain Adaptive / Fast / Expert / Heavy / Build and when to use each.",
          kind: "chat",
          hint: "Default",
        },
        {
          id: "def-host",
          label: "Desktop tools",
          value:
            "Explain what Desktop Host can do for me and when you'll use HOST_CMD vs just answering.",
          kind: "chat",
          hint: "Default",
        },
      ],
      22,
    ),
  ];
  return packs[rotation % packs.length]!;
}

function topicChipsFromTitle(title: string | null | undefined): QuickChip[] {
  const t = (title || "").trim();
  if (!t || /^new chat$/i.test(t) || t.length < 3) return [];
  return [
    {
      id: "topic-focus",
      label: shorten(`Focus: ${t}`, 32),
      value: `Stay focused on "${t}". Give the next concrete step and do it (use HOST_CMD if local data is needed).`,
      kind: "chat",
      score: 68,
      hint: `Thread topic: ${t}`,
    },
    {
      id: "topic-wrap",
      label: shorten(`Wrap up: ${t}`, 32),
      value: `Wrap up the "${t}" thread: status, decisions, and remaining todos in bullets.`,
      kind: "chat",
      score: 55,
      hint: `Thread topic: ${t}`,
    },
  ];
}

function applyDraftBoost(chips: QuickChip[], draftRaw: string): QuickChip[] {
  const draft = draftRaw.trim().toLowerCase();
  if (!draft) return chips;

  return chips
    .map((c) => {
      const hay = `${c.label} ${c.value} ${c.hint || ""}`.toLowerCase();
      let boost = 0;
      if (hay.startsWith(draft)) boost += 42;
      else if (hay.includes(draft)) boost += 26;
      for (const tok of draft.split(/\s+/)) {
        if (tok.length > 2 && hay.includes(tok)) boost += 9;
      }
      if (draft.startsWith("$") && c.kind === "shell") boost += 36;
      if (/imagine|draw|image|logo|video/.test(draft) && /imagine|image|video/i.test(hay))
        boost += 40;
      if (/bug|error|fix|crash|fail/.test(draft) && /bug|error|fix|diagnos|root cause/i.test(hay))
        boost += 28;
      if (/code|refactor|test|type/.test(draft) && (c.id.startsWith("code-") || c.id.startsWith("last-code")))
        boost += 24;
      if (/host|shell|process|machine|linux|install/.test(draft) && (c.kind === "shell" || /host|HOST_CMD/i.test(hay)))
        boost += 30;
      if (/usage|quota|limit|subscription/.test(draft) && /usage|quota/i.test(hay)) boost += 32;
      return { ...c, score: c.score + boost };
    })
    .sort((a, b) => b.score - a.score);
}

/**
 * Build ranked quick-assistant chips.
 * Default ≤ 4 visible (primary + context). Hard cap 8.
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
  const draft = (input.draft || "").trim().toLowerCase();
  const ctx = detectChipContext(input.chat);
  const stage = detectChipStage(input.chat, input.activity);

  // ── Blockers first (connection / host / quota) ────────────────────────
  if (!input.grokConnected) {
    chips.push({
      id: "ctx-connect",
      label: "Connect Grok",
      value: "__nav:settings",
      kind: "nav",
      score: 120,
      hint: "You're not connected — this is the best next step",
    });
  }
  if (input.hostOnline === false) {
    chips.push({
      id: "ctx-host",
      label: "Connect desktop host",
      value: "__nav:settings",
      kind: "nav",
      score: 110,
      hint: "Host tools need the desktop bridge",
    });
  }
  if (pct >= 80) {
    chips.push({
      id: "ctx-quota",
      label: `Usage ${pct}% — save units`,
      value:
        "What's my usage right now and how can I save units without losing quality?",
      kind: "chat",
      score: 100,
      hint: "Quota is high",
    });
  }

  // ── Predictive pool (draft / habits / transitions / activity) ─────────
  const { chips: predicted, intents } = collectPredictiveChips({
    chat: input.chat,
    draft: input.draft,
    activity: input.activity,
    memory: input.memory,
  });
  chips.push(...predicted);

  // ── Last-assistant targeting ──────────────────────────────────────────
  chips.push(...chipsFromLastAssistant(input.chat));

  if (ctx.incomplete) {
    chips.push({
      id: "ctx-incomplete",
      label: "Finish the job",
      value:
        "Finish the incomplete work from your last reply. Act now (HOST_CMD if needed). End with status.",
      kind: "chat",
      score: 96,
      hint: "Predicted incomplete turn",
    });
  }
  if (ctx.decide) {
    chips.push({
      id: "ctx-decide",
      label: "Recommend & do",
      value:
        "Recommend the best option for my setup and take the first concrete step.",
      kind: "chat",
      score: 89,
      hint: "Decision context",
    });
  }
  if (ctx.implement) {
    chips.push({
      id: "ctx-implement",
      label: "Implement it",
      value:
        "Implement the requested change as a minimal solid slice. Inspect files if needed, then apply.",
      kind: "chat",
      score: 91,
      hint: "Implement context",
    });
  }

  // ── Stage packs ───────────────────────────────────────────────────────
  chips.push(...stageChips(stage, lastUser, plan.label));

  // ── Context packs ─────────────────────────────────────────────────────
  if (ctx.code) chips.push(...codeChips(rotation));
  if (ctx.app) chips.push(...appChips(rotation));
  if (ctx.host || input.hostOnline) chips.push(...hostChips());
  if (ctx.imagine) {
    chips.push({
      id: "ctx-imagine",
      label: "Open Imagine",
      value: "__nav:imagine",
      kind: "nav",
      score: 88,
      hint: "Conversation mentioned images/video",
    });
  }
  if (ctx.error && stage !== "error") {
    chips.push({
      id: "err-diagnose",
      label: "Root cause + fix",
      value:
        "Diagnose the error we hit — root cause, exact fix, and how to verify.",
      kind: "chat",
      score: 93,
      hint: "Error mentioned in chat",
    });
  }

  // ── Thread topic ──────────────────────────────────────────────────────
  chips.push(...topicChipsFromTitle(input.threadTitle));

  // ── Activity signals ──────────────────────────────────────────────────
  for (const a of input.activity.slice(0, 6)) {
    if (a.kind === "chat" && a.status === "failed" && lastUser) {
      chips.push({
        id: `act-retry-${a.id}`,
        label: "Retry last ask",
        value: lastUser,
        kind: "chat",
        score: 84,
        hint: "Last attempt failed",
      });
    }
  }

  // ── Defaults when thin ────────────────────────────────────────────────
  const hasStrong =
    ctx.code || ctx.app || ctx.error || stage === "error" || stage === "tools" || stage === "empty";
  if (!hasStrong || chips.length < 3) {
    chips.push(...defaultChips(plan.label, input.mode, rotation));
  } else {
    chips.push(
      ...defaultChips(plan.label, input.mode, rotation).map((c) => ({
        ...c,
        score: Math.min(c.score, 16),
      })),
    );
  }

  // ── LLM context chips (Fast mode) ─────────────────────────────────────
  if (input.llmChips?.length) {
    for (const c of input.llmChips) {
      chips.push({
        ...c,
        score: Math.max(c.score, 95),
        hint: c.hint || "Suggested for this chat",
      });
    }
  }

  // ── Memory ────────────────────────────────────────────────────────────
  let withMemory = applyMemoryToChips(chips, input.memory);
  if (input.contextTag && input.memory?.hits?.length) {
    withMemory = withMemory.map((c) => ({
      ...c,
      score:
        c.score + memoryBoostForContext(input.memory as QuickAssistMemory, c, input.contextTag),
    }));
  }
  withMemory = withMemory.filter(
    (c) => !dismissed.has(c.value.trim().toLowerCase()) && !dismissed.has(c.id),
  );

  let ranked = uniqByValue(withMemory)
    .filter((c) => c.value.trim().toLowerCase() !== draft)
    .sort((a, b) => b.score - a.score);

  if (rotation > 0) {
    ranked = ranked
      .map((c, i) => ({
        ...c,
        score: c.score + ((i + rotation) % 5 === 0 ? 10 : 0) - (i < 2 ? 5 : 0),
      }))
      .sort((a, b) => b.score - a.score);
  }

  // ── Intent boost (trajectory prediction) ──────────────────────────────
  ranked = applyIntentBoost(ranked, intents);

  // ── Draft predictive re-rank (strong while typing) ────────────────────
  if (draft.length >= 1) {
    ranked = applyPredictiveDraftBoost(ranked, draft, intents);
    // Legacy soft token boost for very short drafts
    if (draft.length > 0 && draft.length < 4) {
      ranked = applyDraftBoost(ranked, draft);
    }
    // Soft filter — keep high-confidence predictions even if wording differs
    if (draft.length >= 2) {
      const filtered = ranked.filter((c) => {
        const hay = `${c.label} ${c.value} ${c.hint || ""}`.toLowerCase();
        return (
          c.score >= 85 ||
          c.id.startsWith("pred-") ||
          hay.includes(draft) ||
          draft.split(/\s+/).some((tok) => tok.length > 2 && hay.includes(tok)) ||
          c.kind === "nav" ||
          c.kind === "shell"
        );
      });
      if (filtered.length) ranked = filtered;
    }
  }

  // Mid-thread: prefer chat actions over nav chrome
  if (stage !== "empty" && input.chat.length >= 2) {
    ranked = ranked
      .map((c) =>
        c.kind === "nav" || c.kind === "mode" ? { ...c, score: c.score - 16 } : c,
      )
      .sort((a, b) => b.score - a.score);
  }

  // ── Mix kinds · pick top N · mark primary ─────────────────────────────
  const picked: QuickChip[] = [];
  const kindCount: Record<string, number> = {};
  for (const c of ranked) {
    if (picked.length >= max) break;
    const k = c.kind;
    const n = kindCount[k] || 0;
    if (k === "shell" && n >= 1 && max <= 5) continue;
    if (k === "shell" && n >= 2) continue;
    if (k === "nav" && n >= 1 && max <= 5 && stage !== "empty") continue;
    if (k === "nav" && n >= 2) continue;
    if (k === "mode" && n >= 1) continue;
    picked.push(c);
    kindCount[k] = n + 1;
  }

  if (picked.length < Math.min(3, max)) {
    for (const c of ranked) {
      if (picked.length >= max) break;
      if (picked.some((p) => p.id === c.id)) continue;
      picked.push(c);
    }
  }

  const final = picked.slice(0, max);
  const intentHint = topIntentLabel(intents);
  if (final[0]) {
    final[0] = {
      ...final[0],
      primary: true,
      hint:
        final[0].hint ||
        (intentHint
          ? `Predicted next: ${intentHint}`
          : stage === "empty"
            ? "Best next step for a new chat"
            : "Highest-ranked next action"),
    };
  }
  return final;
}

/** Generate a fresh alternate pack (for Refresh). */
export function suggestMoreChips(
  input: QuickAssistantInput,
  extraRotation = 1,
): QuickChip[] {
  return buildQuickChips({
    ...input,
    rotation: (input.rotation || 0) + extraRotation,
    max: input.max ?? MAX_DEFAULT,
  });
}
