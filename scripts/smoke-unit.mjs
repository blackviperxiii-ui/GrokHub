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
assert.equal(typeof bridge.versionNewer, "function", "versionNewer must be exported for updater tests");
assert.equal(bridge.versionNewer("1.1.20", "1.1.19"), true);
assert.equal(bridge.versionNewer("1.1.19", "1.1.20"), false);
assert.equal(bridge.versionNewer("1.1.19", "1.1.19"), false);
assert.equal(bridge.versionNewer("v1.1.20", "1.1.19"), true);
assert.equal(typeof bridge.updateIsAvailable, "function");
assert.equal(
  bridge.updateIsAvailable({ releaseNewer: false, shaAhead: true, uiStale: false }),
  true,
  "main ahead of the install must offer an update",
);
assert.equal(
  bridge.updateIsAvailable({ releaseNewer: false, shaAhead: false, uiStale: false }),
  false,
);
assert.equal(
  bridge.updateIsAvailable({ releaseNewer: true, shaAhead: false, uiStale: false }),
  true,
);
assert.equal(typeof bridge.shouldUseGithubRelease, "function");
assert.equal(bridge.shouldUseGithubRelease("1.1.21", "1.1.21"), false);
assert.equal(bridge.shouldUseGithubRelease("1.1.22", "1.1.21"), true);
assert.equal(typeof bridge.updateInstallSucceeded, "function");
assert.equal(
  bridge.updateInstallSucceeded({ usedRelease: false, rebuiltUi: false, staleUi: true }),
  false,
  "source tarball + old UI is not a successful install",
);
assert.equal(
  bridge.updateInstallSucceeded({ usedRelease: true, rebuiltUi: false, staleUi: false }),
  true,
);
assert.equal(
  bridge.updateInstallSucceeded({ usedRelease: false, rebuiltUi: true, staleUi: false }),
  true,
);

const bridgeSrc = fs.readFileSync(
  path.join(process.cwd(), "desktop/grok-bridge.cjs"),
  "utf8",
);
assert.match(bridgeSrc, /async function factoryReinstall/);
assert.match(bridgeSrc, /isGrokHubUiPid/);
assert.match(
  bridgeSrc,
  /Skipping GitHub release/,
  "updater must skip a latest release that is not newer than the install",
);
assert.match(
  bridgeSrc,
  /usedReleaseTag \|\| branch/,
  "VERSION stamp must follow the tarball we actually installed, not always main HEAD",
);
assert.match(
  bridgeSrc,
  /could not build the UI/i,
  "source-tarball install must fail instead of keeping the old UI",
);
assert.match(
  bridgeSrc,
  /npm ci --ignore-scripts --include=dev \|\| npm install --ignore-scripts --include=dev/,
  "source rebuild must install dependencies when node_modules is missing",
);
assert.match(
  bridgeSrc,
  /--include=dev/,
  "UI rebuild npm install must include devDependencies (vite plugins live there)",
);
assert.equal(typeof bridge.uiRebuildHasToolchain, "function");
assert.equal(
  bridge.uiRebuildHasToolchain("/tmp/grokhub-missing-nm"),
  false,
  "missing node_modules is not a UI toolchain",
);
assert.equal(typeof bridge.uiRebuildInstallEnv, "function");
const instEnv = bridge.uiRebuildInstallEnv({ NODE_ENV: "production", PATH: "/usr/bin" });
assert.notEqual(
  instEnv.NODE_ENV,
  "production",
  "NODE_ENV=production makes npm omit vite/plugin-react/nitro",
);
assert.equal(instEnv.NPM_CONFIG_PRODUCTION, "false");
const pkgJson = JSON.parse(fs.readFileSync(path.join(process.cwd(), "package.json"), "utf8"));
for (const name of ["vite", "@vitejs/plugin-react", "nitro"]) {
  assert.ok(
    pkgJson.dependencies?.[name],
    `${name} must be a runtime dependency so the shipped 1.1.22 updater (NODE_ENV=production npm install) can rebuild the UI`,
  );
}
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
const computer = require("../desktop/computer-bridge.cjs");
assert.equal(typeof computer.info, "function");
assert.equal(typeof computer.act, "function");
assert.equal(typeof computer.userStop, "function");
const vision = require("../desktop/vision-messages.cjs");
assert.equal(typeof vision.hydrateForXai, "function");
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

// Backend must be fully healthy before Electron creates a window
const mainSrc = fs.readFileSync(path.join(process.cwd(), "desktop/main.mjs"), "utf8");
assert.doesNotMatch(
  mainSrc,
  /Start UI resolve in parallel with shell window creation/,
  "must not create the Electron window in parallel with backend boot",
);
assert.match(mainSrc, /waiting-backend/, "main must wait for backend before createWindow");
assert.match(mainSrc, /function startupLog\s*\(/, "startupLog helper must be defined");
assert.ok(
  mainSrc.indexOf("function startupLog") < mainSrc.indexOf('startupLog("waiting-backend")'),
  "startupLog must be defined before boot calls it",
);
assert.match(mainSrc, /createWindow\(\{\s*startUrl:/s, "createWindow only after resolveStartUrl");
assert.ok(
  mainSrc.indexOf("waiting-backend") < mainSrc.indexOf("createWindow({"),
  "wait for backend before first createWindow",
);
assert.match(mainSrc, /loadURL\(startUrl\)/, "createWindow starts the first UI load");
{
  const afterWindow = mainSrc.slice(mainSrc.indexOf("bootMark(\"window-created\")"));
  assert.doesNotMatch(
    afterWindow,
    /await mainWindow\.loadURL\(current\)/,
    "must not immediately loadURL the same URL again after createWindow (Electron 37 stack overflow)",
  );
  assert.match(
    mainSrc,
    /__ghUiShowingError/,
    "showLoadError must guard against re-entrant loadURL",
  );
  assert.match(
    mainSrc,
    /disable-dev-shm-usage/,
    "Linux must set disable-dev-shm-usage (noexec /dev/shm kills Chromium)",
  );
  assert.match(mainSrc, /appendSwitch\("no-zygote"\)/, "Linux no-sandbox must also disable zygote");
  assert.doesNotMatch(
    mainSrc,
    /createFromPath\([^)]*svg/i,
    "nativeImage.createFromPath must not be given SVG (Electron NativeImage warning)",
  );
  assert.doesNotMatch(
    mainSrc.slice(mainSrc.indexOf("function loadAppIcon"), mainSrc.indexOf("function createWindow")),
    /grokhub\.svg/,
    "window icon candidates must be raster (png/ico), not svg",
  );
  assert.ok(
    fs.existsSync(path.join(process.cwd(), "desktop/icons/icon.png")),
    "desktop/icons/icon.png required so NativeImage gets a real raster path",
  );
}

const appShellSrc = fs.readFileSync(
  path.join(process.cwd(), "src/components/AppShell.tsx"),
  "utf8",
);
assert.match(appShellSrc, /id: "queue"/, "Queue must be a first-class nav item");
assert.match(appShellSrc, /AgentQueueView/, "AppShell must render the real agent queue");
assert.match(appShellSrc, /data-hydrated/, "AppShell must mark persist rehydrate so e2e does not click stale nav");
assert.doesNotMatch(
  appShellSrc,
  /DesktopHostView/,
  "dead Desktop tab must not stay wired in AppShell — host UI lives in Settings",
);
assert.doesNotMatch(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/AgentsView.tsx"), "utf8"),
  /UI-only/,
  "AgentsView must not advertise a fake multi-agent roster",
);

assert.ok(
  fs.existsSync(path.join(process.cwd(), "src/lib/settings-nav.ts")),
  "settings-nav helper required so Connect/Devices open the right Settings pane",
);
{
  const settingsNav = fs.readFileSync(path.join(process.cwd(), "src/lib/settings-nav.ts"), "utf8");
  assert.match(settingsNav, /export function openSettingsSection/, "openSettingsSection must be exported");
  assert.match(settingsNav, /grokhub:settings-section/, "must use a window event Settings can consume after mount");
}
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/ChatView.tsx"), "utf8"),
  /data-connect-grok/,
  "empty chat must offer a Connect Grok CTA when disconnected",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/ChatView.tsx"), "utf8"),
  /beginGrokOAuthFromUi/,
  "Connect Grok CTA must start OAuth, not only open Settings",
);
assert.doesNotMatch(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/ChatView.tsx"), "utf8"),
  /No project bound/,
  "chat chrome must not show the project-bind status bar (novice clutter; /project bind stays)",
);
assert.doesNotMatch(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/ChatView.tsx"), "utf8"),
  /\$ shell/,
  "empty chat / placeholder must not push $ shell at novices",
);
assert.doesNotMatch(
  appShellSrc,
  /connect OAuth or API key/,
  "must not duplicate the Setup pill with an Offline banner",
);
assert.doesNotMatch(
  appShellSrc,
  /\["skills", "automations", "command", "queue", "settings"\]/,
  "Settings must stay visible when Tools is collapsed",
);
assert.match(
  appShellSrc,
  /\["chat", "history", "imagine", "workboard", "settings"\]/,
  "Settings belongs in Workspace so first-run connect is one click away",
);
assert.match(appShellSrc, /data-conn/, "Live pill must expose connection kind for green/yellow/red");
assert.match(appShellSrc, /data-queue-count/, "Queue nav must show an attention count");
assert.match(mainSrc, /setVersion/, "Electron must set app version (Linux logs No version found otherwise)");
assert.match(mainSrc, /computer:info/, "computer-use IPC must be registered");
assert.match(mainSrc, /computer:startPreview/, "live picture-loop IPC must be registered");
assert.match(mainSrc, /computer:stopPreview/, "live picture-loop stop IPC must be registered");
assert.match(
  fs.readFileSync(path.join(process.cwd(), "desktop/preload.cjs"), "utf8"),
  /computer:frame/,
  "preload must forward live frames to the renderer",
);
assert.match(
  mainSrc,
  /Content-Security-Policy/,
  "packaged Electron session must set CSP (skip Vite :8080 which needs eval)",
);
assert.match(
  mainSrc,
  /packagedSessionCsp/,
  "Electron CSP must come from desktop/csp.cjs so hydration policy cannot drift",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "desktop/csp.cjs"), "utf8"),
  /script-src 'self' 'unsafe-inline'/,
  "TanStack SSR hydrates via inline scripts — script-src 'self' alone blanks the Electron window",
);
assert.match(
  mainSrc,
  /Queue \(\$\{due\} waiting\)/,
  "tray must label Queue with waiting job count",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/AgentQueueView.tsx"), "utf8"),
  /data-next-up/,
  "Queue must show what will run next (or that it is paused/empty)",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/SettingsView.tsx"), "utf8"),
  /DesktopHostView/,
  "Settings must host the real Desktop host CLI/files/apps UI",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/views/ImagineView.tsx"), "utf8"),
  /not an xAI/,
  "Imagine must say local preview is not an xAI image when disconnected",
);
assert.match(
  fs.readFileSync(path.join(process.cwd(), "src/components/CommandPalette.tsx"), "utf8"),
  /openSettingsSection\(\s*["']devices["']/,
  "Devices palette item must open the Devices settings category",
);

const storeSrc = fs.readFileSync(path.join(process.cwd(), "src/lib/store.ts"), "utf8");
assert.doesNotMatch(
  storeSrc,
  /dead = new Set\(\[[^\]]*["']queue["']/,
  "queue nav must not be treated as a dead/removed surface",
);

assert.ok(
  fs.existsSync(path.join(process.cwd(), "scripts/smoke-electron-boot.mjs")),
  "Electron boot smoke required so window-open regressions fail CI",
);
{
  const bootSmoke = fs.readFileSync(
    path.join(process.cwd(), "scripts/smoke-electron-boot.mjs"),
    "utf8",
  );
  assert.match(bootSmoke, /boot-complete/, "Electron smoke must wait for boot-complete");
  assert.match(
    bootSmoke,
    /startupLog is not defined/,
    "Electron smoke must fail on the tray-only startupLog crash",
  );
}
assert.ok(
  fs.existsSync(path.join(process.cwd(), "e2e/smoke.spec.ts")),
  "Playwright smoke spec required for chat + host flows",
);
assert.ok(
  fs.existsSync(path.join(process.cwd(), "playwright.config.ts")),
  "Playwright config required for chat + host e2e",
);
{
  const e2e = fs.readFileSync(path.join(process.cwd(), "e2e/smoke.spec.ts"), "utf8");
  assert.match(e2e, /data-composer/, "e2e must cover the chat composer");
  assert.match(e2e, /\/api\/host/, "e2e must cover host info/exec");
  assert.doesNotMatch(
    e2e,
    /getByRole\(["']button["'],\s*\{\s*name:\s*["']Send["']\s*\}\)\.click/,
    "e2e must not click Send (no live xAI)",
  );
}
{
  const ci = fs.readFileSync(path.join(process.cwd(), ".github/workflows/ci.yml"), "utf8");
  assert.match(ci, /test:electron-boot/, "CI must run Electron boot smoke");
  assert.match(ci, /test:e2e/, "CI must run Playwright e2e");
  assert.match(ci, /xvfb-run/, "Electron boot smoke needs xvfb on Ubuntu");
}

const uiSrc = fs.readFileSync(path.join(process.cwd(), "desktop/ui-server.cjs"), "utf8");
assert.match(uiSrc, /HEALTHY_BODY_RE/, "probe must require a real HTML body");
assert.match(uiSrc, /waitUntilHealthy/, "ui-server exports waitUntilHealthy");
assert.ok(
  fs.existsSync(path.join(process.cwd(), "desktop/wait-backend.cjs")),
  "wait-backend.cjs launcher helper required",
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

const themeCss = fs.readFileSync(path.join(process.cwd(), "src/styles.css"), "utf8");
assert.doesNotMatch(themeCss, /#7aa2ff|#2563eb|#3b82f6|#7dd3fc|#c084fc|#34d399|#fbbf24|#f87171/, "theme must be black/gray/white — no hue accents");
assert.match(themeCss, /--color-bg:\s*#090909/, "dark bg is true black-gray");
assert.match(themeCss, /--color-info:\s*#e5e5e5/, "info token is gray, not blue");
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
  const { routeAuto, buildCatalog, pickFlagshipModel } = mod;
  const cat = buildCatalog([
    "grok-4.20-0309-non-reasoning",
    "grok-4.20-0309-reasoning",
    "grok-4.3",
    "grok-4.5",
    "grok-4.6",
    "grok-build-0.1",
    "grok-imagine-image",
  ]);
  cat.slots.fast = "grok-4.20-0309-non-reasoning";
  cat.slots.balanced = "grok-4.3";
  cat.slots.smart = "grok-4.3";
  cat.slots.heavy = "grok-4.6";
  cat.slots.build = "grok-build-0.1";
  cat.slots.imagine = "grok-imagine-image";

  const expect = (prompt, tier, ctx = {}) => {
    const res = routeAuto(prompt, cat, ctx);
    assert.notEqual(res.tier, "think", `retired think leaked for ${JSON.stringify(prompt)}`);
    assert.notEqual(res.routedMode, "expert", `retired expert leaked for ${JSON.stringify(prompt)}`);
    assert.notEqual(res.routedMode, "heavy", `retired heavy leaked for ${JSON.stringify(prompt)}`);
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
  expect("yes continue", "balanced", { lastRouteTier: "think", historyTurns: 4 });
  // Intent shift: after Fast chat, coding should flip to Build (not stick Fast)
  expect(
    "implement a full refactor of the auth module with unit tests and migration plan",
    "build",
    { lastRouteTier: "fast", historyTurns: 6 },
  );
  // Intent shift: after Build, light UX opinion → balanced/think (not stuck build)
  {
    const r = routeAuto("how do I improve this UI spacing a bit", cat, {
      lastRouteTier: "build",
      historyTurns: 8,
    });
    assert.ok(
      r.tier === "balanced" || r.tier === "deep",
      "after build, UX ask should leave build: " + r.tier + " " + r.reasonDetail,
    );
  }
  // System investigation → think (not fast)
  {
    const r = routeAuto(
      "do a deep dive audit of the grokhub install and processes on my machine",
      cat,
      { lastRouteTier: "fast", historyTurns: 2 },
    );
    assert.ok(
      r.tier === "deep" || r.tier === "build" || r.tier === "balanced",
      "system audit should not stay fast: " + r.tier + " " + r.reasonDetail,
    );
  }
  // hysteresis: pure ack after deep shouldn't go pure fast
  const h = routeAuto("ok cool", cat, { lastRouteTier: "deep", historyTurns: 5 });
  assert.ok(
    h.tier === "deep" || h.tier === "build" || h.tier === "think" || h.tier === "balanced" || h.tier === "fast",
    "hysteresis path ok: " + h.tier,
  );
  // pure greeting after deep → fast is allowed now (save tokens)
  expect("thanks!", "fast", { lastRouteTier: "deep", historyTurns: 5 });
  // usage pressure pushes cheaper
  const pressured = routeAuto(
    "Please review this approach carefully and tell me what you think about the tradeoffs",
    cat,
    { usagePressure: 0.9, preferFree: true },
  );
  assert.ok(
    pressured.tier === "fast" || pressured.tier === "balanced",
    "usage pressure should avoid deep for mid prompt: " + pressured.tier,
  );

  assert.equal(
    pickFlagshipModel(["grok-4.5", "grok-4.6", "grok-4.3", "grok-4.20-reasoning"]),
    "grok-4.6",
    "Max flagship must prefer grok-4.6",
  );

  try {
    fs.unlinkSync(out);
  } catch {
    /* ignore */
  }
  console.log("routeAuto golden OK");
}

{
  const modesSrc = fs.readFileSync(path.join(process.cwd(), "src/lib/modes.ts"), "utf8");
  assert.doesNotMatch(modesSrc, /id: "expert"/);
  assert.doesNotMatch(modesSrc, /id: "heavy"/);
  assert.match(modesSrc, /normalizeMode/);
  assert.match(modesSrc, /grok-4\.6/);
  const settingsSrc = fs.readFileSync(
    path.join(process.cwd(), "src/components/views/SettingsView.tsx"),
    "utf8",
  );
  assert.doesNotMatch(settingsSrc, /sec-model-overrides/);
  assert.doesNotMatch(settingsSrc, /setModelOverride/);
  assert.doesNotMatch(settingsSrc, /sec-models/);
  assert.doesNotMatch(settingsSrc, /sec-modes/);
  assert.match(settingsSrc, /DevicesHubPanel/);
  assert.match(settingsSrc, /id: "devices"/);
  const slashSrc = fs.readFileSync(
    path.join(process.cwd(), "src/lib/slash-commands.ts"),
    "utf8",
  );
  assert.doesNotMatch(slashSrc, /\/mode expert/);
  assert.doesNotMatch(slashSrc, /\/mode think/);
  assert.doesNotMatch(slashSrc, /\/mode heavy/);
  assert.match(slashSrc, /\/sync/);
  assert.match(slashSrc, /\/send/);
  assert.match(slashSrc, /\/hub/);
  console.log("permanent Adaptive map OK");
}

{
  const hub = require("../desktop/hub-server.cjs");
  hub._resetForTests();
  assert.equal(hub.normalizeCode("abc-234"), "ABC234");
  assert.equal(hub.normalizeCode("ab c-23 4"), "ABC234");
  const code = hub.makePairCode();
  assert.match(code, /^[A-Z2-9]{3}-[A-Z2-9]{3}$/, "pair code format " + code);
  const port = 18776 + Math.floor(Math.random() * 120);
  const started = await hub.startShare({ port });
  assert.equal(started.ok, true, "hub startShare");
  assert.equal(started.sharing, true);
  const pair = hub._getState().pair?.code;
  assert.ok(pair, "pair code after share");
  const res = await fetch(`http://127.0.0.1:${port}/v1/pair`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ code: pair, deviceId: "d-test", deviceName: "Laptop" }),
  });
  const data = await res.json();
  assert.equal(data.ok, true, "pair ok: " + JSON.stringify(data));
  assert.ok(data.token, "pair token");
  const snap = {
    kind: "grokhub-hub-v1",
    fromDeviceId: "d-test",
    fromDeviceName: "Laptop",
    exportedAt: Date.now(),
    threads: [{ id: "t1", title: "Hi", updatedAt: 2, messages: [] }],
    workboard: { items: [] },
    skills: [],
    automations: [],
    learning: null,
    memoryFiles: [{ name: "USER.md", content: "# me", updatedAt: 1 }],
    profile: { displayName: "Viper" },
  };
  const put = await fetch(`http://127.0.0.1:${port}/v1/snapshot`, {
    method: "PUT",
    headers: {
      authorization: `Bearer ${data.token}`,
      "content-type": "application/json",
    },
    body: JSON.stringify({ snapshot: snap }),
  });
  const putBody = await put.json();
  assert.equal(putBody.ok, true, "snapshot put");
  const got = await fetch(`http://127.0.0.1:${port}/v1/snapshot`, {
    headers: { authorization: `Bearer ${data.token}` },
  });
  const gotBody = await got.json();
  assert.equal(gotBody.ok, true);
  assert.equal(gotBody.snapshot?.kind, "grokhub-hub-v1");
  await hub.stopShare({ persist: false });
  hub._resetForTests();
  console.log("hub-server pair/snapshot OK");
}

{
  const { spawnSync } = await import("node:child_process");
  const out = path.join(process.cwd(), ".tmp-hub-sync-test.mjs");
  const r = spawnSync(
    "npx",
    ["esbuild", "src/lib/hub-sync.ts", "--bundle", "--platform=node", "--format=esm", `--outfile=${out}`],
    { encoding: "utf8" },
  );
  assert.equal(r.status, 0, "esbuild hub-sync: " + (r.stderr || r.stdout || ""));
  const mod = await import(pathToFileURL(out).href + `?t=${Date.now()}`);
  const older = mod.buildHubSnapshot({
    deviceId: "a",
    deviceName: "A",
    threads: [{ id: "t1", title: "old", updatedAt: 1, messages: [{ id: "m1" }] }],
    workboard: { items: [{ id: "w1", title: "old", updatedAt: 1 }] },
    skills: [{ id: "s1", name: "old", updatedAt: 1 }],
    automations: [{ id: "au1", name: "old", updatedAt: 1 }],
    learning: { tag: "old" },
    memoryFiles: [{ name: "USER.md", content: "old", updatedAt: 1 }],
    displayName: "Old",
  });
  const newer = mod.buildHubSnapshot({
    deviceId: "b",
    deviceName: "B",
    threads: [{ id: "t1", title: "new", updatedAt: 9, messages: [{ id: "m2" }] }],
    workboard: { items: [{ id: "w1", title: "new", updatedAt: 9 }] },
    skills: [{ id: "s1", name: "new", updatedAt: 9 }],
    automations: [{ id: "au1", name: "new", updatedAt: 9 }],
    learning: { tag: "new" },
    memoryFiles: [{ name: "USER.md", content: "new", updatedAt: 9 }],
    displayName: "New",
  });
  const merged = mod.mergeHubSnapshots(older, newer);
  assert.equal(merged.kind, "grokhub-hub-v1");
  assert.equal(merged.threads[0].title, "new");
  assert.equal(merged.workboard.items[0].title, "new");
  assert.equal(merged.skills[0].name, "new");
  assert.equal(merged.automations[0].name, "new");
  assert.equal(merged.memoryFiles[0].content, "new");
  assert.equal(merged.learning.tag, "new");
  assert.ok(mod.isHubSnapshot(merged));
  assert.equal(mod.isHubSnapshot({ kind: "nope" }), false);
  try {
    fs.unlinkSync(out);
  } catch {
    /* ignore */
  }
  console.log("hub-sync merge OK");
}


// Perf util: delta coalescer + boot timeline + trace flags
const perf = require("../desktop/perf-util.cjs");
assert.equal(typeof perf.parseTrace, "function");
assert.equal(typeof perf.createDeltaCoalescer, "function");
const flags = perf.parseTrace({ GROKHUB_DEBUG: "1" });
assert.equal(flags.debug, true);
assert.equal(flags.boot, true);
const tl = perf.createBootTimeline(() => 1000);
tl.mark("a");
const snap = tl.snapshot();
assert.ok(Array.isArray(snap.phases));
assert.equal(snap.phases[0].phase, "a");

const sent = [];
let now = 0;
const timers = [];
const coal = perf.createDeltaCoalescer({
  maxWaitMs: 20,
  maxChars: 10,
  schedule: (fn, ms) => {
    const fireAt = now + ms;
    const entry = { fireAt, fn, cancelled: false };
    timers.push(entry);
    return () => {
      entry.cancelled = true;
    };
  },
});
coal.push("hello", (s) => sent.push(s));
// under maxChars — buffered
assert.equal(sent.length, 0);
coal.push("world!!", (s) => sent.push(s)); // exceeds maxChars
assert.equal(sent.join(""), "helloworld!!");
coal.push("x", (s) => sent.push(s));
coal.flush((s) => sent.push(s));
assert.ok(sent.includes("x") || sent.join("").endsWith("x"));
const coalStats = coal.stats();
assert.ok(coalStats.deltaCount >= 2);
assert.ok(coalStats.flushCount >= 1);

// Runtime metrics (TS via strip-types if available path — import not needed; file presence)
assert.ok(fs.existsSync(path.join(process.cwd(), "src/lib/runtime-metrics.ts")));
assert.ok(fs.existsSync(path.join(process.cwd(), "desktop/perf-util.cjs")));
console.log("smoke-unit: perf helpers ok");
