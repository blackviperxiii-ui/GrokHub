#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";

const root = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..");
const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
const av = fs.readFileSync(path.join(root, "APP_VERSION"), "utf8").trim();
const vt = fs.readFileSync(path.join(root, "src/lib/version.ts"), "utf8");
const m = vt.match(/APP_VERSION\s*=\s*"([^"]+)"/);
const bridge = fs.readFileSync(path.join(root, "desktop/grok-bridge.cjs"), "utf8");
const bm = bridge.match(/const APP_VERSION = "([^"]+)"/);
const errors = [];
if (pkg.version !== av) errors.push(`package.json ${pkg.version} != APP_VERSION ${av}`);
if (!m || m[1] !== av) errors.push(`version.ts ${m?.[1]} != APP_VERSION ${av}`);
if (!bm || bm[1] !== av) errors.push(`grok-bridge ${bm?.[1]} != APP_VERSION ${av}`);
if (errors.length) {
  console.error("Version drift:\n" + errors.join("\n"));
  process.exit(1);
}
console.log(`versions OK · v${av}`);
