/**
 * Durable app logs for field debugging (update, crash, UI lifecycle).
 * Writes under userData/config — never under the install tree.
 */
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

function configDir() {
  if (process.platform === "win32") {
    return path.join(
      process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming"),
      "GrokHub",
    );
  }
  return path.join(
    process.env.XDG_CONFIG_HOME || path.join(os.homedir(), ".config"),
    "GrokHub",
  );
}

function logDir() {
  return path.join(configDir(), "logs");
}

function ensureLogDir() {
  try {
    fs.mkdirSync(logDir(), { recursive: true, mode: 0o700 });
  } catch {
    /* ignore */
  }
}

function logFile() {
  const d = new Date();
  const day = d.toISOString().slice(0, 10);
  return path.join(logDir(), `app-${day}.log`);
}

/** Append one structured line; also mirrors to console. */
function write(level, msg, extra) {
  const line = {
    ts: new Date().toISOString(),
    level,
    msg: String(msg || ""),
    ...(extra && typeof extra === "object" ? extra : extra != null ? { extra } : {}),
    pid: process.pid,
  };
  const text = JSON.stringify(line);
  try {
    ensureLogDir();
    fs.appendFileSync(logFile(), text + "\n", { mode: 0o600 });
  } catch {
    /* disk full / permissions */
  }
  if (level === "error") console.error("[GrokHub]", msg, extra || "");
  else if (level === "warn") console.warn("[GrokHub]", msg, extra || "");
  else console.log("[GrokHub]", msg);
}

function info(msg, extra) {
  write("info", msg, extra);
}
function warn(msg, extra) {
  write("warn", msg, extra);
}
function error(msg, extra) {
  write("error", msg, extra);
}

/** Last N lines of today's log (for Settings / support). */
function tail(n = 80) {
  try {
    const raw = fs.readFileSync(logFile(), "utf8");
    const lines = raw.trim().split("\n");
    return lines.slice(-Math.max(1, n)).join("\n");
  } catch {
    return "";
  }
}

function paths() {
  return { configDir: configDir(), logDir: logDir(), logFile: logFile() };
}

module.exports = { info, warn, error, write, tail, paths, configDir, logDir };
