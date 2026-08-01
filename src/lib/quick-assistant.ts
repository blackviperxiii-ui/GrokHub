/**
 * Predictive quick-assistant chips for the Agent composer.
 * Ranked from recent chat, activity, host/Grok state — max 10, not crowded.
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
  return t.slice(0, n - 1) + "…";
}

/** Infer follow-ups from a prior user line. */
function followUpsFrom(msg: string): QuickChip[] {
  const lower = msg.toLowerCase();
  const out: QuickChip[] = [];
  if (/download/i.test(lower)) {
    out.push({
      id: "fu-dl",
      label: "List Downloads again",
      value: "List what's in my Downloads folder",
      kind: "chat",
      score: 70,
    });
    out.push({
      id: "fu-dl-shell",
      label: "$ ls Downloads",
      value: '$ ls -la "$HOME/Downloads" | head -40',
      kind: "shell",
      score: 65,
    });
  }
  if (/desktop|host|uname|shell|cli/i.test(lower) || lower.startsWith("$")) {
    out.push({
      id: "fu-disk",
      label: "Disk free",
      value: "$ df -h | head -12",
      kind: "shell",
      score: 55,
    });
    out.push({
      id: "fu-ps",
      label: "Top processes",
      value: "$ ps aux --sort=-%mem | head -12",
      kind: "shell",
      score: 50,
    });
  }
  if (/code|bug|error|implement|refactor|build/i.test(lower)) {
    out.push({
      id: "fu-build",
      label: "Switch to Build",
      value: "__mode:build",
      kind: "mode",
      score: 60,
      hint: "mode",
    });
    out.push({
      id: "fu-explain",
      label: "Explain the approach",
      value: "Explain the approach step by step",
      kind: "chat",
      score: 52,
    });
  }
  if (/imagine|image|draw|picture|logo/i.test(lower)) {
    out.push({
      id: "fu-imagine",
      label: "Open Imagine",
      value: "__nav:imagine",
      kind: "nav",
      score: 68,
    });
  }
  if (/usage|quota|limit|plan/i.test(lower)) {
    out.push({
      id: "fu-usage",
      label: "Usage details",
      value: "What's my SuperGrok usage right now?",
      kind: "chat",
      score: 48,
    });
  }
  return out;
}

/**
 * Build ranked quick-assistant chips.
 * Always returns ≤ max (default 8, hard cap 10).
 */
export function buildQuickChips(input: QuickAssistantInput): QuickChip[] {
  const max = Math.min(input.max ?? MAX_DEFAULT, MAX_HARD);
  const chips: QuickChip[] = [];
  const users = recentUserMessages(input.chat);
  const lastUser = users[0] || "";
  const pct = Math.round(usagePercent(input.usage));
  const plan = PLAN_LIMITS[input.usage.plan];
  const recentActivity = input.activity.slice(0, 12);
  const draft = (input.draft || "").trim().toLowerCase();

  // ── Context-aware base chips ──────────────────────────────────────────
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
  } else if (input.hostOnline) {
    chips.push({
      id: "ctx-uname",
      label: "$ uname -a",
      value: "$ uname -a && whoami && pwd",
      kind: "shell",
      score: 40,
    });
  }

  if (pct >= 80) {
    chips.push({
      id: "ctx-quota",
      label: `Usage ${pct}%`,
      value: "What's my usage and how can I save units?",
      kind: "chat",
      score: 85,
    });
  }

  // ── Recent chat → continue / re-run ───────────────────────────────────
  for (let i = 0; i < Math.min(users.length, 5); i++) {
    const msg = users[i]!;
    const ageBoost = 30 - i * 5;
    // Don't re-offer the exact last message as "send again" if it's long
    if (msg.length < 80 && i > 0) {
      chips.push({
        id: `recent-${i}`,
        label: shorten(msg, 28),
        value: msg,
        kind: msg.startsWith("$") ? "shell" : "chat",
        score: 45 + ageBoost,
        hint: "recent",
      });
    }
    chips.push(...followUpsFrom(msg).map((c) => ({ ...c, score: c.score + ageBoost * 0.3 })));
  }

  // ── Activity feed signals ─────────────────────────────────────────────
  for (const a of recentActivity) {
    if (a.kind === "desktop") {
      chips.push({
        id: `act-host-${a.id}`,
        label: "Host status",
        value: "$ uname -a && df -h | head -8",
        kind: "shell",
        score: 42,
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
    if (a.kind === "chat" && a.status === "failed") {
      chips.push({
        id: `act-retry-${a.id}`,
        label: "Retry last ask",
        value: lastUser || "Try that again",
        kind: "chat",
        score: 72,
      });
    }
    if (a.kind === "system" && /update/i.test(a.title + a.detail)) {
      chips.push({
        id: `act-upd-${a.id}`,
        label: "Check updates",
        value: "__nav:settings",
        kind: "nav",
        score: 35,
      });
    }
    if (a.kind === "usage") {
      chips.push({
        id: `act-usage-${a.id}`,
        label: "Usage",
        value: "What's my usage right now?",
        kind: "chat",
        score: 44,
      });
    }
  }

  // ── Thread continuity ─────────────────────────────────────────────────
  const otherThreads = input.threads
    .filter((t) => t.messages.some((m) => m.role === "user"))
    .slice(0, 3);
  for (const th of otherThreads) {
    const first = th.messages.find((m) => m.role === "user");
    if (!first) continue;
    chips.push({
      id: `thread-${th.id}`,
      label: shorten(th.title || first.content, 26),
      value: `Continue: ${shorten(first.content, 120)}`,
      kind: "chat",
      score: 28,
      hint: "history",
    });
  }

  // ── Connector-aware ───────────────────────────────────────────────────
  const liveConnectors = input.connectors.filter((c) => c.status === "connected");
  if (liveConnectors.some((c) => c.id === "desktop-host")) {
    chips.push({
      id: "conn-files",
      label: "Browse home",
      value: '$ ls -la "$HOME" | head -30',
      kind: "shell",
      score: 38,
    });
  }

  // ── Stable helpful defaults (low score — fill remaining) ──────────────
  const defaults: QuickChip[] = [
    {
      id: "def-help",
      label: "What can you help with?",
      value: "What can you help me with in GrokHub?",
      kind: "chat",
      score: 20,
    },
    {
      id: "def-modes",
      label: "Explain my modes",
      value: "Explain Auto / Fast / Expert / Heavy / Build and when to use each",
      kind: "chat",
      score: 18,
    },
    {
      id: "def-usage",
      label: "My usage",
      value: `What's my usage? (${plan.label})`,
      kind: "chat",
      score: 16,
    },
    {
      id: "def-imagine",
      label: "Imagine",
      value: "__nav:imagine",
      kind: "nav",
      score: 15,
    },
    {
      id: "def-auto",
      label: input.mode === "auto" ? "How Auto routes" : "Use Auto mode",
      value:
        input.mode === "auto"
          ? "How does Auto choose models for my prompts?"
          : "__mode:auto",
      kind: input.mode === "auto" ? "chat" : "mode",
      score: 14,
    },
  ];
  chips.push(...defaults);

  // Don't suggest the exact draft text as a chip
  let ranked = uniqByValue(chips)
    .filter((c) => c.value.trim().toLowerCase() !== draft)
    .sort((a, b) => b.score - a.score);

  // ── Predictive filter from draft ──────────────────────────────────────
  if (draft.length >= 1) {
    const pred = ranked
      .map((c) => {
        const hay = `${c.label} ${c.value}`.toLowerCase();
        let boost = 0;
        if (hay.startsWith(draft)) boost += 40;
        else if (hay.includes(draft)) boost += 25;
        // token overlap
        for (const tok of draft.split(/\s+/)) {
          if (tok.length > 2 && hay.includes(tok)) boost += 8;
        }
        if (draft.startsWith("$") && c.kind === "shell") boost += 30;
        if (/imagine|draw|image/.test(draft) && c.kind === "nav" && c.value.includes("imagine"))
          boost += 35;
        return { ...c, score: c.score + boost };
      })
      .filter((c) => {
        const hay = `${c.label} ${c.value}`.toLowerCase();
        // Keep high-score context chips even if no match when draft is short
        if (draft.length < 2) return true;
        return (
          c.score >= 80 ||
          hay.includes(draft) ||
          draft.split(/\s+/).some((tok) => tok.length > 2 && hay.includes(tok))
        );
      })
      .sort((a, b) => b.score - a.score);

    // If prediction emptied the list, fall back to top ranked
    ranked = pred.length ? pred : ranked;
  }

  // Prefer a mix: not all shell, not all nav — soft diversity for top slots
  const picked: QuickChip[] = [];
  const kindCount: Record<string, number> = {};
  for (const c of ranked) {
    if (picked.length >= max) break;
    const k = c.kind;
    const n = kindCount[k] || 0;
    // Allow more chat chips; cap shell/nav a bit so UI stays readable
    if (k === "shell" && n >= 3) continue;
    if (k === "nav" && n >= 2) continue;
    if (k === "mode" && n >= 1) continue;
    picked.push(c);
    kindCount[k] = n + 1;
  }

  // Fill if diversity filter was too aggressive
  if (picked.length < Math.min(4, max)) {
    for (const c of ranked) {
      if (picked.length >= max) break;
      if (picked.some((p) => p.id === c.id)) continue;
      picked.push(c);
    }
  }

  return picked.slice(0, max);
}
