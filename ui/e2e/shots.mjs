#!/usr/bin/env node
// Screenshot harness for the Tauri UI (LO-UI-1).
//
// Starts Vite against ui/, injects a stubbed `__TAURI_INTERNALS__`, and writes
// PNGs for Chat, Home, Pulse, Code and every Settings sub-tab at 1600×900 and
// 1100×720 under Ops Dark and OLED Black.
//
//   npm run shots --prefix ui
//   SHOTS_OUT=ui/e2e/shots npm run shots --prefix ui
//
// It also sweeps every page in the views bar (and every Settings section) for
// gray or white outlines (e2e/outlines.mjs), writes them to outlines.json and
// exits nonzero when any remain.
//
// Needs Playwright's Chromium (`npx playwright install chromium`).

import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { initScript } from "./mock-tauri.mjs";
import { sweep } from "./outlines.mjs";

const { chromium } = createRequire(import.meta.url)("playwright");

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));
const OUT = resolve(process.env.SHOTS_OUT || join(ROOT, "e2e/shots"));
const PORT = Number(process.env.SHOTS_PORT || 5179);
const ORIGIN = `http://127.0.0.1:${PORT}`;
const VITE_BIN = join(ROOT, "node_modules", "vite", "bin", "vite.js");

const VIEWPORTS = [
  { name: "wide", width: 1600, height: 900 },
  { name: "mid", width: 1100, height: 720 },
  // The window's default size (src-tauri/tauri.conf.json).
  { name: "narrow", width: 900, height: 700 },
];

const THEMES = [
  { id: "ops-dark", label: "ops-dark" },
  { id: "oled-black", label: "oled" },
];

/** Main tabs the color-share check must cover. */
const VIEWS = ["chat", "home", "pulse", "code", "settings"];

/**
 * Settings sections with their top-row category (CATEGORY_LABELS).
 * Order matches sections() with autoSetup off.
 */
const SETTINGS_SECTIONS = [
  { section: "General", category: "Your PC" },
  { section: "Account", category: "Account" },
  { section: "Defaults", category: "AI & chat" },
  { section: "Behavior", category: "Your PC" },
  { section: "Appearance", category: "Look & feel" },
  { section: "Personality", category: "AI & chat" },
  { section: "Playbooks", category: "AI & chat" },
  { section: "Permissions", category: "Privacy & safety" },
  { section: "Privacy & data", category: "Privacy & safety" },
  { section: "Background", category: "Your PC" },
  { section: "Code", category: "Your PC" },
  { section: "Pulse", category: "AI & chat" },
  { section: "Local AI", category: "AI & chat" },
  { section: "AI models", category: "AI & chat" },
  { section: "Web access", category: "Privacy & safety" },
  { section: "Labs", category: "Help & updates" },
  { section: "About", category: "Help & updates" },
];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** True once Vite answers on ORIGIN (stdout alone is buffered / flaky in CI). */
async function viteAnswers() {
  try {
    const res = await fetch(ORIGIN, { signal: AbortSignal.timeout(1500) });
    return res.ok || res.status === 404;
  } catch {
    return false;
  }
}

function startVite() {
  if (!existsSync(VITE_BIN)) {
    throw new Error(`vite binary missing at ${VITE_BIN}; run npm ci --prefix ui first`);
  }
  const log = [];
  const child = spawn(
    process.execPath,
    [VITE_BIN, "--host", "127.0.0.1", "--port", String(PORT), "--strictPort"],
    {
      cwd: ROOT,
      stdio: ["ignore", "pipe", "pipe"],
      env: { ...process.env, BROWSER: "none" },
    },
  );
  const onData = (buf) => {
    const text = String(buf);
    log.push(text);
    if (log.join("").length > 8000) log.splice(0, log.length - 20);
  };
  child.stdout.on("data", onData);
  child.stderr.on("data", onData);
  return {
    child,
    async wait(ms = 60_000) {
      const start = Date.now();
      while (Date.now() - start < ms) {
        if (child.exitCode !== null) {
          throw new Error(`vite exited ${child.exitCode}\n${log.join("")}`);
        }
        if (await viteAnswers()) return;
        await sleep(250);
      }
      throw new Error(`vite did not become ready at ${ORIGIN}\n${log.join("")}`);
    },
    stop() {
      child.kill("SIGTERM");
    },
  };
}

async function openTab(page, label) {
  const tab = page.getByRole("navigation", { name: "Views" }).getByRole("button", {
    name: label,
    exact: true,
  });
  await tab.click({ timeout: 15_000 });
  await sleep(400);
}

/**
 * Opens a Settings section via the category row + side nav (same path as
 * Settings.test.tsx). More reliable than the command palette under stubs.
 */
async function openSettingsSection(page, { section, category }) {
  const cats = page.getByRole("navigation", { name: "Settings categories" });
  await cats.getByRole("button", { name: category, exact: true }).click({ timeout: 15_000 });
  await sleep(200);
  const sections = page.getByRole("navigation", { name: "Settings sections" });
  await sections.getByRole("button", { name: section, exact: true }).click({ timeout: 15_000 });
  await sleep(400);
  await page
    .getByRole("heading", { name: section, exact: true })
    .waitFor({ timeout: 10_000 })
    .catch(() => {});
}

async function shoot(page, path) {
  mkdirSync(dirname(path), { recursive: true });
  // Jump entrance and fill animations to their end so a shot shows the final
  // XP fill and streak cells, not a mid-tween frame.
  await page.evaluate(() => {
    for (const a of document.getAnimations()) a.finish();
  });
  await page.screenshot({ path, fullPage: false });
}

async function run() {
  mkdirSync(OUT, { recursive: true });
  const vite = startVite();
  const errors = [];
  try {
    await vite.wait();
    const browser = await chromium.launch({ headless: true });
    const manifest = [];
    const outlines = [];
    // Records the gray outlines on the screen that is open now, and whether
    // the page scrolls sideways (a scrollbar over the composer).
    const check = async (page, where) => {
      for (const hit of await page.evaluate(sweep)) outlines.push({ where, ...hit });
      const [wide, fits] = await page.evaluate(() => [document.documentElement.scrollWidth, document.documentElement.clientWidth]);
      if (wide > fits) errors.push(`${where}: the page scrolls sideways, ${wide}px wide in a ${fits}px window`);
    };
    try {
      for (const theme of THEMES) {
        for (const vp of VIEWPORTS) {
          const context = await browser.newContext({
            viewport: { width: vp.width, height: vp.height },
            deviceScaleFactor: 1,
          });
          const page = await context.newPage();
          let at = "start";
          page.on("pageerror", (err) => errors.push(`${theme.label}/${vp.name}/${at}: ${err.message}`));
          await page.addInitScript(initScript(theme.id));
          await page.goto(ORIGIN, { waitUntil: "networkidle", timeout: 60_000 });
          await sleep(800);
          // Dismiss anything that still blocked paint.
          await page.keyboard.press("Escape").catch(() => {});

          for (const view of VIEWS) {
            const label = view === "settings" ? "Settings" : view[0].toUpperCase() + view.slice(1);
            at = label;
            await openTab(page, label);
            if (view === "settings") {
              for (const row of SETTINGS_SECTIONS) {
                at = `Settings/${row.section}`;
                await openSettingsSection(page, row);
                const slug = `settings-${row.section.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
                const file = `${theme.label}_${vp.name}_${slug}.png`;
                const path = join(OUT, file);
                await shoot(page, path);
                await check(page, file);
                manifest.push({
                  file,
                  theme: theme.id,
                  viewport: vp.name,
                  view: "settings",
                  section: row.section,
                });
              }
            } else {
              const file = `${theme.label}_${vp.name}_${view}.png`;
              const path = join(OUT, file);
              await shoot(page, path);
              await check(page, file);
              manifest.push({ file, theme: theme.id, viewport: vp.name, view });
            }
          }
          // The rest of the rail, then every page More lists: swept, not shot.
          const shot = new Set(VIEWS.map((v) => (v === "settings" ? "Settings" : v[0].toUpperCase() + v.slice(1))));
          const slug = (label) => `${theme.label}_${vp.name}_${label.toLowerCase().replace(/[^a-z0-9]+/g, "-")}`;
          const rail = page.getByRole("navigation", { name: "Views" });
          const labels = await rail
            .getByRole("button")
            .evaluateAll((buttons) => buttons.map((b) => b.getAttribute("title") || b.textContent.trim()));
          for (const label of labels.filter((l) => !shot.has(l) && l !== "More")) {
            at = label;
            await openTab(page, label);
            await check(page, slug(label));
          }
          at = "More";
          await rail.getByRole("button", { name: "More", exact: true }).click({ timeout: 15_000 });
          const more = page.getByRole("dialog", { name: "More pages" });
          const pages = await more.getByRole("option").allTextContents();
          await check(page, slug("more"));
          await page.keyboard.press("Escape");
          for (const label of pages) {
            at = label;
            await rail.getByRole("button", { name: "More", exact: true }).click({ timeout: 15_000 });
            await more.getByRole("option", { name: label, exact: true }).click({ timeout: 15_000 });
            await sleep(400);
            await check(page, slug(label));
          }
          await context.close();
        }
      }
    } finally {
      await browser.close();
    }

    writeFileSync(join(OUT, "manifest.json"), JSON.stringify({ out: OUT, shots: manifest }, null, 2));
    console.log(`wrote ${manifest.length} shots to ${OUT}`);
    writeFileSync(join(OUT, "outlines.json"), JSON.stringify(outlines, null, 2));
    if (outlines.length) {
      console.error(`${outlines.length} gray outlines (outlines.json):`);
      for (const o of outlines.slice(0, 40)) console.error(`  ${o.where}: ${o.el} ${o.color} "${o.text}"`);
      process.exitCode = 1;
    } else {
      console.log("0 gray outlines");
    }
    if (errors.length) {
      console.error("page errors:");
      for (const e of errors) console.error(`  ${e}`);
      process.exitCode = 1;
    }
  } finally {
    vite.stop();
  }
}

run()
  .catch((err) => {
    console.error(err);
    process.exitCode = 1;
  })
  .finally(() => {
    // Vite's child can keep the event loop open after SIGTERM; force exit.
    setTimeout(() => process.exit(process.exitCode ?? 0), 500);
  });
