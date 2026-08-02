/**
 * Cross-platform desktop production build (Windows-safe env).
 */
import { spawnSync } from "node:child_process";
import process from "node:process";

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

const npm = process.platform === "win32" ? "npm.cmd" : "npm";
// vite via npx for path reliability
const npx = process.platform === "win32" ? "npx.cmd" : "npx";
run(npx, ["vite", "build"]);
run(npm, ["run", "db:migrate"]);
console.log("desktop:build OK");
