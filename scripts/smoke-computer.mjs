#!/usr/bin/env node
/** Computer-use protocol + vision hydrate (no display / injector required). */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
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
  computerPromptBlock,
} = await import("../src/lib/computer-protocol.ts");
const { hostAllowPrefixesFromConfirm, isHostAllowlisted } = await import(
  "../src/lib/host-safety.ts"
);
const { appendAssistantOnce } = await import("../src/lib/agent-tool-history.ts");

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
assert.equal(typeof computer.preferredInjectorKind, "function");
assert.equal(
  computer.preferredInjectorKind("wayland", { xdotool: "/usr/bin/xdotool", ydotool: "/usr/bin/ydotool" }),
  "ydotool",
  "Wayland must prefer ydotool — xdotool cannot click native Wayland apps",
);
assert.equal(
  computer.preferredInjectorKind("x11", { xdotool: "/usr/bin/xdotool", ydotool: "/usr/bin/ydotool" }),
  "xdotool",
);
assert.equal(typeof computer.silentCaptureTools, "function");
const tools = computer.silentCaptureTools("wayland", {
  grim: "/usr/bin/grim",
  maim: "/usr/bin/maim",
  scrot: "/usr/bin/scrot",
  "gnome-screenshot": "/usr/bin/gnome-screenshot",
  spectacle: "/usr/bin/spectacle",
  flameshot: "/usr/bin/flameshot",
});
assert.equal(tools[0]?.name, "grim");
assert.equal(
  tools.some((t) => t.name === "spectacle" || t.name === "flameshot"),
  false,
  "must not launch interactive screenshot apps",
);
assert.match(
  computerPromptBlock(true),
  /picture loop/i,
  "agent prompt must tell the model to use the live frame",
);

assert.equal(typeof computer.startPreview, "function");
assert.equal(typeof computer.stopPreview, "function");
assert.equal(typeof computer.isPreviewing, "function");
const info = computer.info();
assert.equal(info.ok, true);
assert.ok(info.session === "wayland" || info.session === "x11" || info.session === "unknown");
assert.ok(Array.isArray(info.captureTools), "info must list silent capture tools");
assert.ok(typeof info.capture === "string");
assert.match(String(info.hint), /xdotool|ydotool|grim|maim/i);

const started = computer.startPreview(400, "user");
assert.equal(started.ok, true);
assert.equal(started.previewing, true);
assert.equal(started.owner, "user");
assert.equal(computer.isPreviewing(), true);
const sessionStart = await computer.beginSession();
assert.equal(sessionStart.previewing, true);
const sessionEnd = computer.endSession();
assert.equal(sessionEnd.previewing, true, "user live view must survive an agent session");
assert.equal(computer.isPreviewing(), true);
const stopped = computer.stopPreview({ force: true });
assert.equal(stopped.previewing, false);
assert.equal(computer.isPreviewing(), false);
const shot = await computer.act({ op: "screenshot" });
assert.equal(shot.ok, false);
assert.match(String(shot.error), /Electron|desktop/i);

const computerPrefs = hostAllowPrefixesFromConfirm("computer", [
  "click 10 20",
  "type hello",
  "key ctrl+t",
]);
assert.deepEqual(computerPrefs, [], "computer confirm must not mint host allow prefixes");
const hostPrefs = hostAllowPrefixesFromConfirm("host", ["ls -la", "  cat /etc/os-release"]);
assert.deepEqual(hostPrefs, ["ls", "cat"]);
assert.equal(isHostAllowlisted("type script.sh", ["type"]), true);

const hist = [];
appendAssistantOnce(hist, "full turn");
appendAssistantOnce(hist, "full turn");
hist.push({ role: "user", content: "HOST_RESULT: ok" });
appendAssistantOnce(hist, "full turn");
hist.push({ role: "user", content: "COMPUTER_RESULT: ok" });
assert.equal(hist.filter((m) => m.role === "assistant").length, 1);
assert.equal(hist.length, 3);

assert.equal(typeof computer.restoreHiddenWindow, "function");
const restoreCalls = [];
computer.restoreHiddenWindow({
  showInactive() {
    restoreCalls.push("inactive");
  },
  show() {
    restoreCalls.push("show");
  },
});
assert.deepEqual(restoreCalls, ["inactive"], "showInactive must not fall through to show()");
const restoreFallback = [];
computer.restoreHiddenWindow({
  show() {
    restoreFallback.push("show");
  },
});
assert.deepEqual(restoreFallback, ["show"]);

assert.equal(typeof computer.ydotoolScrollArgs, "function");
const scrollDown = computer.ydotoolScrollArgs("down", 3);
assert.equal(scrollDown[0], "mousemove");
assert.ok(scrollDown.includes("--wheel") || scrollDown.includes("-w"));
assert.equal(scrollDown.includes("click"), false);
const scrollUp = computer.ydotoolScrollArgs("up", 1);
assert.ok(scrollUp.includes("--wheel") || scrollUp.includes("-w"));

computer.resetAbort();
const waitStarted = Date.now();
const waitP = computer.act({ op: "wait", ms: 4000 });
await new Promise((r) => setTimeout(r, 40));
computer.userStop();
computer.endSession();
const waitR = await waitP;
assert.equal(waitR.ok, false, "Stop during wait must fail the step");
assert.match(String(waitR.error), /stop/i);
assert.ok(Date.now() - waitStarted < 1500, "wait must abort without sleeping the full delay");
computer.resetAbort();

const chatSrc = fs.readFileSync(
  path.join(process.cwd(), "src/components/views/ChatView.tsx"),
  "utf8",
);
assert.match(chatSrc, /hostAllowPrefixesFromConfirm/);

console.log("smoke-computer OK");
