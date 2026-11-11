// Stages grokhub-hub as the shell's Tauri sidecar on Windows and macOS
// (bundle.externalBin in src-tauri/tauri.windows.conf.json and
// tauri.macos.conf.json). Tauri wants src-tauri/grokhub-hub-<target-triple>
// (.exe on Windows) before any cargo step in src-tauri: its build script
// checks that the path exists. The bundle puts it next to the app, where
// src-tauri/src/episode.rs finds it. Linux ships the hub through the deb and
// AppImage files instead (scripts/shell-fps-probe.mjs), so it needs nothing
// here unless --force asks for it (a local check of the sidecar path).
// It always builds for the host triple; cross and universal builds are not
// staged.
//
//   node scripts/stage-hub-sidecar.mjs [--force]
import { execFileSync } from "node:child_process";
import { copyFileSync, chmodSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const force = process.argv.includes("--force");
if (process.platform === "linux" && !force) {
  console.log("stage-hub-sidecar: Linux bundles ship grokhub-hub as a package file; nothing to stage");
  process.exit(0);
}

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" })
  .split("\n")
  .find((l) => l.startsWith("host: "))
  ?.slice("host: ".length)
  .trim();
if (!host) {
  throw new Error("stage-hub-sidecar: rustc -vV printed no host triple");
}

// The build names the host triple, so cargo writes to target/hub/<host>/release
// whatever CARGO_BUILD_TARGET or build.target in a cargo config says. Left to
// those, cargo writes to their triple's folder, and a copy from
// target/hub/release would stage a binary from an earlier run.
const targetDir = path.join(root, "target", "hub");
execFileSync(
  "cargo",
  [
    "build", "--release", "--locked", "-p", "grokhub-hub",
    "--target", host,
    "--target-dir", targetDir,
  ],
  { cwd: root, stdio: "inherit" },
);

const exe = host.includes("windows") ? ".exe" : "";
const from = path.join(targetDir, host, "release", `grokhub-hub${exe}`);
const to = path.join(root, "src-tauri", `grokhub-hub-${host}${exe}`);
copyFileSync(from, to);
if (!exe) {
  chmodSync(to, 0o755);
}
console.log(`stage-hub-sidecar: ${path.relative(root, from)} -> ${path.relative(root, to)}`);
