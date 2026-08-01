/**
 * Persistent app memory under Electron userData.
 * Survives restarts and in-place updates (updates never touch userData).
 */
const { app } = require("electron");
const fs = require("node:fs");
const path = require("node:path");

const STATE_FILE = "grokhub-memory.json";
const BACKUP_FILE = "grokhub-memory.backup.json";
const MAX_BYTES = 24 * 1024 * 1024; // 24MB safety cap

function dir() {
  return app.getPath("userData");
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

/**
 * Read raw string value for a named key (zustand store name).
 * File shape: { version: 1, keys: { [name]: string }, updatedAt }
 */
function readFile() {
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

function writeFile(data) {
  ensureDir();
  const payload = JSON.stringify({
    version: 1,
    keys: data.keys || {},
    updatedAt: Date.now(),
  });
  if (Buffer.byteLength(payload, "utf8") > MAX_BYTES) {
    // Drop largest key values until under cap (keep grokhub primary)
    const keys = { ...data.keys };
    const entries = Object.entries(keys).sort(
      (a, b) => Buffer.byteLength(String(b[1]), "utf8") - Buffer.byteLength(String(a[1]), "utf8"),
    );
    let json = payload;
    for (const [k] of entries) {
      if (Buffer.byteLength(json, "utf8") <= MAX_BYTES) break;
      if (k.includes("grokhub")) continue;
      delete keys[k];
      json = JSON.stringify({ version: 1, keys, updatedAt: Date.now() });
    }
    // last resort: truncate primary store payload's nested data is caller's problem
    data = { keys };
  }
  const out = JSON.stringify({
    version: 1,
    keys: data.keys || {},
    updatedAt: Date.now(),
  });
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
  return { ok: true, bytes: Buffer.byteLength(out, "utf8"), path: statePath() };
}

function get(name) {
  const data = readFile();
  const value = data.keys[String(name)];
  return {
    value: typeof value === "string" ? value : value != null ? JSON.stringify(value) : null,
    path: statePath(),
    updatedAt: data.updatedAt || 0,
  };
}

function set(name, value) {
  const data = readFile();
  const key = String(name);
  if (value == null || value === "") {
    delete data.keys[key];
  } else {
    data.keys[key] = String(value);
  }
  return writeFile(data);
}

function remove(name) {
  const data = readFile();
  delete data.keys[String(name)];
  return writeFile(data);
}

function info() {
  const data = readFile();
  let bytes = 0;
  try {
    bytes = fs.statSync(statePath()).size;
  } catch {
    bytes = 0;
  }
  return {
    path: statePath(),
    backupPath: backupPath(),
    userData: dir(),
    updatedAt: data.updatedAt || 0,
    keys: Object.keys(data.keys || {}),
    bytes,
  };
}

/** Full export for user backup */
function exportAll() {
  const data = readFile();
  return {
    ok: true,
    exportedAt: Date.now(),
    userData: dir(),
    data,
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
    const cur = readFile();
    writeFile(cur);
    writeFile({ keys });
    return { ok: true, path: statePath() };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "import failed" };
  }
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
};
