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
//   node scripts/stage-hub-sidecar.mjs [--force] [--profile dev|release]
//
// release (the default) is the optimized binary installers ship. It names
// the host triple, so cargo writes to target/hub/<host>/release whatever
// CARGO_BUILD_TARGET or build.target in a cargo config says. Left to those,
// cargo writes to their triple's folder, and a copy from target/hub/release
// would stage a binary from an earlier run.
//
// dev writes target/debug, the same folder the workspace Windows job caches.
// A beta push can restore that cache and skip a cold optimized build. The
// smoke test only needs a binary that runs; bundles stay on the release profile.
import { execFileSync } from "node:child_process";
import { copyFileSync, chmodSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const force = process.argv.includes("--force");
if (process.platform === "linux" && !force) {
  console.log("stage-hub-sidecar: Linux bundles ship grokhub-hub as a package file; nothing to stage");
  process.exit(0);
}

const profileAt = process.argv.indexOf("--profile");
const profile = profileAt === -1 ? "release" : process.argv[profileAt + 1];
if (profile !== "dev" && profile !== "release") {
  throw new Error(`stage-hub-sidecar: --profile must be dev or release, got ${profile}`);
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

const dev = profile === "dev";
const targetDir = dev ? path.join(root, "target") : path.join(root, "target", "hub");
const cargoArgs = dev
  ? ["build", "--locked", "-p", "grokhub-hub", "--target-dir", targetDir]
  : [
      "build", "--release", "--locked", "-p", "grokhub-hub",
      "--target", host,
      "--target-dir", targetDir,
    ];
execFileSync("cargo", cargoArgs, { cwd: root, stdio: "inherit" });

const exe = host.includes("windows") ? ".exe" : "";
const from = dev
  ? path.join(targetDir, "debug", `grokhub-hub${exe}`)
  : path.join(targetDir, host, "release", `grokhub-hub${exe}`);
const to = path.join(root, "src-tauri", `grokhub-hub-${host}${exe}`);
copyFileSync(from, to);
if (!exe) {
  chmodSync(to, 0o755);
}
console.log(`stage-hub-sidecar: profile ${profile}: ${path.relative(root, from)} -> ${path.relative(root, to)}`);
