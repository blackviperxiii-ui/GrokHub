#!/usr/bin/env node
/** Computer-use protocol + vision hydrate (no display / injector required). */
import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);

const {
  extractComputerCommands,
  stripComputerCommands,
  parseComputerCommand,
  formatComputerCommand,
  parseComputerRecipe,
  classifyComputerStep,
  needsComputerConfirm,
  isComputerWriteOp,
} = await import("../src/lib/computer-protocol.ts");

const click = parseComputerCommand("click 412 880");
assert.ok(click);
assert.equal(click.op, "click");
assert.equal(click.x, 412);
assert.equal(click.y, 880);

const typed = parseComputerCommand('type Hello world');
assert.ok(typed);
assert.equal(typed.op, "type");
assert.equal(typed.text, "Hello world");

const jsonStep = parseComputerCommand('{"op":"key","key":"ctrl+t"}');
assert.ok(jsonStep);
assert.equal(jsonStep.op, "key");
assert.equal(jsonStep.key, "ctrl+t");

assert.equal(parseComputerCommand("screenshot").op, "screenshot");
assert.equal(isComputerWriteOp("screenshot"), false);
assert.equal(isComputerWriteOp("click"), true);
assert.equal(classifyComputerStep({ op: "screenshot" }), "safe");
assert.equal(classifyComputerStep({ op: "type", text: "x" }), "moderate");

assert.equal(
  needsComputerConfirm([{ op: "screenshot" }], { confirmAll: false, confirmDestructive: true }),
  false,
);
assert.equal(
  needsComputerConfirm([{ op: "click", x: 1, y: 2 }], {
    confirmAll: false,
    confirmDestructive: true,
  }),
  true,
);

const blob = [
  "Looking at the screen now.",
  "COMPUTER_CMD: screenshot",
  "COMPUTER_CMD: click 10 20",
  "Done.",
].join("\n");
const cmds = extractComputerCommands(blob);
assert.equal(cmds.length, 2);
assert.equal(cmds[0].op, "screenshot");
assert.equal(cmds[1].op, "click");
const stripped = stripComputerCommands(blob);
assert.equal(/COMPUTER_CMD/i.test(stripped), false);
assert.match(stripped, /Looking at the screen/);

const recipe = parseComputerRecipe({
  version: 1,
  screen: { width: 1920, height: 1080 },
  steps: cmds,
  summary: "open a site",
});
assert.ok(recipe);
assert.equal(recipe.steps.length, 2);
assert.equal(formatComputerCommand(recipe.steps[1]), "click 10 20");

const vision = require("../desktop/vision-messages.cjs");
const hydrated = vision.hydrateForXai(
  [
    { role: "user", content: "see this", images: ["data:image/png;base64,aaa"] },
    { role: "assistant", content: "ok" },
    { role: "user", content: "again", images: ["data:image/jpeg;base64,bbb"] },
    { role: "user", content: "third", images: ["data:image/jpeg;base64,ccc"] },
  ],
  { maxImages: 2 },
);
const imgCount = hydrated.filter(
  (m) => Array.isArray(m.content) && m.content.some((p) => p && p.type === "image_url"),
).length;
assert.equal(imgCount, 2, "cap to last 2 screenshots");
assert.equal(typeof hydrated[0].content, "string", "oldest screenshot stripped to text");

const computer = require("../desktop/computer-bridge.cjs");
assert.equal(typeof computer.info, "function");
assert.equal(typeof computer.act, "function");
const info = computer.info();
assert.equal(info.ok, true);
assert.ok(info.session === "wayland" || info.session === "x11" || info.session === "unknown");
const shot = await computer.act({ op: "screenshot" });
assert.equal(shot.ok, false);
assert.match(String(shot.error), /Electron|desktop/i);

console.log("smoke-computer OK");
