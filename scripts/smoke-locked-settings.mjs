#!/usr/bin/env node
/** Locked advanced settings — no UI knobs, forced defaults. */
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";

const root = process.cwd();
const mod = await import(pathToFileURL(path.join(root, "src/lib/locked-settings.ts")).href);
const {
  LOCKED,
  applyLockedAutonomy,
  applyLockedDesktop,
  applyLockedAgentPrefs,
  resolveSettingsCat,
} = mod;

assert.equal(LOCKED.autonomy.level, 4);
assert.equal(LOCKED.autonomy.autoClaimWorkboard, true);
assert.equal(LOCKED.autonomy.autoGoalResume, true);
assert.equal(LOCKED.desktop.hostSafeMode, true);
assert.equal(LOCKED.desktop.selfModifyEnabled, false);
assert.equal(LOCKED.desktop.confirmHostCommands, true);
assert.equal(LOCKED.desktop.confirmDestructiveOnly, true);
assert.equal(LOCKED.desktop.globalHotkey, "Super+Space");
assert.equal(LOCKED.desktop.wayland, true);
assert.equal(LOCKED.desktop.tray, true);
assert.equal(LOCKED.desktop.launchOnLogin, true);
assert.equal(LOCKED.agentPrefs.hostToolsEnabled, true);
assert.equal(LOCKED.agentPrefs.connectorToolsEnabled, true);
assert.equal(LOCKED.agentPrefs.computerUseEnabled, true);
assert.equal(LOCKED.agentPrefs.temperature, 0.7);

const forced = applyLockedAutonomy({
  level: 0,
  paused: true,
  dailyUnitBudget: 9,
  spentUnitsToday: 2,
  budgetDayKey: "2099-01-01",
  quietStartHour: 22,
  quietEndHour: 6,
  autoClaimWorkboard: false,
  autoGoalResume: false,
  maxQueue: 7,
  maxStepsPerGoal: 4,
  circuitBreakerFails: 9,
});
assert.equal(forced.level, 4);
assert.equal(forced.autoClaimWorkboard, true);
assert.equal(forced.autoGoalResume, true);
assert.equal(forced.paused, true, "tray pause stays operational");
assert.equal(forced.spentUnitsToday, 2);
assert.equal(forced.budgetDayKey, "2099-01-01");

const desk = applyLockedDesktop({
  startMinimized: true,
  launchOnLogin: false,
  wayland: false,
  tray: false,
  globalHotkey: "off",
  confirmHostCommands: false,
  confirmDestructiveOnly: false,
  selfModifyEnabled: true,
  hostSafeMode: false,
});
assert.equal(desk.hostSafeMode, true);
assert.equal(desk.selfModifyEnabled, false);
assert.equal(desk.confirmDestructiveOnly, true);
assert.equal(desk.launchOnLogin, true);

const prefs = applyLockedAgentPrefs({
  temperature: 0.1,
  hostToolsEnabled: false,
  connectorToolsEnabled: false,
  computerUseEnabled: false,
  memoryNotes: "keep me",
});
assert.equal(prefs.hostToolsEnabled, true);
assert.equal(prefs.computerUseEnabled, true);
assert.equal(prefs.temperature, 0.7);
assert.equal(prefs.memoryNotes, "keep me");

assert.equal(resolveSettingsCat("agent"), "app");
assert.equal(resolveSettingsCat("memory"), "app");
assert.equal(resolveSettingsCat("account"), "account");
assert.equal(resolveSettingsCat("devices"), "devices");

const settings = fs.readFileSync(path.join(root, "src/components/views/SettingsView.tsx"), "utf8");
assert.doesNotMatch(settings, /id="sec-autonomy"/);
assert.doesNotMatch(settings, /id="sec-desktop"/);
assert.doesNotMatch(settings, /id="sec-agent"/);
assert.doesNotMatch(settings, /id="sec-selfmod"/);
assert.doesNotMatch(settings, /id="sec-learning"/);
assert.doesNotMatch(settings, /id="sec-project"/);
assert.doesNotMatch(settings, /AutonomySettingsPanel/);
assert.doesNotMatch(settings, /AgentPrefsPanel/);
assert.doesNotMatch(settings, /label: "Agent"/);
assert.doesNotMatch(settings, /label: "Memory"/);
assert.doesNotMatch(settings, /DesktopHostView/);
assert.match(settings, /id="sec-oauth"/);
assert.match(settings, /id="sec-appearance"/);
assert.match(settings, /id="sec-updates"/);

const queue = fs.readFileSync(path.join(root, "src/components/views/AgentQueueView.tsx"), "utf8");
assert.doesNotMatch(queue, /setAutonomy\(\{ level/);
assert.match(queue, /data-next-up/);

const slash = fs.readFileSync(path.join(root, "src/lib/slash-commands.ts"), "utf8");
assert.doesNotMatch(slash, /\/host off/);
assert.doesNotMatch(slash, /\/approve off/);
assert.doesNotMatch(slash, /\/tools off/);

console.log("smoke-locked-settings OK");
