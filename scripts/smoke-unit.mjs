#!/usr/bin/env node
/** Lightweight unit checks (no network). */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
const require = createRequire(import.meta.url);

// redact
const { pathToFileURL } = await import("node:url");
// dynamic import ts not available — test via node on built redact is hard.
// Instead test pure CJS helpers from grok-bridge
const bridge = require("../desktop/grok-bridge.cjs");
assert.equal(typeof bridge.checkForUpdate, "function");
assert.equal(typeof bridge.applyUpdate, "function");
assert.equal(typeof bridge.checkRollback, "function");
assert.equal(typeof bridge.applyRollback, "function");
assert.equal(typeof bridge.postUpdateSelfTest, "function");
assert.equal(typeof bridge.scheduleAppRestart, "function");

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

// shaMatch via check path - call postUpdateSelfTest
const st = await bridge.postUpdateSelfTest({ root: process.cwd() });
assert.equal(typeof st.ok, "boolean");
assert.ok(Array.isArray(st.checks));

console.log("smoke-unit OK");
