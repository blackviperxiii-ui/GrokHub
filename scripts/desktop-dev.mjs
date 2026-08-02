/**
 * Cross-platform desktop:dev — launch Electron against local Vite UI.
 */
import { spawn } from "node:child_process";
import process from "node:process";

process.env.GROKHUB_URL = process.env.GROKHUB_URL || "http://127.0.0.1:8080";
process.env.GROKHUB_TRAY = process.env.GROKHUB_TRAY || "1";

const electronBin =
  process.platform === "win32"
    ? "electron.cmd"
    : process.platform === "darwin"
      ? "electron"
      : "electron";

const child = spawn(electronBin, ["desktop/main.mjs"], {
  stdio: "inherit",
  env: process.env,
  shell: process.platform === "win32",
});
child.on("exit", (code) => process.exit(code ?? 0));
