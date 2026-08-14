#!/usr/bin/env node
/** Max-mode / high-autonomy load caps — do not kill live streams or pile chores. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const budget = await import(pathToFileURL(path.join(root, "src/lib/load-budget.ts")).href);
const {
  LOAD_BUDGET,
  toolRoundBudget,
  finishNudgeBudget,
  shouldReflectThisTurn,
  shouldSelfImproveThisTurn,
  mapPool,
} = budget;

assert.ok(LOAD_BUDGET.maxParallelHost <= 2);
assert.ok(LOAD_BUDGET.maxHostCmdsPerRound <= 3);
assert.ok(LOAD_BUDGET.maxFreeRoamChores <= 1);
assert.ok(LOAD_BUDGET.housekeepingMinMs >= 60_000);
assert.ok(toolRoundBudget("max") <= 8);
assert.ok(toolRoundBudget("fast") <= 6);
assert.ok(finishNudgeBudget("max") <= 3);
assert.equal(shouldReflectThisTurn(1), false);
assert.equal(shouldReflectThisTurn(2), false);
assert.equal(shouldReflectThisTurn(6), true);
assert.equal(shouldSelfImproveThisTurn(12), false);
assert.equal(shouldSelfImproveThisTurn(24), true);

const pooled = await mapPool([1, 2, 3, 4], 2, async (n) => n * 10);
assert.deepEqual(pooled, [10, 20, 30, 40]);

const proactiveSrc = fs.readFileSync(path.join(root, "src/lib/proactive.ts"), "utf8");
assert.match(proactiveSrc, /Never finalize a live turn/);
assert.doesNotMatch(proactiveSrc, /now - input\.streamStartedAt >/);
assert.match(proactiveSrc, /LOAD_BUDGET\.maxFreeRoamChores/);

const storeSrc = fs.readFileSync(path.join(root, "src/lib/store.ts"), "utf8");
assert.match(storeSrc, /Skipped while a turn is running/);
assert.match(storeSrc, /toolRoundBudget/);
assert.match(storeSrc, /mapPool/);
assert.match(storeSrc, /shouldSelfImproveThisTurn/);

const learnSrc = fs.readFileSync(path.join(root, "src/lib/session-learn.ts"), "utf8");
assert.match(learnSrc, /shouldReflectThisTurn/);
assert.doesNotMatch(learnSrc, /totalTurns % 2 === 0/);

console.log("smoke-load-budget OK");
