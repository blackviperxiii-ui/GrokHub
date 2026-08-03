/**
 * Prune stale build artifacts under .output (shared by desktop-build + update + boot).
 */
const fs = require("node:fs");
const path = require("node:path");

/**
 * Keep only TanStack server manifests that are actually referenced (or the newest one).
 * @param {string} installRoot GROKHUB_HOME
 * @returns {{ kept: number, removed: number, keptNames: string[] }}
 */
function cleanStaleServerManifests(installRoot) {
  const serverDir = path.join(installRoot, ".output", "server");
  const result = { kept: 0, removed: 0, keptNames: [] };
  if (!fs.existsSync(serverDir)) return result;

  const re = /^_tanstack-start-manifest.*\.mjs$/i;
  let files = [];
  try {
    files = fs
      .readdirSync(serverDir)
      .filter((n) => re.test(n))
      .map((n) => {
        const full = path.join(serverDir, n);
        let mtime = 0;
        try {
          mtime = fs.statSync(full).mtimeMs;
        } catch {
          /* ignore */
        }
        return { n, full, mtime };
      })
      .sort((a, b) => b.mtime - a.mtime);
  } catch {
    return result;
  }

  if (!files.length) return result;

  const keep = new Set();
  // Prefer names referenced from index.mjs / active SSR entry
  const scanTargets = ["index.mjs", "chunks", "_ssr"];
  let indexText = "";
  try {
    indexText = fs.readFileSync(path.join(serverDir, "index.mjs"), "utf8");
  } catch {
    /* ignore */
  }
  // Also scan a few small mjs under _ssr for imports
  let scanBlob = indexText;
  try {
    const ssr = path.join(serverDir, "_ssr");
    if (fs.existsSync(ssr)) {
      for (const n of fs.readdirSync(ssr).slice(0, 40)) {
        if (!/\.mjs$/.test(n)) continue;
        const full = path.join(ssr, n);
        try {
          const st = fs.statSync(full);
          if (st.size > 2_000_000) continue;
          scanBlob += "\n" + fs.readFileSync(full, "utf8");
        } catch {
          /* ignore */
        }
      }
    }
  } catch {
    /* ignore */
  }

  for (const f of files) {
    if (scanBlob.includes(f.n)) keep.add(f.n);
  }
  // Always keep the newest if nothing referenced (or as safety)
  if (!keep.size && files[0]) keep.add(files[0].n);
  // Cap: never keep more than 2 manifests
  if (keep.size > 2) {
    const ordered = files.filter((f) => keep.has(f.n));
    keep.clear();
    for (const f of ordered.slice(0, 2)) keep.add(f.n);
  }

  for (const f of files) {
    if (keep.has(f.n)) {
      result.kept += 1;
      result.keptNames.push(f.n);
      continue;
    }
    try {
      fs.unlinkSync(f.full);
      result.removed += 1;
    } catch {
      /* ignore */
    }
  }
  return result;
}

/**
 * One-shot hygiene for an install tree (safe to call on every boot).
 */
function cleanInstallOutput(installRoot) {
  if (!installRoot) return { ok: false, detail: "no root" };
  const server = path.join(installRoot, ".output", "server");
  if (!fs.existsSync(server)) return { ok: false, detail: "no .output/server" };
  const manifests = cleanStaleServerManifests(installRoot);
  return {
    ok: true,
    manifests,
    detail: `manifests kept=${manifests.kept} removed=${manifests.removed}`,
  };
}

module.exports = {
  cleanStaleServerManifests,
  cleanInstallOutput,
};
