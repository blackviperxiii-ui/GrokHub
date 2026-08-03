/**
 * Cross-platform desktop production build (Windows-safe env).
 * Also copies PGLite sibling assets required at runtime (BUG: ENOENT pglite.data).
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const { cleanStaleServerManifests } = require("../desktop/clean-output.cjs");

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


/**
 * Keep only assets referenced by the current HTML/SSR manifest.
 * Removes stale hashed ChatView-*.js / models-catalog-*.js piles that
 * confuse host greps and inflate install size.
 */

function cleanStaleClientAssets() {
  const assetDirs = [
    path.join(root, ".output", "public", "assets"),
    path.join(root, ".output", "public", "_build", "assets"),
  ];
  // Collect referenced basenames from any html/json/mjs under .output
  const referenced = new Set();
  function walkRefs(dir, depth = 0) {
    if (depth > 6 || !fs.existsSync(dir)) return;
    let names;
    try {
      names = fs.readdirSync(dir);
    } catch {
      return;
    }
    for (const name of names) {
      const full = path.join(dir, name);
      let st;
      try {
        st = fs.statSync(full);
      } catch {
        continue;
      }
      if (st.isDirectory()) {
        if (name === "node_modules" || name === ".git") continue;
        walkRefs(full, depth + 1);
        continue;
      }
      if (!/\.(html|mjs|js|json|css|map)$/i.test(name)) continue;
      // Skip scanning huge binary-ish assets as text sources of truth
      if (st.size > 8_000_000) continue;
      if (full.includes(`${path.sep}assets${path.sep}`) && /\.(js|css|map)$/i.test(name)) {
        // don't use asset files themselves as ref sources for other assets
        continue;
      }
      let text = "";
      try {
        text = fs.readFileSync(full, "utf8");
      } catch {
        continue;
      }
      const re = /(?:assets\/|\/assets\/)([A-Za-z0-9._@{}[\]+-]+\.(?:js|css|mjs|map|woff2?|png|svg|webp))/g;
      let m;
      while ((m = re.exec(text))) {
        referenced.add(m[1]);
        // also basename without query
        referenced.add(path.basename(m[1]));
      }
      // bare hashed filenames in import maps
      const re2 = /["']([A-Za-z0-9._-]+-[A-Za-z0-9_-]{6,}\.(?:js|css))["']/g;
      while ((m = re2.exec(text))) {
        referenced.add(m[1]);
      }
    }
  }
  walkRefs(path.join(root, ".output"));

  let removed = 0;
  let kept = 0;
  for (const dir of assetDirs) {
    if (!fs.existsSync(dir)) continue;
    for (const name of fs.readdirSync(dir)) {
      const full = path.join(dir, name);
      try {
        if (!fs.statSync(full).isFile()) continue;
      } catch {
        continue;
      }
      // Always keep source maps for kept js if present; decide by basename
      if (referenced.has(name) || referenced.has(name.replace(/\.map$/, ""))) {
        kept += 1;
        continue;
      }
      // If nothing referenced (parse miss), keep everything rather than wipe
      if (referenced.size < 5) {
        kept += 1;
        continue;
      }
      try {
        fs.unlinkSync(full);
        removed += 1;
      } catch {
        /* ignore */
      }
    }
  }
  // Write stable pointer for debug tools
  const manifest = {
    generatedAt: new Date().toISOString(),
    referencedCount: referenced.size,
    kept,
    removed,
    note: "Active client assets are those referenced by current SSR/HTML entrypoints. Prefer grepping GROKHUB_BUILD.json version + this manifest over scanning all hashes.",
  };
  try {
    fs.writeFileSync(
      path.join(root, ".output", "ASSETS_MANIFEST.json"),
      JSON.stringify(manifest, null, 2) + "\n",
    );
  } catch {
    /* ignore */
  }
  console.log(
    `[desktop-build] asset hygiene: kept=${kept} removed=${removed} refs=${referenced.size}`,
  );
}


const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const npx = process.platform === "win32" ? "npx.cmd" : "npx";
run(npx, ["vite", "build"]);
run(npm, ["run", "db:migrate"]);
copyPgliteAssets();
cleanStaleClientAssets();
{
  const r = cleanStaleServerManifests(root);
  console.log(`[desktop-build] server manifests: kept=${r.kept} removed=${r.removed}`);
}

// Stamp so install/updater can verify UI matches APP_VERSION
try {
  const ver = fs.readFileSync(path.join(root, "APP_VERSION"), "utf8").trim();
  const stamp = {
    version: ver,
    builtAt: new Date().toISOString(),
    source: "desktop-build",
  };
  fs.writeFileSync(
    path.join(root, ".output", "GROKHUB_BUILD.json"),
    JSON.stringify(stamp, null, 2) + "\n",
  );
  console.log(`[desktop-build] GROKHUB_BUILD.json → v${ver}`);
} catch (e) {
  console.warn("[desktop-build] could not write GROKHUB_BUILD.json", e);
}

const serverEntry = path.join(root, ".output", "server", "index.mjs");
if (!fs.existsSync(serverEntry)) {
  console.error("[desktop-build] ERROR: missing .output/server/index.mjs");
  process.exit(1);
}

// Pointer for agents/debug: which hashed ChatView/models bundles are live
try {
  const pub = path.join(root, ".output", "public", "assets");
  const htmlCandidates = [
    path.join(root, ".output", "public", "index.html"),
    path.join(root, ".output", "server", "index.mjs"),
  ];
  let refText = "";
  for (const f of htmlCandidates) {
    if (fs.existsSync(f)) {
      try {
        refText += fs.readFileSync(f, "utf8").slice(0, 500_000);
      } catch {
        /* ignore */
      }
    }
  }
  // also scan SSR entry for ChatView / models-catalog hashes
  const ssrDir = path.join(root, ".output", "server", "_ssr");
  const active = { version: null, builtAt: new Date().toISOString(), assets: {} };
  try {
    active.version = fs.readFileSync(path.join(root, "APP_VERSION"), "utf8").trim();
  } catch {
    /* ignore */
  }
  if (fs.existsSync(ssrDir)) {
    for (const name of fs.readdirSync(ssrDir)) {
      if (/^ChatView-.*\.mjs$/.test(name)) active.assets.chatViewSsr = name;
      if (/^models-catalog-.*\.mjs$/.test(name)) active.assets.modelsCatalogSsr = name;
      if (/^SettingsView-.*\.mjs$/.test(name)) active.assets.settingsViewSsr = name;
    }
  }
  if (fs.existsSync(pub)) {
    for (const name of fs.readdirSync(pub)) {
      if (/^ChatView-.*\.js$/.test(name)) active.assets.chatView = name;
      if (/^models-catalog-.*\.js$/.test(name)) active.assets.modelsCatalog = name;
      if (/^SettingsView-.*\.js$/.test(name)) active.assets.settingsView = name;
    }
  }
  fs.writeFileSync(
    path.join(root, ".output", "ACTIVE_UI.json"),
    JSON.stringify(active, null, 2) + "\n",
  );
  console.log("[desktop-build] ACTIVE_UI.json", active.assets);
} catch (e) {
  console.warn("[desktop-build] ACTIVE_UI.json skip", e);
}

console.log("desktop:build OK");
