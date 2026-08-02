/**
 * M1 file memory under Electron userData/memory/
 * Never under the install tree — survives updates & factory reinstall of code.
 *
 * Layout (Linux typical):
 *   ~/.config/GrokHub/memory/MEMORY.md
 *   ~/.config/GrokHub/memory/USER.md
 *   ~/.config/GrokHub/memory/LEARNINGS.md
 *   ~/.config/GrokHub/memory/daily/YYYY-MM-DD.md
 *   ~/.config/GrokHub/memory/README.md   (path map for agents / host scans)
 *
 * Note: directory is "GrokHub" (capital G/H), NOT ~/.config/grokhub
 */
const fs = require("node:fs");
const path = require("node:path");

const MAX_FILE_BYTES = 512 * 1024; // 512KB per file safety
const ALLOWED = new Set(["MEMORY.md", "USER.md", "LEARNINGS.md", "README.md", "STATUS.md"]);

function userDataDir() {
  try {
    const { app } = require("electron");
    if (app?.isReady?.() && app?.getPath) return app.getPath("userData");
    if (app?.getPath) {
      try {
        return app.getPath("userData");
      } catch {
        /* not ready */
      }
    }
  } catch {
    /* not in Electron / app not ready */
  }
  const home = process.env.HOME || process.env.USERPROFILE || "/tmp";
  // Electron on Linux uses product name under XDG config — match GrokHub branding
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

function writeIfMissing(abs, lines) {
  if (fs.existsSync(abs)) return false;
  fs.mkdirSync(path.dirname(abs), { recursive: true });
  fs.writeFileSync(abs, lines.join("\n"), "utf8");
  return true;
}

function ensureLayout() {
  fs.mkdirSync(dailyDir(), { recursive: true });
  const root = memoryRoot();
  const ud = userDataDir();

  writeIfMissing(path.join(root, "MEMORY.md"), [
    "# Long-term memory",
    "",
    "Durable facts, decisions, paths, and preferences.",
    "Edited by you, `/memory`, chat compact flush, or self-improve reflect.",
    "",
  ]);
  writeIfMissing(path.join(root, "USER.md"), [
    "# User profile",
    "",
    "Who you are, preferred tools, environment notes.",
    "",
    "- OS: Linux",
    "- Shell: bash",
    "",
  ]);
  writeIfMissing(path.join(root, "LEARNINGS.md"), [
    "# GrokHub learnings",
    "",
    "Distilled self-improvement insights.",
    "Updated by Settings → Learning → Reflect, `/learn reflect`, and automatic turn learning.",
    "",
    "_No reflections yet — use the app and rate replies, then Reflect._",
    "",
  ]);
  writeIfMissing(path.join(root, "README.md"), [
    "# GrokHub file memory",
    "",
    "This folder is the **on-disk** agent memory (survives app updates).",
    "",
    "## Paths (do not look under ~/.config/grokhub — wrong casing)",
    "",
    `- userData: \`${ud}\``,
    `- memory root: \`${root}\``,
    "",
    "| File | Purpose |",
    "|------|---------|",
    "| USER.md | Profile / prefs |",
    "| MEMORY.md | Durable facts & decisions |",
    "| LEARNINGS.md | Self-improve insights + route stats |",
    "| STATUS.md | Live learning status (auto-written) |",
    "| daily/YYYY-MM-DD.md | Day log |",
    "",
    "Also persisted (app state, not plain markdown):",
    `- \`${path.join(ud, "grokhub-memory.json")}\` — chat, learning engine state, workboard, etc.`,
    "",
    "When investigating with HOST_CMD, list **this** directory first:",
    `HOST_CMD: ls -la "${root}"`,
    "",
  ]);

  // Always refresh STATUS skeleton if missing; content updated by renderer sync
  writeIfMissing(path.join(root, "STATUS.md"), [
    "# Learning status",
    "",
    "_Waiting for first learning event from the app…_",
    "",
    `Memory root: \`${root}\``,
    "",
  ]);

  return { root, userData: ud };
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
  for (const name of ["USER.md", "MEMORY.md", "LEARNINGS.md", "STATUS.md", "README.md"]) {
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
  return { ok: true, id: target, path: abs, bytes: r.bytes };
}

function append(rel, text, { heading } = {}) {
  ensureLayout();
  const target =
    !rel || rel === "today"
      ? `daily/${todaySlug()}.md`
      : rel === "memory"
        ? "MEMORY.md"
        : rel === "user"
          ? "USER.md"
          : rel === "learnings" || rel === "learning"
            ? "LEARNINGS.md"
            : rel;
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
 * Write live learning status + optional full LEARNINGS body from renderer.
 */
function syncLearning(payload = {}) {
  ensureLayout();
  const status = String(payload.statusMarkdown || "").trim();
  const learnings = String(payload.learningsMarkdown || "").trim();
  const results = {};
  if (status) {
    results.status = writeFileSafe(path.join(memoryRoot(), "STATUS.md"), status + "\n");
  }
  if (learnings) {
    results.learnings = writeFileSafe(
      path.join(memoryRoot(), "LEARNINGS.md"),
      learnings + "\n",
    );
  }
  return { ok: true, root: memoryRoot(), ...results };
}

function buildPinBundle(opts = {}) {
  ensureLayout();
  const maxUser = opts.maxUserChars ?? 3_000;
  const maxMemory = opts.maxMemoryChars ?? 6_000;
  const maxDaily = opts.maxDailyChars ?? 4_000;
  const maxLearn = opts.maxLearnChars ?? 3_000;
  const maxTotal = opts.maxTotalChars ?? 14_000;

  const user = readFileSafe(path.join(memoryRoot(), "USER.md")).trim();
  const mem = readFileSafe(path.join(memoryRoot(), "MEMORY.md")).trim();
  const learn = readFileSafe(path.join(memoryRoot(), "LEARNINGS.md")).trim();
  const slug = todaySlug();
  const today = readFileSafe(path.join(dailyDir(), `${slug}.md`)).trim();
  const y = new Date();
  y.setDate(y.getDate() - 1);
  const ySlug = y.toISOString().slice(0, 10);
  const yesterday = readFileSafe(path.join(dailyDir(), `${ySlug}.md`)).trim();

  const parts = [];
  const clip = (s, n) =>
    s.length <= n ? s : s.slice(0, n) + "\n…[truncated for context budget]…";

  parts.push(`## Memory paths (authoritative)\n- root: \`${memoryRoot()}\`\n- userData: \`${userDataDir()}\``);
  if (user) parts.push(`## USER.md\n${clip(user, maxUser)}`);
  if (mem) parts.push(`## MEMORY.md\n${clip(mem, maxMemory)}`);
  if (learn && !/^_No reflections/i.test(learn.split("\n").filter(Boolean).pop() || "")) {
    parts.push(`## LEARNINGS.md\n${clip(learn, maxLearn)}`);
  }
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
    userData: userDataDir(),
    bundle,
    chars: bundle.length,
    hasUser: Boolean(user),
    hasMemory: Boolean(mem),
    hasLearnings: Boolean(learn),
    hasToday: Boolean(today),
  };
}

function info() {
  const layout = ensureLayout();
  const files = listFiles();
  let bytes = 0;
  for (const f of files) bytes += f.bytes || 0;
  return {
    ok: true,
    root: layout.root,
    userData: layout.userData,
    files,
    bytes,
    today: todaySlug(),
    /** Common wrong path agents search for */
    notThisPath: path.join(
      process.env.XDG_CONFIG_HOME || path.join(process.env.HOME || "", ".config"),
      "grokhub",
    ),
  };
}

module.exports = {
  memoryRoot,
  userDataDir,
  ensureLayout,
  listFiles,
  read,
  write,
  append,
  appendFacts,
  buildPinBundle,
  syncLearning,
  info,
  todaySlug,
};
