/**
 * Build GrokHub for Windows (NSIS installer + portable).
 * Run from repo root on Windows or Linux (electron-builder cross-compiles).
 *
 *   npm run desktop:win              # full NSIS + portable
 *   npm run desktop:win:dir          # unpacked dir only (faster)
 *   npm run desktop:win:portable     # portable .exe only
 */
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(__dirname, "..");
process.chdir(root);

const args = new Set(process.argv.slice(2));
const dirOnly = args.has("--dir");
const portableOnly = args.has("--portable");
const skipUi = args.has("--skip-ui");

function run(cmd, cmdArgs, opts = {}) {
  console.log(`\n> ${cmd} ${cmdArgs.join(" ")}\n`);
  const r = spawnSync(cmd, cmdArgs, {
    stdio: "inherit",
    env: { ...process.env, ...opts.env },
    shell: process.platform === "win32",
  });
  if (r.status) process.exit(r.status ?? 1);
}

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
const npx = process.platform === "win32" ? "npx.cmd" : "npx";

// Stamp version
const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
fs.writeFileSync("APP_VERSION", `${pkg.version}\n`);
if (!fs.existsSync("VERSION") || !fs.readFileSync("VERSION", "utf8").trim()) {
  fs.writeFileSync("VERSION", `${pkg.version}-windows\n`);
}

if (!skipUi) {
  process.env.GROKHUB_DESKTOP = "1";
  process.env.NODE_ENV = "production";
  run(npm, ["run", "desktop:build"], {
    env: { GROKHUB_DESKTOP: "1", NODE_ENV: "production" },
  });
}

const serverEntry = path.join(root, ".output", "server", "index.mjs");
if (!fs.existsSync(serverEntry)) {
  console.error("Missing UI build at .output/server/index.mjs — run desktop:build first");
  process.exit(1);
}

const icon = path.join(root, "packaging", "windows", "icon.ico");
if (!fs.existsSync(icon)) {
  console.error("Missing packaging/windows/icon.ico");
  process.exit(1);
}

const ebArgs = ["electron-builder", "--win"];
if (dirOnly) {
  ebArgs.push("--dir");
} else if (portableOnly) {
  ebArgs.push("--config.win.target=portable");
} else {
  // default: nsis + portable from package.json build.win.target
}

// Avoid signing prompts on CI / unsigned machines
process.env.CSC_IDENTITY_AUTO_DISCOVERY = process.env.CSC_IDENTITY_AUTO_DISCOVERY || "false";

run(npx, ebArgs, {
  env: {
    CSC_IDENTITY_AUTO_DISCOVERY: "false",
    GROKHUB_DESKTOP: "1",
  },
});

const out = path.join(root, "dist-desktop");
console.log("\nWindows build complete →", out);
try {
  for (const f of fs.readdirSync(out)) {
    const st = fs.statSync(path.join(out, f));
    if (st.isFile()) {
      console.log(`  ${f}  (${(st.size / 1024 / 1024).toFixed(1)} MB)`);
    }
  }
} catch {
  /* ignore */
}
console.log(`
Install on Windows:
  • NSIS:     dist-desktop/GrokHub-*-win-x64.exe  (setup wizard)
  • Portable: dist-desktop/GrokHub-*-portable.exe

Or source install:
  powershell -ExecutionPolicy Bypass -File scripts/install-windows.ps1
`);
