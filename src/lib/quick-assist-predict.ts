/**
 * Predictive scoring for quick-assist chips.
 * Combines: draft prefix, intent trajectory, habit transitions, time-of-day.
 */
import type { ActivityItem, ChatMessage } from "./types";
import type { QuickChip } from "./quick-assistant";
import type { QuickAssistMemory } from "./quick-assist-memory";
import { chipMemoryKey, learnedChipsFromMemory } from "./quick-assist-memory";

export type PredictedIntent =
  | "finish"
  | "fix"
  | "ship"
  | "explain"
  | "host"
  | "decide"
  | "create"
  | "continue"
  | "review"
  | "chat";

export type IntentScores = Record<PredictedIntent, number>;

function lastAssistant(chat: ChatMessage[]): string {
  for (let i = chat.length - 1; i >= 0; i--) {
    if (chat[i]?.role === "assistant" && chat[i]!.content?.trim()) {
      return chat[i]!.content;
    }
  }
  return "";
}

function lastUser(chat: ChatMessage[]): string {
  for (let i = chat.length - 1; i >= 0; i--) {
    if (chat[i]?.role === "user" && chat[i]!.content?.trim()) {
      return chat[i]!.content;
    }
  }
  return "";
}

/** Softmax-ish intent probabilities from chat + draft + activity. */
export function predictIntents(opts: {
  chat: ChatMessage[];
  draft?: string;
  activity?: ActivityItem[];
}): IntentScores {
  const scores: IntentScores = {
    finish: 0,
    fix: 0,
    ship: 0,
    explain: 0,
    host: 0,
    decide: 0,
    create: 0,
    continue: 0,
    review: 0,
    chat: 0.15,
  };

  const asst = lastAssistant(opts.chat);
  const user = lastUser(opts.chat);
  const draft = (opts.draft || "").toLowerCase();
  const blob = `${user}\n${asst}\n${draft}`.toLowerCase();

  // Assistant incomplete / planning
  if (
    /\b(let me|i('ll| will)|looking into|running checks|continuing)\b/i.test(asst) ||
    (/\b(check|probe|investigate)\b/i.test(asst) && !/HOST_CMD\s*:/i.test(asst))
  ) {
    scores.finish += 0.55;
    scores.host += 0.25;
  }

  // Errors
  if (/\b(error|fail|crash|broken|exception|eacces|couldn't complete)\b/i.test(blob)) {
    scores.fix += 0.5;
  }

  // Code
  if (/```[\s\S]{12,}/.test(asst + user) || /\b(function|const |import |typescript|refactor)\b/i.test(blob)) {
    scores.review += 0.4;
    scores.ship += 0.2;
  }

  // User implement language
  if (/\b(add |implement|build |create |make |wire |patch|fix )\b/i.test(user + " " + draft)) {
    scores.ship += 0.45;
  }

  // Explain
  if (/\b(explain|how does|what is|why |walk me)\b/i.test(user + " " + draft)) {
    scores.explain += 0.45;
  }

  // Host
  if (
    /HOST_|desktop host|\$ |journalctl|process|install path|system/i.test(blob) ||
    (opts.activity || []).some((a) => a.kind === "desktop")
  ) {
    scores.host += 0.4;
  }

  // Decide
  if (/\b(should i|which|options?|recommend|vs\.?|tradeoff)\b/i.test(blob)) {
    scores.decide += 0.4;
  }

  // Create / imagine
  if (/\b(imagine|image|draw|logo|video|generate)\b/i.test(blob)) {
    scores.create += 0.5;
  }

  // Continue
  if (/\b(continue|keep going|go on|resume|next)\b/i.test(user + " " + draft)) {
    scores.continue += 0.5;
    scores.finish += 0.2;
  }

  // Draft-only early signals (strong prediction while typing)
  if (draft.length >= 2) {
    if (/^(fix|bug|error|crash)/.test(draft)) scores.fix += 0.35;
    if (/^(add|implement|build|make|create)/.test(draft)) scores.ship += 0.35;
    if (/^(explain|how|why|what)/.test(draft)) scores.explain += 0.35;
    if (/^(check|scan|look|ps |find |ls )/.test(draft)) scores.host += 0.35;
    if (/^(continue|keep|finish)/.test(draft)) scores.finish += 0.4;
    if (/^(imagine|draw|image)/.test(draft)) scores.create += 0.4;
    if (draft.startsWith("$") || draft.startsWith("/sh")) scores.host += 0.5;
  }

  // Normalize lightly (not required to sum to 1)
  return scores;
}

/** Map intent → chip id/value keywords for boost matching. */
const INTENT_KEYWORDS: Record<PredictedIntent, RegExp> = {
  finish: /finish|complete|continue|started|goal|HOST_CMD|act now/i,
  fix: /fix|diagnos|error|root cause|bug|repair/i,
  ship: /implement|ship|apply|patch|add |build|wire/i,
  explain: /explain|simple|plain|walkthrough|eli5|takeaway/i,
  host: /host|HOST_|system|snapshot|process|shell|machine|scan/i,
  decide: /recommend|option|pick|best|tradeoff|decide/i,
  create: /imagine|image|draw|logo|video|create/i,
  continue: /continue|keep|resume|next step/i,
  review: /review|bugs|improve|refactor|test|types/i,
  chat: /help|what can|capability/i,
};

/** Boost existing chips from predicted intents. */
export function applyIntentBoost(
  chips: QuickChip[],
  intents: IntentScores,
): QuickChip[] {
  const top = (Object.entries(intents) as [PredictedIntent, number][])
    .sort((a, b) => b[1] - a[1])
    .slice(0, 4);

  return chips
    .map((c) => {
      const hay = `${c.id} ${c.label} ${c.value} ${c.hint || ""}`;
      let boost = 0;
      let matched: PredictedIntent | null = null;
      for (const [intent, score] of top) {
        if (score < 0.2) continue;
        if (INTENT_KEYWORDS[intent].test(hay)) {
          boost += score * 48;
          if (!matched || score > (intents[matched] || 0)) matched = intent;
        }
      }
      if (!boost) return c;
      const conf = matched ? Math.round((intents[matched] || 0) * 100) : 0;
      return {
        ...c,
        score: c.score + boost,
        hint:
          conf >= 35
            ? `Predicted · ${matched}${c.hint ? ` · ${c.hint}` : ""}`
            : c.hint,
      };
    })
    .sort((a, b) => b.score - a.score);
}

/**
 * Chips synthesized from what the user is typing (completion / expansion).
 * These are the strongest "predictive" signal while drafting.
 */
export function draftPredictionChips(draftRaw: string): QuickChip[] {
  const draft = draftRaw.trim();
  if (draft.length < 2) return [];
  const lower = draft.toLowerCase();
  const out: QuickChip[] = [];

  // Shell draft
  if (draft.startsWith("$") || draft.startsWith("/sh ")) {
    out.push({
      id: "pred-shell-run",
      label: "Run this on host",
      value: draft.startsWith("$") ? draft : `$ ${draft.replace(/^\/sh\s+/, "")}`,
      kind: "shell",
      score: 130,
      hint: "Predicted from your draft",
    });
    return out;
  }

  // Prefix templates — complete the thought
  const templates: Array<{ re: RegExp; label: string; build: (d: string) => string; score: number }> = [
    {
      re: /^(fix|debug)\b/i,
      label: "Debug with host evidence",
      build: (d) =>
        `${d}. Investigate with HOST_CMD for real evidence. Root cause, fix, verify.`,
      score: 125,
    },
    {
      re: /^(add|implement|build|make)\b/i,
      label: "Implement this now",
      build: (d) =>
        `${d}. Ship a minimal solid slice. Inspect files with HOST_CMD if needed, then apply.`,
      score: 124,
    },
    {
      re: /^(explain|how|why|what)\b/i,
      label: "Explain simply",
      build: (d) =>
        `${d}. Plain language, one example, one practical takeaway. No filler.`,
      score: 120,
    },
    {
      re: /^(check|scan|look at|inspect)\b/i,
      label: "Check with HOST_CMD",
      build: (d) =>
        `${d}. Use bounded HOST_CMD (maxdepth/head). Summarize results clearly.`,
      score: 123,
    },
    {
      re: /^(continue|finish|keep)\b/i,
      label: "Continue until done",
      build: (d) =>
        `${d}. Complete the goal fully. Act now — no planning-only replies.`,
      score: 126,
    },
    {
      re: /^(improve|polish|optimize)\b/i,
      label: "3 high-impact improvements",
      build: (d) =>
        `${d}. List exactly 3 improvements with change, location, and verification.`,
      score: 118,
    },
    {
      re: /^(imagine|draw|generate)\b/i,
      label: "Open Imagine for this",
      build: () => "__nav:imagine",
      score: 115,
    },
  ];

  for (const t of templates) {
    if (t.re.test(draft)) {
      const value = t.build(draft);
      out.push({
        id: `pred-draft-${t.label.slice(0, 12).replace(/\s/g, "")}`,
        label: t.label,
        value,
        kind: value.startsWith("__nav:") ? "nav" : "chat",
        score: t.score,
        hint: "Predicted from what you're typing",
      });
      break;
    }
  }

  // Partial word → common full prompts (prefix match)
  if (draft.length >= 3 && draft.length <= 40 && !out.length) {
    const completions: Array<[string, string, string]> = [
      ["fix the", "Fix the bug we hit", "Diagnose and fix the latest bug. Root cause, exact fix, verify steps."],
      ["fix o", "Fix OAuth / session", "Fix Grok OAuth or session issues. Check tokens, refresh, and reconnect path."],
      ["add ", "Add a feature", "Implement the feature I'm describing as a minimal solid slice."],
      ["check c", "Check CPU / processes", "Run HOST_CMD for top CPU/memory processes and summarize what's hot."],
      ["check i", "Check install paths", "Via HOST_CMD, verify GrokHub install paths (~/.local/lib/grokhub) and launcher."],
      ["make ", "Make it better", "Improve what we just discussed with 3 concrete high-impact changes."],
      ["how d", "How does this work?", "Explain how this works simply with one example."],
      ["why ", "Why did this fail?", "Explain why this failed and the fix. Be specific."],
      ["contin", "Continue the task", "Continue until the current goal is fully done. Use tools when needed."],
      ["summar", "Summarize the thread", "Summarize this thread: goals, decisions, open questions, next steps."],
      ["test ", "Add tests", "Suggest focused tests for the code we discussed, including edge cases."],
      ["refact", "Refactor cleanly", "Refactor for clarity without changing behavior. Show key diffs."],
    ];
    for (const [prefix, label, value] of completions) {
      if (lower.startsWith(prefix) || prefix.startsWith(lower)) {
        out.push({
          id: `pred-prefix-${prefix.trim()}`,
          label,
          value: value.startsWith(draft) ? value : `${draft} — ${value}`,
          kind: "chat",
          score: 112,
          hint: "Predicted completion",
        });
        if (out.length >= 2) break;
      }
    }
  }

  // Generic: "send expanded form of draft" when draft is a short seed
  if (draft.length >= 8 && draft.length <= 60 && out.length < 2) {
    out.push({
      id: "pred-expand-draft",
      label: "Expand & send",
      value: `${draft}. Be concrete and complete the goal. Use HOST_CMD if you need machine data.`,
      kind: "chat",
      score: 100,
      hint: "Predicted expansion of your draft",
    });
  }

  return out.slice(0, 3);
}

/**
 * Predict next chips from habit transition graph (A → B sequences).
 */
export function transitionPredictionChips(
  memory: QuickAssistMemory | null | undefined,
): QuickChip[] {
  if (!memory?.lastChipKey || !memory.transitions[memory.lastChipKey]) return [];
  const row = memory.transitions[memory.lastChipKey]!;
  const ranked = Object.entries(row).sort((a, b) => b[1] - a[1]).slice(0, 4);
  const out: QuickChip[] = [];
  for (const [key, n] of ranked) {
    if (n < 1) continue;
    const hit = memory.hits.find((h) => h.key === key);
    if (!hit || !hit.value) continue;
    if (hit.dismisses && hit.dismisses >= 2) continue;
    out.push({
      id: `pred-tx-${key.slice(0, 24)}`,
      label: hit.label,
      value: hit.value,
      kind: hit.kind,
      score: 70 + Math.min(30, n * 8),
      hint: `Usually next after your last action (×${n})`,
    });
  }
  return out;
}

/**
 * Habits that match draft prefix strongly → surface as "you'll probably send this".
 */
export function draftHabitPredictions(
  memory: QuickAssistMemory | null | undefined,
  draftRaw: string,
): QuickChip[] {
  const draft = draftRaw.trim().toLowerCase();
  if (!memory?.hits?.length || draft.length < 2) return [];
  const out: QuickChip[] = [];
  for (const h of memory.hits) {
    if (h.uses < 1) continue;
    if (h.dismisses && h.dismisses >= 2) continue;
    const v = h.value.toLowerCase();
    const lab = h.label.toLowerCase();
    let score = 0;
    if (v.startsWith(draft)) score += 55;
    else if (lab.startsWith(draft)) score += 45;
    else if (v.includes(draft)) score += 25;
    else if (draft.split(/\s+/).every((t) => t.length < 3 || v.includes(t) || lab.includes(t)))
      score += 15;
    if (score < 20) continue;
    score += Math.min(25, Math.log2(1 + h.uses) * 8);
    score += Math.min(12, (h.successes || 0) * 3);
    out.push({
      id: `pred-habit-${h.key.slice(0, 28)}`,
      label: h.label,
      value: h.value,
      kind: h.kind,
      score: 90 + score,
      hint: h.uses >= 4 ? "Predicted from your habits" : "You've used this before",
    });
  }
  return out.sort((a, b) => b.score - a.score).slice(0, 3);
}

/** Stronger draft boost for ranking (prefix + token + intent). */
export function applyPredictiveDraftBoost(
  chips: QuickChip[],
  draftRaw: string,
  intents: IntentScores,
): QuickChip[] {
  const draft = draftRaw.trim().toLowerCase();
  if (!draft) return chips;

  return chips
    .map((c) => {
      const hay = `${c.label} ${c.value} ${c.hint || ""}`.toLowerCase();
      let boost = 0;
      // Prefix hierarchy
      if (c.label.toLowerCase().startsWith(draft)) boost += 55;
      else if (hay.startsWith(draft)) boost += 48;
      else if (c.value.toLowerCase().startsWith(draft)) boost += 42;
      else if (hay.includes(` ${draft}`) || hay.includes(draft)) boost += 22;

      // Token overlap
      const tokens = draft.split(/\s+/).filter((t) => t.length > 2);
      let hits = 0;
      for (const tok of tokens) {
        if (hay.includes(tok)) {
          hits += 1;
          boost += 11;
        }
      }
      // All tokens hit → strong prediction
      if (tokens.length >= 2 && hits === tokens.length) boost += 28;

      // Kind hints from draft shape
      if (draft.startsWith("$") && c.kind === "shell") boost += 50;
      if (/imagine|draw|image|logo|video/.test(draft) && /imagine|image|video|__nav:imagine/i.test(hay))
        boost += 45;
      if (/bug|error|fix|crash|fail/.test(draft) && INTENT_KEYWORDS.fix.test(hay)) boost += 32;
      if (/code|refactor|test|type|review/.test(draft) && INTENT_KEYWORDS.review.test(hay))
        boost += 28;
      if (/host|shell|process|machine|linux|install|scan|check/.test(draft) && INTENT_KEYWORDS.host.test(hay))
        boost += 34;
      if (/add|implement|build|ship|make|create/.test(draft) && INTENT_KEYWORDS.ship.test(hay))
        boost += 30;
      if (/continue|finish|keep|resume/.test(draft) && INTENT_KEYWORDS.finish.test(hay))
        boost += 36;

      // Intent alignment
      for (const [intent, score] of Object.entries(intents) as [PredictedIntent, number][]) {
        if (score >= 0.3 && INTENT_KEYWORDS[intent].test(hay)) {
          boost += score * 20;
        }
      }

      return {
        ...c,
        score: c.score + boost,
        hint:
          boost >= 40
            ? `Predicted match${c.hint ? ` · ${c.hint}` : ""}`
            : c.hint,
      };
    })
    .sort((a, b) => b.score - a.score);
}

/** Activity trajectory: failed chat → retry; host success → summarize; etc. */
export function activityPredictionChips(
  activity: ActivityItem[],
  lastUserText: string,
): QuickChip[] {
  const out: QuickChip[] = [];
  const recent = activity.slice(0, 8);
  for (const a of recent) {
    if (a.kind === "chat" && a.status === "failed" && lastUserText) {
      out.push({
        id: `pred-act-retry-${a.id}`,
        label: "Retry last ask",
        value: lastUserText,
        kind: "chat",
        score: 105,
        hint: "Predicted — last attempt failed",
      });
    }
    if (a.kind === "desktop" && a.status === "success") {
      out.push({
        id: `pred-act-sum-${a.id}`,
        label: "Summarize host results",
        value:
          "Summarize the latest host results in plain language. Call out failures or timeouts.",
        kind: "chat",
        score: 88,
        hint: "Predicted after host success",
      });
    }
    if (a.kind === "desktop" && a.status === "failed") {
      out.push({
        id: `pred-act-narrow-${a.id}`,
        label: "Retry narrower host cmd",
        value:
          "The last host command failed or timed out. Retry with a narrower, safer HOST_CMD (head/maxdepth).",
        kind: "chat",
        score: 92,
        hint: "Predicted after host failure",
      });
    }
  }
  // dedupe by id prefix type
  const seen = new Set<string>();
  return out.filter((c) => {
    const k = c.label;
    if (seen.has(k)) return false;
    seen.add(k);
    return true;
  }).slice(0, 3);
}

/** Top predicted intent label for UI. */
export function topIntentLabel(intents: IntentScores): string | null {
  const [intent, score] = (Object.entries(intents) as [PredictedIntent, number][]).sort(
    (a, b) => b[1] - a[1],
  )[0] || [null, 0];
  if (!intent || score < 0.35) return null;
  const labels: Record<PredictedIntent, string> = {
    finish: "finish work",
    fix: "fix something",
    ship: "build/ship",
    explain: "explain",
    host: "use desktop host",
    decide: "decide",
    create: "create media",
    continue: "continue",
    review: "review code",
    chat: "chat",
  };
  return labels[intent];
}

/** Merge predictive sources into the candidate pool. */
export function collectPredictiveChips(opts: {
  chat: ChatMessage[];
  draft?: string;
  activity?: ActivityItem[];
  memory?: QuickAssistMemory | null;
}): { chips: QuickChip[]; intents: IntentScores } {
  const intents = predictIntents({
    chat: opts.chat,
    draft: opts.draft,
    activity: opts.activity,
  });
  const chips: QuickChip[] = [
    ...draftPredictionChips(opts.draft || ""),
    ...draftHabitPredictions(opts.memory, opts.draft || ""),
    ...transitionPredictionChips(opts.memory),
    ...activityPredictionChips(opts.activity || [], lastUser(opts.chat)),
  ];
  // Light inject of strong habits even without draft (time-of-day + frequency)
  if (!opts.draft?.trim() && opts.memory) {
    const hour = new Date().getHours();
    for (const h of learnedChipsFromMemory(opts.memory).slice(0, 4)) {
      const hit = opts.memory.hits.find((x) => chipMemoryKey({ id: h.id, value: h.value, kind: h.kind }) === x.key)
        || opts.memory.hits.find((x) => x.value === h.value);
      let score = h.score + 10;
      if (hit?.hourHits?.[hour]) score += Math.min(15, hit.hourHits[hour]! * 3);
      chips.push({
        ...h,
        id: `pred-tod-${h.id}`,
        score,
        hint: hit?.hourHits?.[hour] ? "Predicted for this time of day" : h.hint || "habit",
      });
    }
  }
  return { chips, intents };
}
