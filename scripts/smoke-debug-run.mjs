#!/usr/bin/env node
/** Debug-run regressions: OOM history, sendChat gate, abort, autostart, safe mode. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const vision = await import(pathToFileURL(path.join(root, "src/lib/grok-vision.ts")).href);
const { capHistoryImages, capHistoryImagesInPlace } = vision;

const fat = (n) => ({
  role: "user",
  content: `shot ${n}`,
  images: [`data:image/png;base64,${"A".repeat(80)}`],
});
const live = [fat(1), { role: "assistant", content: "ok" }, fat(2), fat(3), fat(4)];
const copy = capHistoryImages(live, 2);
assert.equal(copy.filter((m) => m.images?.length).length, 2);
assert.equal(live.filter((m) => m.images?.length).length, 4, "copy cap must not mutate live history");

capHistoryImagesInPlace(live, 2);
assert.equal(live.filter((m) => m.images?.length).length, 2);
assert.equal(live.at(-1)?.images?.length, 1);
assert.equal(live[0].images, undefined);

const storeSrc = fs.readFileSync(path.join(root, "src/lib/store.ts"), "utf8");
assert.match(storeSrc, /capHistoryImagesInPlace\(history,\s*2\)/);
assert.match(storeSrc, /let sendChatBusy = false/);
assert.match(storeSrc, /if \(sendChatBusy \|\| get\(\)\.running\)/);
assert.match(storeSrc, /accepted === false/);
assert.match(storeSrc, /status:\s*"queued"/);
assert.match(storeSrc, /function abortActiveChatWork\(/);
assert.match(storeSrc, /abortWork:\s*true/);
assert.match(
  storeSrc,
  /stopChat:[\s\S]*processAgentQueue\(\)/,
  "Stop must drain the Hands-on queue",
);
assert.match(storeSrc, /runSelfImprove:[\s\S]*Skipped while a turn is running/);
assert.match(storeSrc, /desktopEntry\?\.autostart/);

const shellSrc = fs.readFileSync(path.join(root, "src/components/AppShell.tsx"), "utf8");
assert.match(shellSrc, /desktopEntry\?\.autostart/);

const hostSrc = fs.readFileSync(path.join(root, "desktop/host-bridge.cjs"), "utf8");
assert.match(
  hostSrc,
  /let safeMode = process\.env\.GROKHUB_HOST_SAFE !== "0"/,
  "host safe mode must default on",
);

const settingsSrc = fs.readFileSync(
  path.join(root, "src/components/views/SettingsView.tsx"),
  "utf8",
);
assert.match(
  settingsSrc,
  /id="sec-setup-sync"\s+data-settings-cat="account"/,
);

const grokSrc = fs.readFileSync(path.join(root, "desktop/grok-bridge.cjs"), "utf8");
assert.doesNotMatch(
  grokSrc,
  /version:\s*newVersion[\s\S]{0,200}JSON\.stringify\(\{\s*pending:\s*true/,
);
assert.doesNotMatch(grokSrc, /JSON\.stringify\(\{\s*pending:\s*true/);

const mainSrc = fs.readFileSync(path.join(root, "desktop/main.mjs"), "utf8");
assert.match(
  mainSrc,
  /update:factory[\s\S]*app\.exit\(0\)/,
  "factory reinstall must exit like apply/rollback",
);

const learnSrc = fs.readFileSync(path.join(root, "src/lib/session-learn.ts"), "utf8");
assert.match(learnSrc, /reflectMarkdown/);
assert.equal(
  (learnSrc.match(/reflectLearning\(/g) || []).length,
  1,
  "reflect once per turn — reuse markdown",
);

const ciSrc = fs.readFileSync(path.join(root, ".github/workflows/ci.yml"), "utf8");
assert.match(ciSrc, /node_modules\/electron\/install\.js/);
assert.match(ciSrc, /ELECTRON_DISABLE_SANDBOX=1/);

const bootSrc = fs.readFileSync(path.join(root, "scripts/smoke-electron-boot.mjs"), "utf8");
assert.match(
  bootSrc,
  /"--no-sandbox"/,
  "Electron boot smoke must pass --no-sandbox at spawn (JS appendSwitch is too late for SUID chrome-sandbox)",
);

console.log("smoke-debug-run OK");
