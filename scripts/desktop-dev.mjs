/**
 * Cross-platform desktop:dev — wait for the local Vite UI, then launch Electron.
 */
import { spawn } from "node:child_process";
import http from "node:http";
import process from "node:process";

process.env.GROKHUB_URL = process.env.GROKHUB_URL || "http://127.0.0.1:8080";
process.env.GROKHUB_TRAY = process.env.GROKHUB_TRAY || "1";

const url = process.env.GROKHUB_URL.replace(/\/$/, "") + "/";

function probe() {
  return new Promise((resolve) => {
    const req = http.get(url, { timeout: 800 }, (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => {
        const body = Buffer.concat(chunks).toString("utf8");
        const ok =
          (res.statusCode || 0) < 500 &&
          /GrokHub|<!DOCTYPE html>|tanstack|\/assets\//i.test(body);
        resolve(ok);
      });
    });
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      try {
        req.destroy();
      } catch {
        /* ignore */
      }
      resolve(false);
    });
  });
}

const deadline = Date.now() + 45_000;
let ready = await probe();
if (!ready) {
  console.log(`[desktop:dev] waiting for UI at ${url} …`);
}
while (!ready && Date.now() < deadline) {
  await new Promise((r) => setTimeout(r, 250));
  ready = await probe();
}
if (!ready) {
  console.error(`[desktop:dev] UI not healthy at ${url} — start \`npm run dev\` first`);
  process.exit(1);
}
console.log(`[desktop:dev] backend ready — launching Electron`);

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
