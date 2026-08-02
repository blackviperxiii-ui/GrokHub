/**
 * M1 file memory under Electron userData/memory/
 * Never under the install tree — survives updates & factory reinstall of code.
 *
 * Layout:
 *   MEMORY.md
 *   USER.md
 *   daily/YYYY-MM-DD.md
 */
const fs = require("node:fs");
const path = require("node:path");

const MAX_FILE_BYTES = 512 * 1024; // 512KB per file safety
const ALLOWED = new Set(["MEMORY.md", "USER.md", "LEARNINGS.md"]);

function userDataDir() {
  try {
    const { app } = require("electron");
    if (app?.getPath) return app.getPath("userData");
  } catch {
    /* not in Electron / app not ready */
  }
  const home = process.env.HOME || process.env.USERPROFILE || "/tmp";
  return path.join(
    process.env.XDG_CONFIG_HOME || path.join(home, ".config"),
    "GrokHub",
  );
}

function memoryRoot() {
  return path.join(userDataDir(), "memory");
}

function dailyDir() {
  return path.join(memoryRoot(), "daily");
}

function ensureLayout() {
  fs.mkdirSync(dailyDir(), { recursive: true });
  const mem = path.join(memoryRoot(), "MEMORY.md");
  const user = path.join(memoryRoot(), "USER.md");
  if (!fs.existsSync(mem)) {
    fs.writeFileSync(
      mem,
      [
        "# Long-term memory",
        "",
        "Durable facts, decisions, paths, and preferences.",
        "Edited by you or flushed from chat compaction.",
        "",
      ].join("\n"),
      "utf8",
    );
  }
  if (!fs.existsSync(user)) {
    fs.writeFileSync(
      user,
      [
        "# User profile",
        "",
        "Who you are, preferred tools, environment notes.",
        "",
        "- OS: Linux",
        "- Shell: bash",
        "",
      ].join("\n"),
      "utf8",
    );
  }
  const learn = path.join(memoryRoot(), "LEARNINGS.md");
  if (!fs.existsSync(learn)) {
    fs.writeFileSync(
      learn,
      [
        "# GrokHub learnings",
        "",
        "Distilled self-improvement insights. Updated by Reflect / compact.",
        "",
      ].join("\n"),
      "utf8",
    );
  }
}

function todaySlug() {
  return new Date().toISOString().slice(0, 10);
}

function resolveSafe(rel) {
  const root = path.resolve(memoryRoot());
  const name = String(rel || "").replace(/\\/g, "/").replace(/^\/+/, "");
  if (!name || name.includes("..")) return null;
  if (ALLOWED.has(name)) return path.join(root, name);
  const day = name.match(/^daily\/(\d{4}-\d{2}-\d{2})(?:\.md)?$/);
  if (day) return path.join(root, "daily", `${day[1]}.md`);
  if (/^\d{4}-\d{2}-\d{2}\.md$/.test(name)) return path.join(root, "daily", name);
  return null;
}

function readFileSafe(abs) {
  try {
    if (!fs.existsSync(abs)) return "";
    const st = fs.statSync(abs);
    if (!st.isFile()) return "";
    const buf = fs.readFileSync(abs);
    if (buf.length > MAX_FILE_BYTES) {
      return buf.subarray(0, MAX_FILE_BYTES).toString("utf8") + "\n…[truncated]…\n";
    }
    return buf.toString("utf8");
  } catch {
    return "";
  }
}

function writeFileSafe(abs, content) {
  ensureLayout();
  const text = String(content ?? "");
  const buf = Buffer.from(text, "utf8");
  if (buf.length > MAX_FILE_BYTES) {
    return { ok: false, error: `File exceeds ${MAX_FILE_BYTES} bytes` };
  }
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  const tmp = abs + ".tmp";
  fs.writeFileSync(tmp, buf);
  fs.renameSync(tmp, abs);
  return { ok: true, path: abs, bytes: buf.length };
}

function listFiles() {
  ensureLayout();
  const out = [];
  for (const name of ["USER.md", "MEMORY.md", "LEARNINGS.md"]) {
    const abs = path.join(memoryRoot(), name);
    let bytes = 0;
    let updatedAt = 0;
    try {
      const st = fs.statSync(abs);
      bytes = st.size;
      updatedAt = st.mtimeMs;
    } catch {
      /* missing */
    }
    out.push({ id: name, name, kind: "core", bytes, updatedAt });
  }
  try {
    const days = fs
      .readdirSync(dailyDir())
      .filter((n) => /^\d{4}-\d{2}-\d{2}\.md$/.test(n))
      .sort()
      .reverse();
    for (const n of days.slice(0, 60)) {
      const abs = path.join(dailyDir(), n);
      try {
        const st = fs.statSync(abs);
        out.push({
          id: `daily/${n}`,
          name: n,
          kind: "daily",
          bytes: st.size,
          updatedAt: st.mtimeMs,
        });
      } catch {
        /* skip */
      }
    }
  } catch {
    /* no daily */
  }
  return out;
}

function read(rel) {
  ensureLayout();
  if (!rel || rel === "today") {
    const slug = todaySlug();
    const abs = path.join(dailyDir(), `${slug}.md`);
    return {
      ok: true,
      id: `daily/${slug}.md`,
      content: readFileSafe(abs),
      path: abs,
    };
  }
  const abs = resolveSafe(rel);
  if (!abs) return { ok: false, error: "Invalid memory path" };
  return {
    ok: true,
    id: rel.includes("daily") || /^\d{4}/.test(rel) ? (rel.startsWith("daily/") ? rel : `daily/${rel}`) : rel,
    content: readFileSafe(abs),
    path: abs,
  };
}

function write(rel, content) {
  ensureLayout();
  const target = !rel || rel === "today" ? `daily/${todaySlug()}.md` : rel;
  const abs = resolveSafe(target);
  if (!abs) return { ok: false, error: "Invalid memory path" };
  const r = writeFileSafe(abs, content);
  if (!r.ok) return r;
  return { ok: true, id: target.startsWith("daily/") ? target : path.basename(abs) === path.basename(target) ? target : target, path: abs, bytes: r.bytes };
}

function append(rel, text, { heading } = {}) {
  ensureLayout();
  const target = !rel || rel === "today" ? `daily/${todaySlug()}.md` : rel === "memory" ? "MEMORY.md" : rel === "user" ? "USER.md" : rel;
  const abs = resolveSafe(target);
  if (!abs) return { ok: false, error: "Invalid memory path" };
  const prev = readFileSafe(abs);
  const stamp = new Date().toISOString().replace("T", " ").slice(0, 16);
  const block = String(text || "").trim();
  if (!block) return { ok: false, error: "Empty note" };
  const chunk = heading
    ? `\n## ${heading}\n\n- ${stamp}: ${block}\n`
    : `\n- ${stamp}: ${block}\n`;
  const next = (prev || "").trimEnd() + chunk;
  const r = writeFileSafe(abs, next);
  if (!r.ok) return r;
  return { ok: true, id: target, path: abs, bytes: r.bytes, appended: block };
}

function appendFacts(facts, { target = "MEMORY.md" } = {}) {
  const list = (facts || []).map((f) => String(f || "").trim()).filter((f) => f.length >= 8);
  if (!list.length) return { ok: true, added: 0 };
  ensureLayout();
  const abs = resolveSafe(target === "today" ? `daily/${todaySlug()}.md` : target);
  if (!abs) return { ok: false, error: "Invalid target" };
  let body = readFileSafe(abs);
  const lower = body.toLowerCase();
  let added = 0;
  const stamp = new Date().toISOString().slice(0, 10);
  for (const f of list) {
    if (lower.includes(f.toLowerCase().slice(0, 40))) continue;
    body = body.trimEnd() + `\n- ${stamp}: ${f}`;
    added += 1;
  }
  if (added) {
    const r = writeFileSafe(abs, body.trimEnd() + "\n");
    if (!r.ok) return r;
  }
  return { ok: true, added, path: abs };
}

/**
 * Build a budgeted pin string for the model context.
 */
function buildPinBundle(opts = {}) {
  ensureLayout();
  const maxUser = opts.maxUserChars ?? 3_000;
  const maxMemory = opts.maxMemoryChars ?? 6_000;
  const maxDaily = opts.maxDailyChars ?? 4_000;
  const maxTotal = opts.maxTotalChars ?? 12_000;

  const user = readFileSafe(path.join(memoryRoot(), "USER.md")).trim();
  const mem = readFileSafe(path.join(memoryRoot(), "MEMORY.md")).trim();
  const slug = todaySlug();
  const today = readFileSafe(path.join(dailyDir(), `${slug}.md`)).trim();
  // yesterday
  const y = new Date();
  y.setDate(y.getDate() - 1);
  const ySlug = y.toISOString().slice(0, 10);
  const yesterday = readFileSafe(path.join(dailyDir(), `${ySlug}.md`)).trim();

  const parts = [];
  const clip = (s, n) =>
    s.length <= n ? s : s.slice(0, n) + "\n…[truncated for context budget]…";

  if (user) parts.push(`## USER.md\n${clip(user, maxUser)}`);
  if (mem) parts.push(`## MEMORY.md\n${clip(mem, maxMemory)}`);
  if (today) parts.push(`## daily/${slug}.md (today)\n${clip(today, maxDaily)}`);
  else if (yesterday)
    parts.push(`## daily/${ySlug}.md (yesterday)\n${clip(yesterday, Math.floor(maxDaily * 0.75))}`);

  let bundle = parts.join("\n\n").trim();
  if (bundle.length > maxTotal) {
    bundle = bundle.slice(0, maxTotal) + "\n…[memory pin total cap]…";
  }
  return {
    ok: true,
    root: memoryRoot(),
    bundle,
    chars: bundle.length,
    hasUser: Boolean(user),
    hasMemory: Boolean(mem),
    hasToday: Boolean(today),
  };
}

function info() {
  ensureLayout();
  const files = listFiles();
  let bytes = 0;
  for (const f of files) bytes += f.bytes || 0;
  return {
    ok: true,
    root: memoryRoot(),
    userData: userDataDir(),
    files,
    bytes,
    today: todaySlug(),
  };
}

module.exports = {
  memoryRoot,
  ensureLayout,
  listFiles,
  read,
  write,
  append,
  appendFacts,
  buildPinBundle,
  info,
  todaySlug,
};
