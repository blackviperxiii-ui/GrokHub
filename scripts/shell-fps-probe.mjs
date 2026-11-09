// Tauri's beforeBundleCommand: builds the small frame-rate probe that the
// shell's Linux bundles ship as /usr/lib/loadout/fps-probe (tauri.conf.json
// bundle.linux.*.files). It needs only libc, so it still starts once Home's
// Frame Rate setup gives it CAP_PERFMON. Other platforms need nothing here.
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

if (process.platform === "linux") {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
  execFileSync(
    "cargo",
    [
      "build",
      "--release",
      "--locked",
      "-p",
      "grokhub-fps-probe",
      "--target-dir",
      path.join(root, "target", "fps-probe"),
    ],
    { cwd: root, stdio: "inherit" },
  );
}
