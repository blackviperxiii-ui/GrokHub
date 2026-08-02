/**
 * Adaptive empty-chat welcome — Fast mode + learned user context.
 */
import type { ChatThread } from "./types";
import type { LearningState } from "./learning";
import type { QuickAssistMemory } from "./quick-assist-memory";
import { topHabitLabels } from "./quick-assist-memory";

export type WelcomePayload = {
  headline: string;
  body: string;
  generatedAt: number;
  source: "llm" | "fallback";
};

export function emptyWelcomeFallback(opts?: {
  displayName?: string | null;
  habits?: string[];
  interests?: string[];
}): WelcomePayload {
  const name = (opts?.displayName || "").trim().split(/\s+/)[0] || "";
  const habit = opts?.habits?.[0];
  const interest = opts?.interests?.[0];
  let body =
    "Ask anything, run $ shell commands, or try /help for slash commands.";
  if (habit && interest) {
    body = `Last time you leaned into “${habit}”. Want to continue that, dig into ${interest}, or start something new?`;
  } else if (habit) {
    body = `Ready when you are — pick up “${habit}”, open the desktop host, or just ask.`;
  } else if (interest) {
    body = `Curious about ${interest}? Ask me anything, or jump in with a shell command.`;
  } else if (name) {
    body = `Hey ${name} — what are we building or fixing today?`;
  }
  return {
    headline: name ? `Welcome back, ${name}` : "What's next?",
    body,
    generatedAt: Date.now(),
    source: "fallback",
  };
}

export function collectWelcomeContext(opts: {
  learning?: LearningState | null;
  quickAssistMemory?: QuickAssistMemory | null;
  memoryNotes?: string;
  userMd?: string;
  threads?: ChatThread[];
  displayName?: string | null;
  planLabel?: string | null;
}): {
  displayName: string;
  habits: string[];
  insights: string[];
  interests: string[];
  recentTopics: string[];
  notes: string;
} {
  const habits = topHabitLabels(opts.quickAssistMemory || { hits: [], transitions: {}, lastChipKey: null, totalEvents: 0, updatedAt: 0, version: 1 }, 8);
  const insights = (opts.learning?.insights || [])
    .slice()
    .sort((a, b) => b.confidence - a.confidence || b.lastSeenAt - a.lastSeenAt)
    .slice(0, 8)
    .map((i) => i.text.trim())
    .filter(Boolean);

  const prefHits = opts.learning?.prefHits || {};
  const interests = Object.entries(prefHits)
    .sort((a, b) => b[1] - a[1])
    .slice(0, 8)
    .map(([k]) => k.replace(/_/g, " "));

  const recentTopics = (opts.threads || [])
    .filter((t) => t.title && !/^new chat$/i.test(t.title))
    .slice(0, 8)
    .map((t) => t.title.trim());

  const notes = [opts.memoryNotes, opts.userMd]
    .filter(Boolean)
    .join("\n")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 900);

  return {
    displayName: (opts.displayName || "").trim(),
    habits,
    insights,
    interests,
    recentTopics,
    notes,
  };
}

export function buildWelcomePrompt(ctx: ReturnType<typeof collectWelcomeContext>): string {
  return [
    "Write a warm, sharp empty-state welcome for GrokHub (desktop Grok agent).",
    "Return ONLY valid JSON:",
    '{"headline":"≤6 words","body":"1–2 short sentences, max 160 chars"}',
    "",
    "Rules:",
    "- Personal without being creepy; use first name only if given",
    "- Reference 1 concrete interest, habit, or recent topic when available",
    "- Invite action (build, fix, explore, shell, imagine) — not generic filler",
    "- No markdown, no emojis, no quotes around the whole message",
    "- Sound like a capable teammate, not a corporate chatbot",
    "",
    ctx.displayName ? `Name: ${ctx.displayName}` : "Name: (unknown)",
    ctx.habits.length ? `Habits / frequent actions: ${ctx.habits.join(" · ")}` : "",
    ctx.interests.length ? `Interests / prefs: ${ctx.interests.join(" · ")}` : "",
    ctx.insights.length ? `Learned about user: ${ctx.insights.slice(0, 5).join(" · ")}` : "",
    ctx.recentTopics.length ? `Recent chats: ${ctx.recentTopics.slice(0, 5).join(" · ")}` : "",
    ctx.notes ? `Notes: ${ctx.notes.slice(0, 400)}` : "",
    !ctx.habits.length && !ctx.insights.length && !ctx.interests.length
      ? "Little known yet — still make it lively and specific to a desktop agent."
      : "",
  ]
    .filter(Boolean)
    .join("\n");
}

export function parseWelcomePayload(raw: string, fallback: WelcomePayload): WelcomePayload {
  let text = String(raw || "").trim();
  text = text.replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/i, "").trim();
  const start = text.indexOf("{");
  const end = text.lastIndexOf("}");
  if (start >= 0 && end > start) text = text.slice(start, end + 1);
  try {
    const obj = JSON.parse(text) as { headline?: string; body?: string };
    const headline = String(obj.headline || "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 48);
    const body = String(obj.body || "")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 200);
    if (headline.length >= 2 && body.length >= 8) {
      return {
        headline,
        body,
        generatedAt: Date.now(),
        source: "llm",
      };
    }
  } catch {
    /* fall through */
  }
  // plain text fallback: first line headline, rest body
  const lines = String(raw)
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);
  if (lines[0] && lines[0].length >= 2) {
    return {
      headline: lines[0].slice(0, 48),
      body: (lines.slice(1).join(" ") || lines[0]).slice(0, 200),
      generatedAt: Date.now(),
      source: "llm",
    };
  }
  return { ...fallback, generatedAt: Date.now() };
}
