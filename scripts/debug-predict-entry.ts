import assert from "node:assert/strict";
import { buildQuickChips, detectChipContext } from "../src/lib/quick-assistant";
import {
  predictIntents,
  draftPredictionChips,
  collectPredictiveChips,
} from "../src/lib/quick-assist-predict";
import { emptyQuickAssistMemory, rememberChipClick, rememberChipDismiss } from "../src/lib/quick-assist-memory";
import type { UsageSnapshot } from "../src/lib/types";

const usage = {
  plan: "pro", usedUnits: 1, messages: 1, imagine: 0, automations: 0, host: 0, periodStart: Date.now(),
} as unknown as UsageSnapshot;
const base = {
  activity: [] as any[], threads: [] as any[], connectors: [] as any[],
  mode: "auto" as const, grokConnected: true, usage, hostOnline: true,
};

const issues: string[] = [];

try {
  const empty = buildQuickChips({ ...base, chat: [] });
  assert.ok(Array.isArray(empty) && empty.length <= 5);
} catch (e) {
  issues.push("empty chat crash: " + e);
}

{
  const c = buildQuickChips({ ...base, chat: [], grokConnected: false });
  if (!c.some((x) => /connect/i.test(x.label))) issues.push("missing Connect Grok when offline");
}

{
  const chat = [
    { id: "1", role: "user" as const, content: "hi", createdAt: 1 },
    { id: "2", role: "assistant" as const, content: "Hello there, how can I help?", createdAt: 2 },
  ];
  const built = buildQuickChips({ ...base, chat: chat as any });
  if (built[0]) {
    const d = buildQuickChips({
      ...base,
      chat: chat as any,
      dismissed: [built[0].value, built[0].id],
    });
    if (d.some((x) => x.value === built[0]!.value)) {
      issues.push("dismissed chip still present: " + built[0].label);
    }
  }
}

{
  const c = buildQuickChips({ ...base, chat: [], draft: "a".repeat(5000) });
  assert.ok(c.length <= 8);
  for (const chip of c) {
    if (chip.label.length > 48) issues.push("label too long: " + chip.label.length);
  }
}

{
  const c = buildQuickChips({ ...base, chat: [], memory: null as any });
  assert.ok(Array.isArray(c));
}

{
  const ctx = detectChipContext([]);
  if (ctx.incomplete) issues.push("incomplete true on empty chat");
}

{
  const intents = predictIntents({ chat: [], draft: "" });
  for (const [k, v] of Object.entries(intents)) {
    if (!Number.isFinite(v) || v < 0) issues.push(`intent ${k}=${v}`);
  }
}

assert.equal(draftPredictionChips("").length, 0);

{
  let mem = emptyQuickAssistMemory();
  mem = rememberChipClick(mem, {
    id: "habit1", label: "My habit", value: "Do the habit thing", kind: "chat", score: 40,
  });
  mem = rememberChipDismiss(mem, {
    id: "habit1", label: "My habit", value: "Do the habit thing", kind: "chat", score: 40,
  });
  mem = rememberChipDismiss(mem, {
    id: "habit1", label: "My habit", value: "Do the habit thing", kind: "chat", score: 40,
  });
  const { chips } = collectPredictiveChips({ chat: [], draft: "Do the", memory: mem });
  if (chips.some((c) => c.value === "Do the habit thing" && c.id.includes("habit"))) {
    issues.push("dismissed habit still predicted");
  }
}

{
  const c = buildQuickChips({
    ...base,
    chat: [
      { id: "1", role: "user", content: "review code", createdAt: 1 },
      { id: "2", role: "assistant", content: "```ts\nconst x: any = 1\n```", createdAt: 2 },
    ] as any,
  });
  if (c.length && !c[0]!.primary) issues.push("primary not set");
}

{
  const c = buildQuickChips({
    ...base,
    chat: [
      { id: "1", role: "user", content: "error crash fail broken", createdAt: 1 },
      { id: "2", role: "assistant", content: "Error: crash failed exception", createdAt: 2 },
    ] as any,
    draft: "fix",
  });
  const vals = c.map((x) => x.value.toLowerCase());
  if (new Set(vals).size !== vals.length) issues.push("duplicate values");
}

console.log(issues.length ? "ISSUES:\n" + issues.map((i) => " - " + i).join("\n") : "predict edge cases: all clear (" + issues.length + ")");
process.exit(issues.length ? 1 : 0);
