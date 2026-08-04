import assert from "node:assert/strict";
import { buildQuickChips } from "../src/lib/quick-assistant";
import {
  predictIntents,
  draftPredictionChips,
  applyPredictiveDraftBoost,
} from "../src/lib/quick-assist-predict";
import type { UsageSnapshot } from "../src/lib/types";
import { emptyQuickAssistMemory, rememberChipClick } from "../src/lib/quick-assist-memory";

const usage = {
  plan: "pro",
  usedUnits: 1,
  messages: 1,
  imagine: 0,
  automations: 0,
  host: 0,
  periodStart: Date.now(),
} as unknown as UsageSnapshot;

const base = {
  activity: [] as any[],
  threads: [] as any[],
  connectors: [] as any[],
  mode: "auto" as const,
  grokConnected: true,
  usage,
  hostOnline: true,
};

// Draft "fix" should predict debug chips
const draftChips = draftPredictionChips("fix the launcher");
assert.ok(draftChips.length >= 1);
assert.ok(/debug|fix|HOST/i.test(draftChips[0]!.label + draftChips[0]!.value));

const intents = predictIntents({
  chat: [
    { id: "1", role: "user", content: "cpu high", createdAt: 1 },
    { id: "2", role: "assistant", content: "Let me check processes.", createdAt: 2 },
  ] as any,
  draft: "contin",
});
assert.ok(intents.finish > 0.3 || intents.continue > 0.3 || intents.host > 0.2);

// Typing "fix" re-ranks
const chat = [
  { id: "1", role: "user", content: "something broke", createdAt: 1 },
  {
    id: "2",
    role: "assistant",
    content: "There was an error in the launcher.",
    createdAt: 2,
  },
] as any;
const withDraft = buildQuickChips({ ...base, chat, draft: "fix" });
assert.ok(withDraft.length >= 1);
assert.ok(
  withDraft.some((c) => /fix|diagnos|error|bug/i.test(c.label + c.value + (c.hint || ""))),
  withDraft.map((c) => c.label).join(", "),
);

// Habit transition prediction
let mem = emptyQuickAssistMemory();
mem = rememberChipClick(mem, {
  id: "a",
  label: "System snapshot",
  value: "Run a system snapshot",
  kind: "chat",
  score: 50,
});
mem = rememberChipClick(mem, {
  id: "b",
  label: "Summarize results",
  value: "Summarize the host results",
  kind: "chat",
  score: 50,
});
const after = buildQuickChips({
  ...base,
  chat: [{ id: "1", role: "user", content: "status", createdAt: 1 }] as any,
  memory: mem,
});
assert.ok(after.length >= 1);

// Prefix completion
const prefix = draftPredictionChips("check c");
assert.ok(prefix.some((c) => /cpu|process|HOST/i.test(c.label + c.value)));

console.log(
  "smoke-predict OK · draft:",
  withDraft.map((c) => c.label).join(" · "),
);

// Memory key: predicted dismiss must share habit with built-in chip
import {
  emptyQuickAssistMemory,
  rememberChipClick,
  rememberChipDismiss,
  chipMemoryKey,
} from "../src/lib/quick-assist-memory";
{
  let mem = emptyQuickAssistMemory();
  const habit = {
    id: "host-diag",
    label: "System snapshot",
    value: "Run a quick system snapshot",
    kind: "chat" as const,
    score: 50,
  };
  mem = rememberChipClick(mem, habit);
  const pred = { ...habit, id: "pred-habit-xyz", score: 90 };
  assert.equal(chipMemoryKey(pred), chipMemoryKey(habit));
  mem = rememberChipDismiss(mem, pred);
  mem = rememberChipDismiss(mem, pred);
  const hit = mem.hits.find((h) => h.value === habit.value);
  assert.ok(hit && (hit.dismisses || 0) >= 2);
  console.log("memory-key dismiss OK");
}
