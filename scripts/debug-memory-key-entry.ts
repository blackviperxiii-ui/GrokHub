import assert from "node:assert/strict";
import {
  emptyQuickAssistMemory,
  rememberChipClick,
  rememberChipDismiss,
  chipMemoryKey,
  applyMemoryToChips,
} from "../src/lib/quick-assist-memory";
import { collectPredictiveChips } from "../src/lib/quick-assist-predict";
import type { QuickChip } from "../src/lib/quick-assistant";

let mem = emptyQuickAssistMemory();
const habit: QuickChip = {
  id: "host-diag",
  label: "System snapshot",
  value: "Run a quick system snapshot",
  kind: "chat",
  score: 50,
};
mem = rememberChipClick(mem, habit);
mem = rememberChipClick(mem, habit);
// User dismisses a *predicted* clone of the same habit
const pred: QuickChip = {
  id: "pred-habit-xyz",
  label: "System snapshot",
  value: "Run a quick system snapshot",
  kind: "chat",
  score: 90,
};
assert.equal(chipMemoryKey(pred), chipMemoryKey(habit));
mem = rememberChipDismiss(mem, pred);
mem = rememberChipDismiss(mem, pred);
const hit = mem.hits.find((h) => h.value === habit.value);
assert.ok(hit && (hit.dismisses || 0) >= 2, "dismisses should land on same hit");
const { chips } = collectPredictiveChips({ chat: [], draft: "Run a", memory: mem });
assert.ok(
  !chips.some((c) => c.value === habit.value && c.id.includes("habit")),
  "dismissed habit should not be re-predicted",
);
console.log("memory-key OK");
