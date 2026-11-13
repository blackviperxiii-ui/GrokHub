#!/usr/bin/env node
// Color-share check for LO-UI shots (LO-UI-1).
//
// Counts red / amber / neutral-gray / black pixels in each PNG and prints a
// table. Exits nonzero when any screen fails:
//   - red pixels ≥ amber pixels
//   - amber ≤ 10% of non-black pixels
//   - neutral-gray outline pixels ≥ 50% lower than a baseline (when given)
//
//   npm run colorshare --prefix ui
//   npm run colorshare --prefix ui -- --dir ui/e2e/shots --baseline ui/e2e/shots
//
// Needs Playwright's Chromium (same as shots).

import { createRequire } from "node:module";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const { chromium } = createRequire(import.meta.url)("playwright");

const ROOT = resolve(fileURLToPath(new URL("..", import.meta.url)));

function arg(flag, fallback) {
  const i = process.argv.indexOf(flag);
  return i >= 0 ? process.argv[i + 1] : fallback;
}

const DIR = resolve(arg("--dir", join(ROOT, "e2e/shots")));
const BASELINE = process.argv.includes("--baseline")
  ? resolve(arg("--baseline", DIR))
  : null;

/** Locked palette anchors (Nov 13, 2026) plus today's amber/gray. */
const RED = { r: 215, g: 38, b: 61 }; // #D7263D
const RED_HOVER = { r: 232, g: 56, b: 79 }; // #E8384F
const AMBER = { r: 255, g: 159, b: 26 }; // #FF9F1A
const AMBER_OLD = { r: 255, g: 176, b: 0 }; // #FFB000 (current --accent)
const GRAY = { r: 35, g: 36, b: 40 }; // #232428
const BLACK = { r: 11, g: 11, b: 13 }; // #0B0B0D

function dist(a, b) {
  const dr = a.r - b.r;
  const dg = a.g - b.g;
  const db = a.b - b.b;
  return Math.sqrt(dr * dr + dg * dg + db * db);
}

function classify(r, g, b) {
  const p = { r, g, b };
  // Near-black background (void / app bg).
  if (dist(p, BLACK) < 18 || (r < 28 && g < 28 && b < 32)) return "black";
  const dRed = Math.min(dist(p, RED), dist(p, RED_HOVER));
  const dAmber = Math.min(dist(p, AMBER), dist(p, AMBER_OLD));
  const dGray = dist(p, GRAY);
  // Red-tinted line (#3A161D) still counts as red family for the share check.
  if (r > g + 20 && r > b + 20 && dRed <= 70) return "red";
  if (dAmber <= 55 && r > 180 && g > 100 && b < 120) return "amber";
  // Neutral gray hairlines: low chroma, mid-dark.
  if (dGray <= 40 || (Math.abs(r - g) < 12 && Math.abs(g - b) < 12 && r > 20 && r < 80)) {
    return "gray";
  }
  return "other";
}

async function countPng(browser, file) {
  const buf = readFileSync(file);
  const b64 = buf.toString("base64");
  const page = await browser.newPage();
  try {
    await page.setContent(
      `<img id="i" src="data:image/png;base64,${b64}" />`,
      { waitUntil: "load" },
    );
    return await page.evaluate(() => {
      const img = document.getElementById("i");
      const canvas = document.createElement("canvas");
      canvas.width = img.naturalWidth;
      canvas.height = img.naturalHeight;
      const ctx = canvas.getContext("2d");
      ctx.drawImage(img, 0, 0);
      const { data } = ctx.getImageData(0, 0, canvas.width, canvas.height);
      // classify is injected below via addInitScript… so we inline a copy.
      const RED = { r: 215, g: 38, b: 61 };
      const RED_HOVER = { r: 232, g: 56, b: 79 };
      const AMBER = { r: 255, g: 159, b: 26 };
      const AMBER_OLD = { r: 255, g: 176, b: 0 };
      const GRAY = { r: 35, g: 36, b: 40 };
      const BLACK = { r: 11, g: 11, b: 13 };
      const dist = (a, b) => {
        const dr = a.r - b.r;
        const dg = a.g - b.g;
        const db = a.b - b.b;
        return Math.sqrt(dr * dr + dg * dg + db * db);
      };
      const classify = (r, g, b) => {
        const p = { r, g, b };
        if (dist(p, BLACK) < 18 || (r < 28 && g < 28 && b < 32)) return "black";
        const dRed = Math.min(dist(p, RED), dist(p, RED_HOVER));
        const dAmber = Math.min(dist(p, AMBER), dist(p, AMBER_OLD));
        const dGray = dist(p, GRAY);
        if (r > g + 20 && r > b + 20 && dRed <= 70) return "red";
        if (dAmber <= 55 && r > 180 && g > 100 && b < 120) return "amber";
        if (dGray <= 40 || (Math.abs(r - g) < 12 && Math.abs(g - b) < 12 && r > 20 && r < 80)) {
          return "gray";
        }
        return "other";
      };
      const counts = { red: 0, amber: 0, gray: 0, black: 0, other: 0, total: 0 };
      for (let i = 0; i < data.length; i += 4) {
        if (data[i + 3] < 16) continue;
        counts.total += 1;
        counts[classify(data[i], data[i + 1], data[i + 2])] += 1;
      }
      return counts;
    });
  } finally {
    await page.close();
  }
}

function pct(n, d) {
  return d ? ((100 * n) / d).toFixed(2) : "0.00";
}

function check(row, baselineGray) {
  const nonBlack = row.total - row.black;
  const fails = [];
  if (row.red < row.amber) fails.push(`red(${row.red}) < amber(${row.amber})`);
  const amberShare = nonBlack ? row.amber / nonBlack : 0;
  if (amberShare > 0.1) fails.push(`amber ${pct(row.amber, nonBlack)}% > 10% of non-black`);
  if (baselineGray != null) {
    if (row.gray > baselineGray * 0.5) {
      fails.push(`gray(${row.gray}) not ≤ 50% of baseline(${baselineGray})`);
    }
  }
  return fails;
}

async function run() {
  if (!existsSync(DIR)) {
    console.error(`no shots dir: ${DIR} (run npm run shots first)`);
    process.exit(2);
  }
  const files = readdirSync(DIR)
    .filter((f) => f.endsWith(".png"))
    .sort();
  if (!files.length) {
    console.error(`no PNGs in ${DIR}`);
    process.exit(2);
  }

  const browser = await chromium.launch({ headless: true });
  const rows = [];
  try {
    for (const file of files) {
      const counts = await countPng(browser, join(DIR, file));
      let baselineGray = null;
      if (BASELINE) {
        const baseFile = join(BASELINE, file);
        if (existsSync(baseFile) && resolve(BASELINE) !== resolve(DIR)) {
          const base = await countPng(browser, baseFile);
          baselineGray = base.gray;
        }
      }
      const fails = check(counts, baselineGray);
      rows.push({ file, ...counts, baselineGray, fails });
    }
  } finally {
    await browser.close();
  }

  // Table
  console.log(
    [
      "file".padEnd(48),
      "red".padStart(8),
      "amber".padStart(8),
      "gray".padStart(8),
      "amber%".padStart(8),
      "ok",
    ].join(" "),
  );
  for (const row of rows) {
    const nonBlack = row.total - row.black;
    console.log(
      [
        row.file.slice(0, 48).padEnd(48),
        String(row.red).padStart(8),
        String(row.amber).padStart(8),
        String(row.gray).padStart(8),
        pct(row.amber, nonBlack).padStart(8),
        row.fails.length ? "FAIL" : "ok",
      ].join(" "),
    );
    for (const f of row.fails) console.log(`  · ${f}`);
  }

  writeFileSync(join(DIR, "colorshare.json"), JSON.stringify({ dir: DIR, baseline: BASELINE, rows }, null, 2));

  const failed = rows.filter((r) => r.fails.length);
  if (failed.length) {
    console.error(`\n${failed.length}/${rows.length} screens failed color-share thresholds`);
    process.exit(1);
  }
  console.log(`\n${rows.length} screens passed`);
}

run().catch((err) => {
  console.error(err);
  process.exit(1);
});
