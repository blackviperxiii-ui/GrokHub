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
assert.equal(typeof computer.ffmpegX11grabArgs, "function");
const ffArgs = computer.ffmpegX11grabArgs(":0", "OUT", { width: 1920, height: 1080 });
assert.ok(ffArgs.includes("x11grab"), "Cursor-style capture is ffmpeg x11grab, not a screenshot app");
assert.ok(ffArgs.includes("-frames:v"));
assert.equal(
  ffArgs.some((a) => /spectacle|flameshot|gnome-screenshot/i.test(String(a))),
  false,
);

const tools = computer.silentCaptureTools("wayland", {
  grim: "/usr/bin/grim",
  ffmpeg: "/usr/bin/ffmpeg",
  display: ":0",
  maim: "/usr/bin/maim",
  scrot: "/usr/bin/scrot",
  "gnome-screenshot": "/usr/bin/gnome-screenshot",
  spectacle: "/usr/bin/spectacle",
  flameshot: "/usr/bin/flameshot",
});
assert.equal(tools[0]?.name, "grim");
assert.equal(
  tools.some((t) => t.name === "ffmpeg-x11grab"),
  false,
  "Wayland + grim must not x11grab the XWayland root",
);
assert.equal(
  tools.some((t) =>
    /spectacle|flameshot|gnome-screenshot|desktopCapturer/i.test(t.name),
  ),
  false,
  "must not launch interactive screenshot apps or Electron portal picker",
);
assert.equal(
  computer.previewCaptureKind("wayland", {
    grim: "/usr/bin/grim",
    ffmpeg: "/usr/bin/ffmpeg",
    display: ":0",
  }),
  "grim",
  "live view must follow grim-first, not DISPLAY+ffmpeg",
);
assert.equal(
  computer.silentCaptureTools("wayland", {
    ffmpeg: "/usr/bin/ffmpeg",
    display: ":0",
  })[0]?.name,
  "ffmpeg-x11grab",
  "x11grab is only a Wayland fallback when grim is missing",
);

const x11tools = computer.silentCaptureTools("x11", {
  ffmpeg: "/usr/bin/ffmpeg",
  display: ":1",
  "gnome-screenshot": "/usr/bin/gnome-screenshot",
});
assert.equal(x11tools[0]?.name, "ffmpeg-x11grab");
assert.deepEqual(computer.silentCaptureTools("unknown", {}), []);

assert.equal(typeof computer.splitJpegFrames, "function");
const jpegFrames = [];
const pushJpeg = computer.splitJpegFrames((j) => jpegFrames.push(j));
const oneJpeg = Buffer.concat([
  Buffer.from([0xff, 0xd8]),
  Buffer.from("frame"),
  Buffer.from([0xff, 0xd9]),
]);
pushJpeg(Buffer.concat([Buffer.from("xx"), oneJpeg, oneJpeg]));
assert.equal(jpegFrames.length, 2, "MJPEG pipe must split concatenated JPEGs");

const decoy = [];
const pushDecoy = computer.splitJpegFrames((j) => decoy.push(j));
const comJpeg = Buffer.from([
  0xff, 0xd8, 0xff, 0xfe, 0x00, 0x04, 0xff, 0xd9, 0xff, 0xd9,
]);
pushDecoy(comJpeg);
assert.equal(decoy.length, 1, "FF D9 inside a COM segment is not end-of-image");
assert.equal(decoy[0].length, comJpeg.length);

const splitSoi = [];
const pushSplit = computer.splitJpegFrames((j) => splitSoi.push(j));
pushSplit(Buffer.from([0xff]));
pushSplit(Buffer.from([0xd8, 0x00, 0xff, 0xd9]));
assert.equal(splitSoi.length, 1, "SOI split across chunks must not be dropped");

assert.equal(typeof computer.mapToScreen, "function");
assert.equal(
  computer.mapToScreen(100, 50, { width: 1280, height: 800, screenWidth: 0, screenHeight: 0 }).mapped,
  false,
  "zero screen size must not map clicks to 0,0",
);
assert.equal(
  computer.mapToScreen(640, 400, {
    width: 1280,
    height: 800,
    screenWidth: 1920,
    screenHeight: 1200,
  }).x,
  960,
);

assert.equal(typeof computer.injectorScreenSize, "function");
assert.deepEqual(
  computer.injectorScreenSize({
    injector: "xdotool",
    x11: { width: 3840, height: 2160 },
    electron: { width: 1920, height: 1080, scaleFactor: 2 },
  }),
  { width: 3840, height: 2160 },
  "xdotool clicks in X11 pixels, not Electron DIP",
);
assert.deepEqual(
  computer.injectorScreenSize({
    injector: "ydotool",
    x11: { width: 0, height: 0 },
    electron: { width: 1920, height: 1080, scaleFactor: 2 },
  }),
  { width: 3840, height: 2160 },
  "ydotool on HiDPI must use physical pixels (DIP × scale)",
);
assert.deepEqual(
  computer.injectorScreenSize({
    injector: "xdotool",
    x11: { width: 1920, height: 1080 },
    electron: { width: 1920, height: 1080, scaleFactor: 1 },
  }),
  { width: 1920, height: 1080 },
);

const hidpiMeta = computer.shotMetaForMapping(
  { width: 1280, height: 720 },
  computer.injectorScreenSize({
    injector: "xdotool",
    x11: { width: 3840, height: 2160 },
    electron: { width: 1920, height: 1080, scaleFactor: 2 },
  }),
);
assert.equal(computer.mapToScreen(640, 360, hidpiMeta).x, 1920, "center of 1280 shot → center of 3840 screen");
assert.equal(computer.mapToScreen(640, 360, hidpiMeta).y, 1080);

assert.equal(typeof computer.pointHitsScaledBounds, "function");
assert.equal(
  computer.pointHitsScaledBounds(400, 400, { x: 100, y: 100, width: 800, height: 600 }, 2),
  true,
  "pixel click vs DIP window must scale the window",
);
assert.equal(
  computer.pointHitsScaledBounds(150, 150, { x: 100, y: 100, width: 800, height: 600 }, 2),
  false,
  "DIP-space point must not hit a window that lives at 2× pixels",
);

assert.equal(typeof computer.previewStartVerdict, "function");
assert.equal(computer.previewStartVerdict({ gotFrame: true, procAlive: true, elapsedMs: 10, waitMs: 1500 }), "ready");
assert.equal(
  computer.previewStartVerdict({ gotFrame: false, procAlive: false, elapsedMs: 40, waitMs: 1500 }),
  "dead",
  "ffmpeg that exits before a frame is a failed live view",
);
assert.equal(
  computer.previewStartVerdict({ gotFrame: false, procAlive: true, elapsedMs: 80, waitMs: 1500 }),
  "wait",
);
assert.equal(
  computer.previewStartVerdict({ gotFrame: false, procAlive: true, elapsedMs: 1600, waitMs: 1500 }),
  "timeout-alive",
);
assert.equal(typeof computer.nextPreviewPlan, "function");
assert.equal(
  computer.nextPreviewPlan(
    [
      { name: "ffmpeg-x11grab" },
      { name: "grim" },
    ],
    "ffmpeg-x11grab",
  )?.name,
  "grim",
);
assert.deepEqual(
  computer.filterCapturePlans(
    [{ name: "ffmpeg-x11grab" }, { name: "grim" }, { name: "maim" }],
    "ffmpeg-x11grab",
  ).map((p) => p.name),
  ["grim", "maim"],
  "dead ffmpeg must not be retried on every grim/maim tick",
);

const { mapContainedImageClick } = await import("../src/lib/computer-geometry.ts");
const letterbox = {
  left: 0,
  top: 0,
  width: 800,
  height: 400,
};
// 1280×800 in an 800×400 box: scale=0.5, drawn 640×400, x-offset 80
assert.equal(
  mapContainedImageClick(40, 200, letterbox, 1280, 800),
  null,
  "click on the side letterbox must not fire a desktop click",
);
assert.deepEqual(mapContainedImageClick(80, 0, letterbox, 1280, 800), { x: 0, y: 0 });
assert.deepEqual(mapContainedImageClick(80 + 320, 200, letterbox, 1280, 800), { x: 640, y: 400 });
assert.equal(mapContainedImageClick(100, 50, { left: 0, top: 0, width: 0, height: 0 }, 1280, 800), null);

assert.equal(computer.previewAllowsDesktopCapturer(), false);
assert.doesNotMatch(
  fs.readFileSync(path.join(process.cwd(), "desktop/computer-bridge.cjs"), "utf8"),
  /desktopCapturer\.getSources/,
  "must not call Electron portal capture (opens a screenshot app every frame)",
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
assert.ok(info.capture === null || typeof info.capture === "string");
assert.match(String(info.hint), /xdotool|ydotool|grim|maim|ffmpeg/i);

const started = await computer.startPreview(400, "user");
if (started.ok) {
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
} else {
  assert.match(String(started.error), /ffmpeg|grim|silent/i);
  assert.equal(computer.isPreviewing(), false);
}
computer.stopPreview({ force: true });
computer.resetAbort();
const sessionPreview = await computer.startPreview(400, "session");
if (sessionPreview.ok) {
  computer.userStop();
  assert.equal(
    computer.isPreviewing(),
    false,
    "Stop must end a session-owned live view",
  );
}
computer.resetAbort();
computer.stopPreview({ force: true });
const shot = await computer.act({ op: "screenshot" });
if (shot.ok) {
  assert.ok(String(shot.dataUrl || "").startsWith("data:image/"));
  assert.notEqual(shot.capture, "desktopCapturer");
} else {
  assert.match(String(shot.error), /silent|ffmpeg|grim/i);
}

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

const hostViewSrc = fs.readFileSync(
  path.join(process.cwd(), "src/components/views/DesktopHostView.tsx"),
  "utf8",
);
assert.match(
  hostViewSrc,
  /mapContainedImageClick/,
  "live-view clicks must account for object-contain letterboxing",
);

console.log("smoke-computer OK");
