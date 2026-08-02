#!/usr/bin/env node
/** Lightweight unit checks (no network). */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
const require = createRequire(import.meta.url);

const bridge = require("../desktop/grok-bridge.cjs");
assert.equal(typeof bridge.checkForUpdate, "function");
assert.equal(typeof bridge.applyUpdate, "function");
assert.equal(typeof bridge.checkRollback, "function");
assert.equal(typeof bridge.applyRollback, "function");
assert.equal(typeof bridge.postUpdateSelfTest, "function");
assert.equal(typeof bridge.scheduleAppRestart, "function");
assert.equal(typeof bridge.factoryReinstall, "function", "factoryReinstall must be defined (system install crash)");

const bridgeSrc = fs.readFileSync(
  path.join(process.cwd(), "desktop/grok-bridge.cjs"),
  "utf8",
);
assert.match(bridgeSrc, /async function factoryReinstall/);
assert.match(bridgeSrc, /isGrokHubUiPid/);
const liveFuser = bridgeSrc
  .split("\n")
  .filter((l) => /fuser\s+-k/.test(l))
  .filter((l) => !/^\s*(\/\/|\*|\/\*|#)/.test(l.trim()))
  .filter((l) => !/never|NEVER|no fuser|not use|don't|do not/i.test(l));
assert.equal(liveFuser.length, 0, "must not invoke fuser -k: " + liveFuser.join(" | "));

const log = require("../desktop/log.cjs");
assert.equal(typeof log.info, "function");
assert.equal(typeof log.error, "function");
log.info("smoke-unit log write");
assert.ok(fs.existsSync(log.paths().logDir));

const host = require("../desktop/host-bridge.cjs");
assert.equal(typeof host.runExec, "function");
assert.equal(typeof host.setSafeMode, "function");
assert.equal(typeof host.getSafeMode, "function");
assert.equal(typeof host.killExec, "function");
const sm = host.setSafeMode(true);
assert.equal(sm.safeMode, true);
const blocked = await host.runExec("rm -rf /tmp/grokhub-should-not");
assert.equal(blocked.ok, false);
assert.match(String(blocked.stderr), /safe mode/i);
host.setSafeMode(false);

const st = await bridge.postUpdateSelfTest({ root: process.cwd() });
assert.equal(typeof st.ok, "boolean");
assert.ok(Array.isArray(st.checks));

const launcher = fs.readFileSync(
  path.join(process.cwd(), "packaging/aur/grokhub.sh"),
  "utf8",
);
assert.match(launcher, /factoryReinstall/);
assert.match(launcher, /pid_is_our_ui/);
assert.equal(
  (launcher.match(/^\s*[^#\n]*fuser\s+-k/m) || []).length,
  0,
  "launcher must not invoke fuser -k",
);

// Context manager module present
assert.ok(
  fs.existsSync(path.join(process.cwd(), "src/lib/context-manager.ts")),
  "context-manager.ts required",
);
const ctxSrc = fs.readFileSync(
  path.join(process.cwd(), "src/lib/context-manager.ts"),
  "utf8",
);
assert.match(ctxSrc, /export function buildContext/);
assert.match(ctxSrc, /export function compactMessages/);
assert.match(ctxSrc, /CONTEXT_BUDGET_TOKENS/);

const mem = require("../desktop/memory-store.cjs");
assert.equal(typeof mem.buildPinBundle, "function");
assert.equal(typeof mem.appendFacts, "function");
assert.equal(typeof mem.info, "function");
const memSrc = fs.readFileSync(path.join(process.cwd(), "desktop/memory-store.cjs"), "utf8");
assert.match(memSrc, /MEMORY\.md/);
assert.match(memSrc, /USER\.md/);
assert.ok(fs.existsSync(path.join(process.cwd(), "src/lib/file-memory.ts")));
assert.ok(fs.existsSync(path.join(process.cwd(), "src/lib/learning.ts")));
console.log("smoke-unit OK");


// Adaptive router golden prompts (esbuild + import)
{
  const { spawnSync } = await import("node:child_process");
  const out = path.join(process.cwd(), ".tmp-router-test.mjs");
  const r = spawnSync(
    "npx",
    ["esbuild", "src/lib/models-catalog.ts", "--bundle", "--platform=node", "--format=esm", `--outfile=${out}`],
    { encoding: "utf8" },
  );
  assert.equal(r.status, 0, "esbuild router: " + (r.stderr || r.stdout || ""));
  const mod = await import(pathToFileURL(out).href + `?t=${Date.now()}`);
  const { routeAuto, buildCatalog } = mod;
  const cat = buildCatalog([
    "grok-4.20-0309-non-reasoning",
    "grok-4.20-0309-reasoning",
    "grok-4.3",
    "grok-4.5",
    "grok-build-0.1",
    "grok-imagine-image",
  ]);
  // ensure flagship slots
  cat.slots.fast = "grok-4.20-0309-non-reasoning";
  cat.slots.balanced = "grok-4.3";
  cat.slots.smart = "grok-4.20-0309-reasoning";
  cat.slots.heavy = "grok-4.5";
  cat.slots.build = "grok-build-0.1";
  cat.slots.imagine = "grok-imagine-image";

  const expect = (prompt, tier, ctx = {}) => {
    const res = routeAuto(prompt, cat, ctx);
    assert.equal(
      res.tier,
      tier,
      `routeAuto(${JSON.stringify(prompt)}) → ${res.tier} (want ${tier}) · ${res.reasonDetail}`,
    );
  };

  expect("hi", "fast");
  expect("thanks!", "fast");
  expect("what do you think?", "balanced");
  expect("explain docker compose in plain english", "balanced");
  expect("how do I improve this UI spacing a bit", "balanced");
  expect(
    "Compare trade-offs of event-driven vs request-response for our multi-region architecture and recommend a path",
    "deep",
  );
  expect(
    "implement a full refactor of the auth module with unit tests and migration plan",
    "build",
  );
  expect("draw a logo of a red fox astronaut", "imagine");
  // follow-up hold
  expect("yes continue", "think", { lastRouteTier: "think", historyTurns: 4 });
  // hysteresis: don't leap deep→fast
  const h = routeAuto("ok cool", cat, { lastRouteTier: "deep", historyTurns: 5 });
  assert.ok(
    h.tier === "deep" || h.tier === "build" || h.tier === "think" || h.tier === "balanced",
    "hysteresis should not jump to pure fast from deep: " + h.tier,
  );
  // usage pressure pushes cheaper
  const pressured = routeAuto(
    "Please review this approach carefully and tell me what you think about the tradeoffs",
    cat,
    { usagePressure: 0.9, preferFree: true },
  );
  assert.ok(
    pressured.tier === "fast" || pressured.tier === "balanced" || pressured.tier === "think",
    "usage pressure should avoid deep for mid prompt: " + pressured.tier,
  );

  try {
    fs.unlinkSync(out);
  } catch {
    /* ignore */
  }
  console.log("routeAuto golden OK");
}

