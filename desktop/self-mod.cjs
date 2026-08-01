/**
 * Controlled self-modification for GrokHub install tree + factory restore hooks.
 * User data / memory lives outside the install root and is never written here
 * unless an explicit factory wipe is requested.
 */
const fs = require("node:fs/promises");
const fssync = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const execFileAsync = promisify(execFile);

const ALLOWED_TOP = new Set([
  "src",
  "desktop",
  "scripts",
  "packaging",
  "public",
  "vendor",
  ".grok",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  "vite.config.ts",
  "vite.config.js",
  "startup.sh",
  "README.md",
  "APP_VERSION",
  "VERSION",
]);

const DENY_SEGMENTS = new Set([
  "node_modules",
  ".git",
  ".output",
  "userData",
  "secrets",
  "grokhub-secrets.json",
  "grokhub-memory.json",
]);

async function isAppRoot(root) {
  try {
    await fs.stat(path.join(root, ".output", "server", "index.mjs"));
    return true;
  } catch {}
  try {
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
    return pkg.name === "grokhub" || pkg.name === "GrokHub";
  } catch {
    return false;
  }
}

async function resolveAppRoot() {
  const candidates = [
    process.env.GROKHUB_HOME,
    process.cwd(),
    "/usr/lib/grokhub",
    path.join(os.homedir(), ".local/share/grokhub"),
  ].filter(Boolean);
  for (const c of candidates) {
    if (await isAppRoot(c)) return path.resolve(c);
  }
  return path.resolve(process.env.GROKHUB_HOME || process.cwd());
}

function userDataDir() {
  // Prefer Electron userData when available
  try {
    const { app } = require("electron");
    if (app?.getPath) return app.getPath("userData");
  } catch {
    /* not in electron */
  }
  return path.join(os.homedir(), ".config", "GrokHub");
}

function selfModDir() {
  return path.join(userDataDir(), "self-mod");
}

function snapshotsDir() {
  return path.join(selfModDir(), "snapshots");
}

function journalPath() {
  return path.join(selfModDir(), "journal.jsonl");
}

function normalizeRel(rel) {
  const clean = String(rel || "")
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/^\.\//, "");
  if (!clean || clean.includes("..") || path.isAbsolute(clean)) {
    throw new Error("Invalid path");
  }
  const parts = clean.split("/").filter(Boolean);
  for (const p of parts) {
    if (DENY_SEGMENTS.has(p)) throw new Error(`Path not allowed: ${p}`);
  }
  const top = parts[0];
  // allow top-level files in ALLOWED_TOP or directories in ALLOWED_TOP
  if (!ALLOWED_TOP.has(top) && !ALLOWED_TOP.has(clean)) {
    throw new Error(`Top-level path not allowed for self-mod: ${top}`);
  }
  return parts.join(path.sep);
}

async function appendJournal(entry) {
  await fs.mkdir(selfModDir(), { recursive: true });
  const line = JSON.stringify({ ts: Date.now(), ...entry }) + "\n";
  await fs.appendFile(journalPath(), line, "utf8");
}

async function info() {
  const root = await resolveAppRoot();
  let snaps = [];
  try {
    const names = await fs.readdir(snapshotsDir());
    const stats = await Promise.all(
      names.map(async (name) => {
        try {
          const p = path.join(snapshotsDir(), name, "manifest.json");
          const m = JSON.parse(await fs.readFile(p, "utf8"));
          return m;
        } catch {
          return { id: name };
        }
      }),
    );
    snaps = stats.sort((a, b) => (b.createdAt || 0) - (a.createdAt || 0)).slice(0, 20);
  } catch {
    snaps = [];
  }
  return {
    ok: true,
    root,
    selfModDir: selfModDir(),
    snapshots: snaps,
    journal: journalPath(),
  };
}

async function readFileRel(rel) {
  const root = await resolveAppRoot();
  const safe = normalizeRel(rel);
  const full = path.join(root, safe);
  const text = await fs.readFile(full, "utf8");
  return {
    ok: true,
    path: safe.replace(/\\/g, "/"),
    absolute: full,
    bytes: Buffer.byteLength(text, "utf8"),
    content: text.length > 400_000 ? text.slice(0, 400_000) + "\n/* truncated */" : text,
  };
}

async function listDirRel(rel = "src") {
  const root = await resolveAppRoot();
  const safe = rel ? normalizeRel(rel) : "";
  const full = safe ? path.join(root, safe) : root;
  const entries = await fs.readdir(full, { withFileTypes: true });
  return {
    ok: true,
    path: safe.replace(/\\/g, "/") || ".",
    entries: entries
      .filter((e) => !DENY_SEGMENTS.has(e.name))
      .map((e) => ({
        name: e.name,
        type: e.isDirectory() ? "dir" : "file",
      }))
      .slice(0, 200),
  };
}

/**
 * Write a file under the install root. Optionally snapshot previous content first.
 */
async function writeFileRel(rel, content, opts = {}) {
  const root = await resolveAppRoot();
  const safe = normalizeRel(rel);
  const full = path.join(root, safe);
  await fs.mkdir(path.dirname(full), { recursive: true });

  let previous = null;
  try {
    previous = await fs.readFile(full, "utf8");
  } catch {
    previous = null;
  }

  if (opts.snapshot !== false) {
    const id = `auto-${Date.now()}`;
    const snapRoot = path.join(snapshotsDir(), id);
    await fs.mkdir(path.join(snapRoot, "files", path.dirname(safe)), { recursive: true });
    if (previous != null) {
      await fs.writeFile(path.join(snapRoot, "files", safe), previous, "utf8");
    }
    await fs.writeFile(
      path.join(snapRoot, "manifest.json"),
      JSON.stringify(
        {
          id,
          createdAt: Date.now(),
          kind: "auto-before-write",
          files: previous != null ? [safe.replace(/\\/g, "/")] : [],
          note: opts.note || `Before write ${safe}`,
        },
        null,
        2,
      ),
      "utf8",
    );
  }

  const text = String(content ?? "");
  const tmp = full + ".tmp-" + process.pid;
  await fs.writeFile(tmp, text, "utf8");
  await fs.rename(tmp, full);

  await appendJournal({
    action: "write",
    path: safe.replace(/\\/g, "/"),
    bytes: Buffer.byteLength(text, "utf8"),
    note: opts.note || "",
  });

  return {
    ok: true,
    path: safe.replace(/\\/g, "/"),
    absolute: full,
    bytes: Buffer.byteLength(text, "utf8"),
    hadPrevious: previous != null,
  };
}

/**
 * Snapshot key trees for manual restore points.
 */
async function createSnapshot(note = "manual") {
  const root = await resolveAppRoot();
  const id = `snap-${new Date().toISOString().replace(/[:.]/g, "-")}`;
  const snapRoot = path.join(snapshotsDir(), id);
  await fs.mkdir(path.join(snapRoot, "files"), { recursive: true });

  const trees = ["src", "desktop", "scripts"];
  const files = [];
  async function walk(rel, depth) {
    if (depth > 8) return;
    const full = path.join(root, rel);
    let entries;
    try {
      entries = await fs.readdir(full, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      if (DENY_SEGMENTS.has(e.name) || e.name.startsWith(".")) continue;
      const child = path.join(rel, e.name);
      if (e.isDirectory()) {
        await walk(child, depth + 1);
      } else if (e.isFile()) {
        const size = (await fs.stat(path.join(root, child))).size;
        if (size > 1_500_000) continue; // skip huge
        const dest = path.join(snapRoot, "files", child);
        await fs.mkdir(path.dirname(dest), { recursive: true });
        await fs.copyFile(path.join(root, child), dest);
        files.push(child.replace(/\\/g, "/"));
      }
    }
  }
  for (const t of trees) {
    try {
      await fs.stat(path.join(root, t));
      await walk(t, 0);
    } catch {
      /* skip */
    }
  }
  // top-level configs
  for (const f of ["package.json", "vite.config.ts", "startup.sh", "APP_VERSION"]) {
    try {
      await fs.copyFile(path.join(root, f), path.join(snapRoot, "files", f));
      files.push(f);
    } catch {
      /* skip */
    }
  }

  const manifest = {
    id,
    createdAt: Date.now(),
    kind: "full",
    note: String(note || "manual"),
    root,
    files: files.slice(0, 5000),
    fileCount: files.length,
  };
  await fs.writeFile(path.join(snapRoot, "manifest.json"), JSON.stringify(manifest, null, 2));
  await appendJournal({ action: "snapshot", id, fileCount: files.length, note: manifest.note });
  return { ok: true, ...manifest };
}

async function restoreSnapshot(id) {
  if (!id) return { ok: false, error: "snapshot id required" };
  const root = await resolveAppRoot();
  const snapRoot = path.join(snapshotsDir(), String(id));
  let manifest;
  try {
    manifest = JSON.parse(await fs.readFile(path.join(snapRoot, "manifest.json"), "utf8"));
  } catch {
    return { ok: false, error: "snapshot not found" };
  }
  const filesRoot = path.join(snapRoot, "files");
  async function walk(rel) {
    const full = path.join(filesRoot, rel);
    const entries = await fs.readdir(full, { withFileTypes: true });
    for (const e of entries) {
      const child = rel ? path.join(rel, e.name) : e.name;
      if (e.isDirectory()) await walk(child);
      else {
        const dest = path.join(root, child);
        await fs.mkdir(path.dirname(dest), { recursive: true });
        await fs.copyFile(path.join(filesRoot, child), dest);
      }
    }
  }
  await walk("");
  await appendJournal({ action: "restore-snapshot", id, fileCount: manifest.fileCount });
  return { ok: true, id, restored: true, note: "Restart the app to load restored code" };
}

async function listJournal(limit = 40) {
  try {
    const raw = await fs.readFile(journalPath(), "utf8");
    const lines = raw.trim().split("\n").filter(Boolean);
    const rows = lines
      .slice(-Math.max(1, Math.min(200, limit)))
      .map((l) => {
        try {
          return JSON.parse(l);
        } catch {
          return { raw: l };
        }
      })
      .reverse();
    return { ok: true, entries: rows };
  } catch {
    return { ok: true, entries: [] };
  }
}

/**
 * Apply a unified-diff style patch is complex; support simple search-replace.
 */
async function patchFileRel(rel, find, replace, opts = {}) {
  const cur = await readFileRel(rel);
  if (!cur.ok) return cur;
  if (!find) return { ok: false, error: "find string required" };
  if (!cur.content.includes(find)) {
    return { ok: false, error: "find string not present in file" };
  }
  const next = opts.replaceAll
    ? cur.content.split(find).join(replace)
    : cur.content.replace(find, replace);
  return writeFileRel(rel, next, { note: opts.note || "patch", snapshot: true });
}

module.exports = {
  resolveAppRoot,
  info,
  readFileRel,
  listDirRel,
  writeFileRel,
  patchFileRel,
  createSnapshot,
  restoreSnapshot,
  listJournal,
  userDataDir,
  selfModDir,
  isAppRoot,
};
