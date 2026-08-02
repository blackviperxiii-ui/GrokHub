/**
 * Fast-mode LLM suggestions for quick-assist chips.
 * Context-aware, super-short labels, actionable prompts.
 */
import type { ChatMessage } from "./types";
import type { QuickChip } from "./quick-assistant";
import { detectChipContext, detectChipStage } from "./quick-assistant";
import type { ActivityItem } from "./types";

export type LlmChipSeed = {
  label: string;
  value: string;
  kind?: "chat" | "shell" | "nav" | "mode";
  hint?: string;
};

function cleanTranscript(messages: ChatMessage[], max = 8): string {
  const rows = messages
    .filter((m) => m.role === "user" || m.role === "assistant")
    .slice(-max);
  const lines: string[] = [];
  for (const m of rows) {
    const who = m.role === "user" ? "User" : "Asst";
    const body = String(m.content || "")
      .replace(/```[\s\S]*?```/g, "[code]")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 220);
    if (body) lines.push(`${who}: ${body}`);
  }
  return lines.join("\n");
}

export function buildChipSuggestPrompt(opts: {
  chat: ChatMessage[];
  threadTitle?: string | null;
  draft?: string;
  habits?: string[];
  dismissed?: string[];
}): string {
  const ctx = detectChipContext(opts.chat);
  const stage = detectChipStage(opts.chat, []);
  const flags = Object.entries(ctx)
    .filter(([, v]) => v)
    .map(([k]) => k)
    .join(", ");
  const habits = (opts.habits || []).slice(0, 6).join(" · ");
  const dismissed = (opts.dismissed || []).slice(0, 8).join(" · ");
  return [
    "You generate quick-action chips for a Grok desktop agent chat.",
    "Return ONLY valid JSON array of 3 to 4 objects:",
    '[{"label":"≤28 chars","value":"full prompt to send","kind":"chat","hint":"why useful"}]',
    "",
    "Rules:",
    "- label: short, action-first (e.g. Fix this bug, Add tests, Next step)",
    "- value: a concrete instruction the user would send (1–2 sentences max)",
    "- Prefer next steps grounded in the transcript (not generic tips)",
    "- If code present → review/optimize/test style chips",
    "- If errors → diagnose/fix/verify chips",
    "- If host/tools → one shell-oriented chat chip is OK (kind chat, not raw $ unless clear)",
    "- Avoid chips the user dismissed",
    "- Prefer habits if they fit the current context",
    "- No markdown fences, no commentary outside JSON",
    "",
    `Stage: ${stage}`,
    `Signals: ${flags || "none"}`,
    opts.threadTitle ? `Thread title: ${opts.threadTitle}` : "",
    opts.draft?.trim() ? `User is typing: ${opts.draft.trim().slice(0, 80)}` : "",
    habits ? `User habits: ${habits}` : "",
    dismissed ? `Dismissed (avoid): ${dismissed}` : "",
    "",
    "Transcript:",
    cleanTranscript(opts.chat) || "(empty chat)",
  ]
    .filter(Boolean)
    .join("\n");
}

export function parseLlmChips(raw: string): QuickChip[] {
  let text = String(raw || "").trim();
  // strip fences
  text = text.replace(/^```(?:json)?\s*/i, "").replace(/\s*```$/i, "").trim();
  // extract array
  const start = text.indexOf("[");
  const end = text.lastIndexOf("]");
  if (start >= 0 && end > start) text = text.slice(start, end + 1);
  let arr: unknown;
  try {
    arr = JSON.parse(text);
  } catch {
    // try line-based fallback "label | value"
    const lines = String(raw)
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean)
      .slice(0, 4);
    return lines
      .map((line, i) => {
        const parts = line.split("|").map((p) => p.trim());
        const label = (parts[0] || line).replace(/^[-*\d.)\s]+/, "").slice(0, 32);
        const value = parts[1] || parts[0] || line;
        if (label.length < 2 || value.length < 4) return null;
        return {
          id: `llm-${i}-${label.slice(0, 12)}`,
          label,
          value: value.slice(0, 400),
          kind: "chat" as const,
          score: 98 - i,
          hint: "Suggested from this chat",
          primary: i === 0,
        } satisfies QuickChip;
      })
      .filter(Boolean) as QuickChip[];
  }
  if (!Array.isArray(arr)) return [];
  const out: QuickChip[] = [];
  for (let i = 0; i < arr.length && out.length < 4; i++) {
    const row = arr[i] as Record<string, unknown>;
    if (!row || typeof row !== "object") continue;
    const label = String(row.label || "").replace(/\s+/g, " ").trim().slice(0, 32);
    const value = String(row.value || "").replace(/\s+/g, " ").trim().slice(0, 400);
    if (label.length < 2 || value.length < 4) continue;
    if (/^__nav:|^__mode:/.test(value) && !String(row.kind || "").includes("nav")) {
      /* allow */
    }
    let kind = String(row.kind || "chat").toLowerCase();
    if (!["chat", "shell", "nav", "mode"].includes(kind)) kind = "chat";
    // force dangerous shell into chat instruction
    if (kind === "shell" && !value.startsWith("$") && !value.startsWith("/sh")) {
      kind = "chat";
    }
    out.push({
      id: `llm-${Date.now().toString(36)}-${i}-${label.slice(0, 10).replace(/\s/g, "")}`,
      label,
      value,
      kind: kind as QuickChip["kind"],
      score: 99 - i * 1.5,
      hint: String(row.hint || "Suggested from this chat").slice(0, 80),
      primary: i === 0,
    });
  }
  return out;
}

export function contextFingerprint(
  chat: ChatMessage[],
  activity: ActivityItem[] = [],
): string {
  const c = detectChipContext(chat);
  const stage = detectChipStage(chat, activity);
  const bits: string[] = [stage];
  if (c.code) bits.push("code");
  if (c.app) bits.push("app");
  if (c.host) bits.push("host");
  if (c.imagine) bits.push("imagine");
  if (c.error) bits.push("error");
  if (c.ui) bits.push("ui");
  return bits.join("+") || "default";
}
