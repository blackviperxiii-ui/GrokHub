/**
 * Encrypted secrets for Electron (safeStorage + userData file).
 * Lazy-requires electron so plain Node (tests/smoke) can load the module.
 */
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

let _electron = null;
function electron() {
  if (_electron) return _electron;
  try {
    _electron = require("electron");
  } catch {
    _electron = null;
  }
  return _electron;
}

function userDataDir() {
  const el = electron();
  try {
    if (el?.app?.getPath) return el.app.getPath("userData");
  } catch {
    /* not ready */
  }
  const home = process.env.HOME || os.homedir() || "/tmp";
  return path.join(home, ".config", "GrokHub");
}

function storePath() {
  return path.join(userDataDir(), "grokhub-secrets.json");
}

function readStore() {
  try {
    const raw = fs.readFileSync(storePath(), "utf8");
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

function writeStore(obj) {
  const dir = path.dirname(storePath());
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(storePath(), JSON.stringify(obj), { mode: 0o600 });
}

function encrypt(value) {
  const s = String(value ?? "");
  if (!s) return { empty: true };
  const el = electron();
  try {
    if (el?.safeStorage?.isEncryptionAvailable?.()) {
      const buf = el.safeStorage.encryptString(s);
      return { enc: buf.toString("base64") };
    }
  } catch {
    /* fall through */
  }
  return { plain: s };
}

function decrypt(entry) {
  if (!entry || entry.empty) return "";
  const el = electron();
  if (entry.enc) {
    try {
      if (el?.safeStorage?.isEncryptionAvailable?.()) {
        return el.safeStorage.decryptString(Buffer.from(entry.enc, "base64"));
      }
    } catch {
      return "";
    }
  }
  return entry.plain || "";
}

function set(key, value) {
  const store = readStore();
  if (!value) {
    delete store[key];
  } else {
    store[key] = encrypt(value);
  }
  writeStore(store);
  return { ok: true };
}

function get(key) {
  const store = readStore();
  return { value: decrypt(store[key]) };
}

function del(key) {
  const store = readStore();
  delete store[key];
  writeStore(store);
  return { ok: true };
}

module.exports = { set, get, delete: del, del };
