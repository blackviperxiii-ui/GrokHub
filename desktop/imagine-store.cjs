/**
 * Persist Imagine images/videos under userData so they survive updates.
 * JSON memory only keeps metadata + relative paths; bytes live on disk.
 */
const fs = require("node:fs");
const fsp = require("node:fs/promises");
const path = require("node:path");
const crypto = require("node:crypto");
const os = require("node:os");

function electronApp() {
  try {
    return require("electron").app;
  } catch {
    return null;
  }
}

function userDataDir() {
  const app = electronApp();
  try {
    if (app?.getPath) return app.getPath("userData");
  } catch {
    /* ignore */
  }
  const home = process.env.HOME || os.homedir() || "/tmp";
  return path.join(
    process.env.XDG_CONFIG_HOME || path.join(home, ".config"),
    "GrokHub",
  );
}

function rootDir() {
  return path.join(userDataDir(), "imagine-media");
}

function ensureRoot() {
  fs.mkdirSync(rootDir(), { recursive: true });
}

function safeId(id) {
  return String(id || "x").replace(/[^a-zA-Z0-9_-]/g, "_").slice(0, 80);
}

function extFor(mime, kind) {
  const m = String(mime || "").toLowerCase();
  if (m.includes("svg")) return "svg";
  if (m.includes("png")) return "png";
  if (m.includes("jpeg") || m.includes("jpg")) return "jpg";
  if (m.includes("webp")) return "webp";
  if (m.includes("gif")) return "gif";
  if (m.includes("mp4")) return "mp4";
  if (m.includes("webm")) return "webm";
  if (kind === "video") return "mp4";
  return "png";
}

function parseDataUrl(dataUrl) {
  const s = String(dataUrl || "");
  const m = /^data:([^;,]+)?(;base64)?,(.*)$/s.exec(s);
  if (!m) return null;
  const mime = m[1] || "application/octet-stream";
  const b64 = m[2] ? m[3] : Buffer.from(decodeURIComponent(m[3]), "utf8").toString("base64");
  return { mime, buffer: Buffer.from(b64, "base64") };
}

async function saveMedia(jobId, dataUrl, kind = "image") {
  ensureRoot();
  // Remote https URL — store as pointer file
  if (/^https?:\/\//i.test(String(dataUrl || ""))) {
    const rel = path.join(safeId(jobId), kind === "video" ? "remote-video.url" : "remote-image.url");
    const abs = path.join(rootDir(), rel);
    await fsp.mkdir(path.dirname(abs), { recursive: true });
    await fsp.writeFile(abs, String(dataUrl), "utf8");
    return { ok: true, relPath: rel.replace(/\\/g, "/"), kind: "url", url: String(dataUrl) };
  }
  const parsed = parseDataUrl(dataUrl);
  if (!parsed || !parsed.buffer.length) {
    return { ok: false, error: "unsupported media payload" };
  }
  const ext = extFor(parsed.mime, kind);
  const rel = path.join(safeId(jobId), `${kind}.${ext}`);
  const abs = path.join(rootDir(), rel);
  await fsp.mkdir(path.dirname(abs), { recursive: true });
  await fsp.writeFile(abs, parsed.buffer);
  return {
    ok: true,
    relPath: rel.replace(/\\/g, "/"),
    mime: parsed.mime,
    bytes: parsed.buffer.length,
  };
}

async function loadMedia(relPath) {
  if (!relPath) return { ok: false, error: "empty path" };
  const abs = path.join(rootDir(), relPath);
  // path traversal guard
  if (!abs.startsWith(rootDir())) return { ok: false, error: "invalid path" };
  try {
    if (abs.endsWith(".url")) {
      const url = (await fsp.readFile(abs, "utf8")).trim();
      return { ok: true, dataUrl: url, isRemote: true };
    }
    const buf = await fsp.readFile(abs);
    const ext = path.extname(abs).toLowerCase();
    const mime =
      ext === ".svg"
        ? "image/svg+xml"
        : ext === ".jpg" || ext === ".jpeg"
          ? "image/jpeg"
          : ext === ".webp"
            ? "image/webp"
            : ext === ".gif"
              ? "image/gif"
              : ext === ".mp4"
                ? "video/mp4"
                : ext === ".webm"
                  ? "video/webm"
                  : "image/png";
    const dataUrl = `data:${mime};base64,${buf.toString("base64")}`;
    return { ok: true, dataUrl, mime, bytes: buf.length };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "read failed" };
  }
}

async function deleteJobMedia(jobId) {
  const dir = path.join(rootDir(), safeId(jobId));
  try {
    await fsp.rm(dir, { recursive: true, force: true });
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "delete failed" };
  }
}

async function clearAll() {
  try {
    await fsp.rm(rootDir(), { recursive: true, force: true });
    ensureRoot();
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "clear failed" };
  }
}

module.exports = {
  rootDir,
  saveMedia,
  loadMedia,
  deleteJobMedia,
  clearAll,
};
