/**
 * Persistent app memory under Electron userData.
 * Survives restarts and in-place updates (updates never touch userData).
 *
 * In-memory cache + debounced flush avoids full-file RMW on every keystroke.
 */
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

function electronApp() {
  try {
    return require("electron").app;
  } catch {
    return null;
  }
}

const STATE_FILE = "grokhub-memory.json";
const BACKUP_FILE = "grokhub-memory.backup.json";
const MAX_BYTES = 24 * 1024 * 1024; // 24MB safety cap
const FLUSH_MS = 180;

function dir() {
  const app = electronApp();
  try {
    if (app?.getPath) return app.getPath("userData");
  } catch {
    /* not ready */
  }
  const home = process.env.HOME || os.homedir() || "/tmp";
  return path.join(
    process.env.XDG_CONFIG_HOME || path.join(home, ".config"),
    "GrokHub",
  );
}

function statePath() {
  return path.join(dir(), STATE_FILE);
}

function backupPath() {
  return path.join(dir(), BACKUP_FILE);
}

function ensureDir() {
  fs.mkdirSync(dir(), { recursive: true });
}

/** @type {{ version: number, keys: Record<string, string>, updatedAt: number } | null} */
let cache = null;
let dirty = false;
/** @type {ReturnType<typeof setTimeout> | null} */
let flushTimer = null;

/**
 * Read raw string value for a named key (zustand store name).
 * File shape: { version: 1, keys: { [name]: string }, updatedAt }
 */
function readFileFromDisk() {
  try {
    const raw = fs.readFileSync(statePath(), "utf8");
    const data = JSON.parse(raw);
    if (!data || typeof data !== "object") return { version: 1, keys: {}, updatedAt: 0 };
    if (!data.keys || typeof data.keys !== "object") data.keys = {};
    return data;
  } catch {
    // try backup
    try {
      const raw = fs.readFileSync(backupPath(), "utf8");
      const data = JSON.parse(raw);
      if (data?.keys) return data;
    } catch {
      /* empty */
    }
    return { version: 1, keys: {}, updatedAt: 0 };
  }
}

function ensureCache() {
  if (!cache) {
    cache = readFileFromDisk();
  }
  return cache;
}

function writeFileSync(data) {
  ensureDir();
  let keys = { ...(data.keys || {}) };
  let out = JSON.stringify({
    version: 1,
    keys,
    updatedAt: Date.now(),
  });
  if (Buffer.byteLength(out, "utf8") > MAX_BYTES) {
    const entries = Object.entries(keys).sort(
      (a, b) => Buffer.byteLength(String(b[1]), "utf8") - Buffer.byteLength(String(a[1]), "utf8"),
    );
    for (const [k] of entries) {
      if (Buffer.byteLength(out, "utf8") <= MAX_BYTES) break;
      if (k.includes("grokhub")) continue;
      delete keys[k];
      out = JSON.stringify({ version: 1, keys, updatedAt: Date.now() });
    }
  }
  // rotate backup
  try {
    if (fs.existsSync(statePath())) {
      fs.copyFileSync(statePath(), backupPath());
    }
  } catch {
    /* ignore */
  }
  const tmp = statePath() + ".tmp";
  fs.writeFileSync(tmp, out, { mode: 0o600 });
  fs.renameSync(tmp, statePath());
  cache = { version: 1, keys, updatedAt: Date.now() };
  dirty = false;
  return { ok: true, bytes: Buffer.byteLength(out, "utf8"), path: statePath() };
}

function scheduleFlush() {
  dirty = true;
  if (flushTimer) return;
  flushTimer = setTimeout(() => {
    flushTimer = null;
    if (!dirty || !cache) return;
    try {
      writeFileSync(cache);
    } catch {
      /* disk full */
    }
  }, FLUSH_MS);
}

function flushNow() {
  if (flushTimer) {
    clearTimeout(flushTimer);
    flushTimer = null;
  }
  if (!cache) return { ok: true, skipped: true };
  if (!dirty) return { ok: true, skipped: true };
  return writeFileSync(cache);
}

function get(name) {
  const data = ensureCache();
  const value = data.keys[String(name)];
  return {
    value: typeof value === "string" ? value : value != null ? JSON.stringify(value) : null,
    path: statePath(),
    updatedAt: data.updatedAt || 0,
  };
}

function set(name, value) {
  const data = ensureCache();
  const key = String(name);
  if (value == null || value === "") {
    delete data.keys[key];
  } else {
    data.keys[key] = String(value);
  }
  data.updatedAt = Date.now();
  scheduleFlush();
  return { ok: true, path: statePath(), deferred: true };
}

function remove(name) {
  const data = ensureCache();
  delete data.keys[String(name)];
  data.updatedAt = Date.now();
  scheduleFlush();
  return { ok: true, path: statePath(), deferred: true };
}

function info() {
  const data = ensureCache();
  let bytes = 0;
  try {
    // Prefer on-disk size; fall back to estimate if not flushed yet
    bytes = fs.statSync(statePath()).size;
  } catch {
    try {
      bytes = Buffer.byteLength(JSON.stringify(data), "utf8");
    } catch {
      bytes = 0;
    }
  }
  return {
    path: statePath(),
    backupPath: backupPath(),
    userData: dir(),
    updatedAt: data.updatedAt || 0,
    keys: Object.keys(data.keys || {}),
    bytes,
    dirty,
  };
}

/** Full export for user backup */
function exportAll() {
  flushNow();
  const data = ensureCache();
  return {
    ok: true,
    exportedAt: Date.now(),
    userData: dir(),
    data: { version: 1, keys: { ...data.keys }, updatedAt: data.updatedAt },
  };
}

function importAll(payload) {
  try {
    const body = typeof payload === "string" ? JSON.parse(payload) : payload;
    const keys = body?.data?.keys || body?.keys || body;
    if (!keys || typeof keys !== "object") {
      return { ok: false, error: "Invalid memory backup" };
    }
    // backup current first
    const cur = ensureCache();
    writeFileSync(cur);
    writeFileSync({ keys });
    return { ok: true, path: statePath() };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "import failed" };
  }
}

// Best-effort flush on process exit
try {
  process.on("exit", () => {
    try {
      if (dirty && cache) writeFileSync(cache);
    } catch {
      /* ignore */
    }
  });
} catch {
  /* ignore */
}

module.exports = {
  get,
  set,
  remove,
  info,
  exportAll,
  importAll,
  statePath,
  dir,
  flushNow,
};
