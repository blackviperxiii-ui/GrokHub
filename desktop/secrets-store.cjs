/**
 * Encrypted secrets for Electron (safeStorage + userData file).
 */
const { safeStorage, app } = require("electron");
const fs = require("node:fs");
const path = require("node:path");

function storePath() {
  return path.join(app.getPath("userData"), "grokhub-secrets.json");
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
  if (safeStorage.isEncryptionAvailable()) {
    const buf = safeStorage.encryptString(s);
    return { enc: buf.toString("base64") };
  }
  // Fallback: still isolate from renderer localStorage (file mode 600)
  return { plain: s };
}

function decrypt(entry) {
  if (!entry || entry.empty) return "";
  if (entry.enc && safeStorage.isEncryptionAvailable()) {
    try {
      return safeStorage.decryptString(Buffer.from(entry.enc, "base64"));
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
