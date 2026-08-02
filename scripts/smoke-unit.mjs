#!/usr/bin/env node
/** Lightweight unit checks (no network). */
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
const require = createRequire(import.meta.url);

const bridge = require("../desktop/grok-bridge.cjs");
assert.equal(typeof bridge.checkForUpdate, "function");
assert.equal(typeof bridge.applyUpdate, "function");
assert.equal(typeof bridge.checkRollback, "function");
assert.equal(typeof bridge.applyRollback, "function");
assert.equal(typeof bridge.postUpdateSelfTest, "function");
assert.equal(typeof bridge.scheduleAppRestart, "function");
assert.equal(typeof bridge.factoryReinstall, "function", "factoryReinstall must be defined (system install crash)");

// Bridge source must contain factoryReinstall (packaging completeness)
const bridgeSrc = fs.readFileSync(
  path.join(process.cwd(), "desktop/grok-bridge.cjs"),
  "utf8",
);
assert.match(bridgeSrc, /async function factoryReinstall/);
assert.match(bridgeSrc, /isGrokHubUiPid/);
// No executable fuser -k (ignore comments / strings that only warn about it)
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

// Launcher must refuse incomplete bridges
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

console.log("smoke-unit OK");
