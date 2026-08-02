/**
 * Cross-platform desktop production build (Windows-safe env).
 * Also copies PGLite sibling assets required at runtime (BUG: ENOENT pglite.data).
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
process.chdir(root);

process.env.GROKHUB_DESKTOP = "1";
process.env.NODE_ENV = process.env.NODE_ENV || "production";

function run(cmd, args) {
  const r = spawnSync(cmd, args, {
    stdio: "inherit",
    env: process.env,
    shell: process.platform === "win32",
  });
  if (r.status) process.exit(r.status ?? 1);
}

/**
 * Nitro packs electric-sql__pglite.mjs but not pglite.data / .wasm siblings.
 * Copy them next to the bundle under .output/server/_libs/.
 */
function copyPgliteAssets() {
  const srcDir = path.join(root, "node_modules", "@electric-sql", "pglite", "dist");
  const destDir = path.join(root, ".output", "server", "_libs");
  if (!fs.existsSync(srcDir)) {
    console.warn("[desktop-build] @electric-sql/pglite/dist missing — skip asset copy");
    return;
  }
  fs.mkdirSync(destDir, { recursive: true });
  const names = fs.readdirSync(srcDir);
  let n = 0;
  for (const name of names) {
    if (
      name === "pglite.data" ||
      name === "pglite.wasm" ||
      name === "initdb.wasm" ||
      name.endsWith(".data") ||
      (name.endsWith(".wasm") && !name.includes("map"))
    ) {
      const from = path.join(srcDir, name);
      const to = path.join(destDir, name);
      if (fs.statSync(from).isFile()) {
        fs.copyFileSync(from, to);
        n += 1;
      }
    }
  }
  // Also copy any extension tarballs PGLite may request relative to dist
  for (const name of names) {
    if (name.endsWith(".tar.gz")) {
      const from = path.join(srcDir, name);
      const to = path.join(destDir, name);
      if (!fs.existsSync(to) && fs.statSync(from).isFile()) {
        fs.copyFileSync(from, to);
        n += 1;
      }
    }
  }
  const probe = path.join(destDir, "pglite.data");
  if (!fs.existsSync(probe)) {
    console.error("[desktop-build] ERROR: pglite.data not found after copy — PGLite will fail at runtime");
    process.exit(1);
  }
  console.log(`[desktop-build] PGLite assets → .output/server/_libs (${n} files, pglite.data OK)`);
}

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const npx = process.platform === "win32" ? "npx.cmd" : "npx";
run(npx, ["vite", "build"]);
run(npm, ["run", "db:migrate"]);
copyPgliteAssets();

const serverEntry = path.join(root, ".output", "server", "index.mjs");
if (!fs.existsSync(serverEntry)) {
  console.error("[desktop-build] ERROR: missing .output/server/index.mjs");
  process.exit(1);
}
console.log("desktop:build OK");
