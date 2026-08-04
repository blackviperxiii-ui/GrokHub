import { spawnSync } from "node:child_process";
import { pathToFileURL } from "node:url";
import path from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const entry = path.join(root, "scripts/smoke-predict-chips-entry.ts");
const outfile = path.join(tmpdir(), `grokhub-smoke-predict-${process.pid}.mjs`);
const r = spawnSync(
  "npx",
  ["--yes", "esbuild", entry, "--bundle", "--platform=node", "--format=esm", `--outfile=${outfile}`],
  { cwd: root, encoding: "utf8" },
);
if (r.status !== 0) {
  console.error(r.stderr || r.stdout);
  process.exit(r.status || 1);
}
await import(pathToFileURL(outfile).href);
