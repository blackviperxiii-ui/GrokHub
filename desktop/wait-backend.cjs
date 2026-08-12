/**
 * Start the GrokHub UI server (if needed) and block until it is fully healthy.
 * Launchers call this before Electron so the window never hits a cold backend.
 *
 *   node desktop/wait-backend.cjs
 *   node desktop/wait-backend.cjs --then-electron [electronBin] [...args]
 */
const path = require("node:path");
const { spawn } = require("node:child_process");
const ui = require("./ui-server.cjs");

async function main() {
  const argv = process.argv.slice(2);
  const thenIdx = argv.indexOf("--then-electron");
  const thenElectron = thenIdx !== -1;
  const electronArgs = thenElectron ? argv.slice(thenIdx + 1) : [];
  const waitOnly = argv.filter((a) => a !== "--then-electron" && !electronArgs.includes(a));
  void waitOnly;

  const desktopDir = __dirname;
  const resolved = await ui.resolveStartUrl(desktopDir);
  if (resolved.url) {
    process.env.GROKHUB_URL = resolved.url.replace(/\/$/, "");
  }
  if (!resolved.ok) {
    const msg = resolved.error || "UI not reachable";
    console.error("[GrokHub] backend failed to become ready:", msg);
    if (!thenElectron) process.exit(1);
  } else {
    console.log("[GrokHub] backend ready", process.env.GROKHUB_URL);
  }

  if (!thenElectron) {
    process.exit(resolved.ok ? 0 : 1);
  }

  const electronBin = electronArgs[0] || process.env.GROKHUB_ELECTRON || "electron";
  const extra = electronArgs.length > 1 ? electronArgs.slice(1) : [];
  const mainJs = path.join(desktopDir, "main.mjs");
  const child = spawn(electronBin, [mainJs, ...extra], {
    stdio: "inherit",
    env: process.env,
    shell: process.platform === "win32" && !/[\\/]/.test(electronBin),
  });
  child.on("exit", (code) => process.exit(code ?? 0));
}

main().catch((e) => {
  console.error("[GrokHub] wait-backend", e && e.message ? e.message : e);
  process.exit(1);
});
