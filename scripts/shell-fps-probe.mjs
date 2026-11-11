// Tauri's beforeBundleCommand: builds the two helpers the shell's Linux
// bundles ship next to the app (tauri.conf.json bundle.linux.*.files):
//   /usr/lib/loadout/fps-probe   the small frame-rate probe. It needs only
//     libc, so it still starts once Home's Frame Rate setup gives it
//     CAP_PERFMON.
//   /usr/lib/loadout/grokhub-hub the headless tools (--oauth, --doctor,
//     --mcp-self, --mcp-desktop, --mcp-cua, --askpass). The app starts the
//     sudo askpass server and the Cua gate with it, and finds it at
//     ../lib/loadout/ from the app binary (src-tauri/src/episode.rs).
// Other platforms need nothing here.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform === "linux") {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  for (const [pkg, dir] of [
    ["grokhub-fps-probe", "fps-probe"],
    ["grokhub-hub", "hub"],
  ]) {
    execFileSync(
      "cargo",
      [
        "build",
        "--release",
        "--locked",
        "-p",
        pkg,
        "--target-dir",
        path.join(root, "target", dir),
      ],
      { cwd: root, stdio: "inherit" },
    );
  }
}
