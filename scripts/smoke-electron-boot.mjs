#!/usr/bin/env node
/**
 * Spawn Electron against an already-running UI (GROKHUB_URL) and succeed only
 * when the main process logs boot-complete. Catches the tray-only crashes:
 * undefined startupLog, double loadURL stack overflow, Chromium SIGTRAP.
 */
import { spawn } from "node:child_process";
import { createRequire } from "node:module";
import process from "node:process";

const require = createRequire(import.meta.url);
const electronBin = require("electron");

const url = String(process.env.GROKHUB_URL || "").replace(/\/$/, "");
if (!url) {
  console.error("[smoke-electron-boot] GROKHUB_URL is required (e.g. http://127.0.0.1:18765)");
  process.exit(2);
}

if (process.platform === "linux" && !process.env.DISPLAY && !process.env.WAYLAND_DISPLAY) {
  console.error("[smoke-electron-boot] DISPLAY is unset — run under xvfb-run on Linux");
  process.exit(2);
}

const timeoutMs = Number(process.env.GROKHUB_BOOT_TIMEOUT_MS || 45_000);
// GHA chrome-sandbox is often root:4755; JS appendSwitch("no-sandbox") is too late.
const sandboxOff = process.platform === "linux" && process.env.GROKHUB_SANDBOX !== "1";
const electronArgs = sandboxOff
  ? ["--no-sandbox", "--no-zygote", "--disable-dev-shm-usage", "desktop/main.mjs"]
  : ["desktop/main.mjs"];
const child = spawn(electronBin, electronArgs, {
  env: {
    ...process.env,
    GROKHUB_URL: url,
    GROKHUB_DISABLE_GPU: process.env.GROKHUB_DISABLE_GPU || "1",
    GROKHUB_WAYLAND: process.env.GROKHUB_WAYLAND || "0",
    ELECTRON_ENABLE_LOGGING: "1",
    ...(sandboxOff ? { ELECTRON_DISABLE_SANDBOX: "1" } : {}),
  },
  stdio: ["ignore", "pipe", "pipe"],
});

let buf = "";
let finished = false;
const failPatterns = [
  /startupLog is not defined/i,
  /Maximum call stack size exceeded/i,
  /FATAL:.*zygote/i,
  /Check failed: .*(Sandbox|zygote)/i,
];

function finish(code, msg) {
  if (finished) return;
  finished = true;
  clearTimeout(timer);
  if (msg) console.error("[smoke-electron-boot]", msg);
  try {
    child.kill("SIGTERM");
  } catch {
    /* already gone */
  }
  setTimeout(() => {
    try {
      child.kill("SIGKILL");
    } catch {
      /* ignore */
    }
    process.exit(code);
  }, 1200);
}

function onChunk(chunk) {
  const text = String(chunk);
  buf += text;
  process.stdout.write(text);
  if (/\[GrokHub\] boot-complete/.test(buf)) {
    console.log("[smoke-electron-boot] ok");
    finish(0);
    return;
  }
  for (const re of failPatterns) {
    if (re.test(buf)) {
      finish(1, `boot failed: ${re}`);
      return;
    }
  }
}

child.stdout.on("data", onChunk);
child.stderr.on("data", (chunk) => {
  process.stderr.write(chunk);
  onChunk(chunk);
});

child.on("error", (err) => finish(1, String(err?.message || err)));
child.on("exit", (code, signal) => {
  if (finished) return;
  if (signal === "SIGTRAP" || code === 133) {
    finish(1, "Chromium SIGTRAP before boot-complete");
    return;
  }
  finish(code || 1, `exited before boot-complete code=${code} signal=${signal}`);
});

const timer = setTimeout(
  () => finish(1, `timeout waiting for boot-complete (${timeoutMs}ms)`),
  timeoutMs,
);
