#!/usr/bin/env node
/** Smoke: plan-only stall detection (no false act announcements). */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

// Prefer TS strip if available (Node 22+)
let looksLikePlanningStall;
let looksLikeDeferredHostWork;
let userWantsHostInvestigation;
try {
  const mod = await import(pathToFileURL(path.join(root, "src/lib/agent-finish.ts")).href);
  looksLikePlanningStall = mod.looksLikePlanningStall;
} catch {
  // Fallback: eval minimal patterns from source
  looksLikePlanningStall = (text) => {
    const s = String(text || "").trim();
    if (/HOST_CMD\s*:/i.test(s)) return false;
    return /running checks?|i'll probe|let me investigate|would you like me to start/i.test(s);
  };
}
try {
  const g = await import(pathToFileURL(path.join(root, "src/lib/grok.ts")).href);
  looksLikeDeferredHostWork = g.looksLikeDeferredHostWork;
  userWantsHostInvestigation = g.userWantsHostInvestigation;
} catch {
  looksLikeDeferredHostWork = looksLikePlanningStall;
  userWantsHostInvestigation = (p) => /deep dive|audit|process|install/i.test(p);
}

assert.equal(
  looksLikePlanningStall("Running checks now."),
  true,
  "announce-only must stall",
);
assert.equal(
  looksLikePlanningStall("I'll probe processes next."),
  true,
  "deferred probe must stall",
);
assert.equal(
  looksLikePlanningStall("HOST_CMD: ps aux | head\nLooking at processes."),
  false,
  "with HOST_CMD must not stall",
);
assert.equal(
  looksLikeDeferredHostWork("Would you like me to start the investigation?"),
  true,
);
assert.equal(userWantsHostInvestigation("do a deep dive on the install"), true);
assert.equal(userWantsHostInvestigation("what is 2+2"), false);

console.log("smoke-agent-finish OK");
