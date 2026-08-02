/**
 * Start / wait for the GrokHub Nitro UI (.output/server).
 * Works for source installs AND packaged Electron (NSIS/portable) via
 * ELECTRON_RUN_AS_NODE so no system Node.js is required.
 */
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const os = require("node:os");

function appRootFrom(desktopDir) {
  // 1) Explicit install home
  if (process.env.GROKHUB_HOME) {
    const h = path.resolve(process.env.GROKHUB_HOME);
    if (fs.existsSync(path.join(h, ".output", "server", "index.mjs"))) return h;
  }
  // 1b) User install layouts (prefer over system)
  const home = os.homedir();
  for (const h of [
    path.join(home, ".local/lib/grokhub"),
    path.join(home, ".local/share/grokhub"),
  ]) {
    if (fs.existsSync(path.join(h, ".output", "server", "index.mjs"))) return h;
  }
  // 2) Packaged Electron — prefer app.asar.unpacked (spawn can't exec inside asar)
  try {
    const { app } = require("electron");
    if (app?.isPackaged) {
      const appPath = app.getAppPath();
      const candidates = [];
      // asarUnpack lands next to app.asar
      if (appPath.endsWith(".asar")) {
        candidates.push(appPath + ".unpacked");
        candidates.push(path.join(path.dirname(appPath), "app.asar.unpacked"));
      }
      candidates.push(appPath);
      if (process.resourcesPath) {
        const r = process.resourcesPath;
        candidates.push(path.join(r, "app.asar.unpacked"));
        candidates.push(path.join(r, "app"));
        candidates.push(r);
      }
      for (const c of candidates) {
        if (c && fs.existsSync(path.join(c, ".output", "server", "index.mjs"))) {
          return c;
        }
      }
      return appPath;
    }
  } catch {
    /* not in electron yet */
  }
  // 3) system install
  if (fs.existsSync(path.join("/usr/lib/grokhub", ".output", "server", "index.mjs"))) {
    return "/usr/lib/grokhub";
  }
  // 4) desktop/ sibling of .output
  const sibling = path.resolve(desktopDir, "..");
  if (fs.existsSync(path.join(sibling, ".output", "server", "index.mjs"))) return sibling;
  return sibling;
}

function serverEntry(root) {
  return path.join(root, ".output", "server", "index.mjs");
}

function pickPort() {
  const n = Number(process.env.GROKHUB_PORT || process.env.PORT || 18765);
  return Number.isFinite(n) && n > 0 ? n : 18765;
}

function probe(url, timeoutMs = 800) {
  return new Promise((resolve) => {
    const req = http.get(url, { timeout: timeoutMs }, (res) => {
      res.resume();
      resolve(res.statusCode && res.statusCode < 500);
    });
    req.on("error", () => resolve(false));
    req.on("timeout", () => {
      try {
        req.destroy();
      } catch {
        /* ignore */
      }
      resolve(false);
    });
  });
}

function runtimeDir() {
  if (process.platform === "win32") {
    return path.join(
      process.env.LOCALAPPDATA || path.join(os.homedir(), "AppData", "Local"),
      "GrokHub",
      "runtime",
    );
  }
  return path.join(process.env.XDG_RUNTIME_DIR || "/tmp", "grokhub");
}

/**
 * Prefer Electron-as-Node so packaged apps need no system Node install.
 */

function diagLog(line) {
  try {
    fs.appendFileSync("/tmp/grokhub-ui-restart.log", line.endsWith("\n") ? line : line + "\n");
  } catch {
    /* ignore */
  }
}

function rotateDiagLog() {
  try {
    const p = "/tmp/grokhub-ui-restart.log";
    if (!fs.existsSync(p)) return;
    const st = fs.statSync(p);
    if (st.size > 200_000) {
      try {
        fs.renameSync(p, p + ".prev");
      } catch {
        fs.writeFileSync(p, "");
      }
    }
  } catch {
    /* ignore */
  }
}

function isOurUiPidSync(pid) {
  if (!pid || pid <= 1) return false;
  try {
    process.kill(pid, 0);
  } catch {
    return false;
  }
  try {
    if (process.platform === "linux") {
      const raw = fs.readFileSync(`/proc/${pid}/cmdline`, "utf8");
      const cmd = raw.replace(/\0/g, " ");
      const isNode = /\bnode\b|ELECTRON_RUN_AS_NODE/i.test(cmd) || /\/node\s/.test(cmd);
      const isUi =
        /\.output\/server|index\.mjs|nitro|grokhub/i.test(cmd) &&
        !/desktop\/main\.mjs/i.test(cmd);
      return isNode && isUi;
    }
    return true;
  } catch {
    return false;
  }
}

/** Kill a stale UI pid we own (pidfile/lock), never random processes. */
function killOurUiPid(pid, reason) {
  if (!isOurUiPidSync(pid)) return false;
  try {
    process.kill(pid, "SIGTERM");
    diagLog(`[ui-server] SIGTERM pid=${pid} (${reason})`);
  } catch {
    return false;
  }
  const until = Date.now() + 1500;
  while (Date.now() < until) {
    try {
      process.kill(pid, 0);
    } catch {
      return true;
    }
    // short spin — only used during reclaim
    const spin = Date.now() + 40;
    while (Date.now() < spin) { /* wait */ }
  }
  try {
    process.kill(pid, "SIGKILL");
    diagLog(`[ui-server] SIGKILL pid=${pid}`);
  } catch {
    /* ignore */
  }
  return true;
}


function whichNode() {
  const { execFileSync } = require("node:child_process");
  try {
    if (process.platform === "win32") {
      const out = execFileSync("where", ["node"], { encoding: "utf8" }).trim();
      return out.split(/\r?\n/)[0] || "node.exe";
    }
    const out = execFileSync("bash", ["-lc", "command -v node"], {
      encoding: "utf8",
    }).trim();
    if (out) return out;
  } catch {
    /* fall through */
  }
  return null;
}

function nodeSpawnSpec(entry) {
  // Prefer real Node on PATH — most reliable for Nitro on Arch/system electron.
  const nodeBin = whichNode();
  if (nodeBin) {
    return {
      bin: nodeBin,
      args: [entry],
      envExtra: {},
      shell: process.platform === "win32",
    };
  }
  // Packaged Windows/mac without system node: Electron-as-Node
  const hasElectron =
    Boolean(process.versions.electron) ||
    /electron|grokhub/i.test(process.execPath);
  if (hasElectron) {
    return {
      bin: process.execPath,
      args: [entry],
      envExtra: { ELECTRON_RUN_AS_NODE: "1" },
    };
  }
  if (process.platform === "win32") {
    return { bin: "node.exe", args: [entry], envExtra: {}, shell: true };
  }
  return { bin: "node", args: [entry], envExtra: {}, shell: false };
}

/**
 * @param {string} desktopDir absolute path to desktop/ (usually __dirname)
 */
async function ensureUiServer(desktopDir) {
  let root = appRootFrom(desktopDir);
  // Guard: never treat $HOME as install root (relative .output → ~/ .output bug)
  try {
    const home = path.resolve(os.homedir());
    if (path.resolve(root) === home || !fs.existsSync(path.join(root, ".output", "server", "index.mjs"))) {
      for (const cand of [
        path.join(home, ".local/lib/grokhub"),
        path.join(home, ".local/share/grokhub"),
        "/usr/lib/grokhub",
      ]) {
        if (fs.existsSync(path.join(cand, ".output", "server", "index.mjs"))) {
          root = cand;
          break;
        }
      }
    }
  } catch {
    /* ignore */
  }
  const port = pickPort();
  const url = (process.env.GROKHUB_URL || `http://127.0.0.1:${port}`).replace(
    /\/$/,
    "",
  );

  rotateDiagLog();
  diagLog(`[ui-server] ensure root=${root} url=${url}`);

  if (await probe(url + "/")) {
    process.env.GROKHUB_URL = url;
    process.env.GROKHUB_HOME = root;
    return { url, started: false, root };
  }

  const entry = path.resolve(serverEntry(root));
  if (!fs.existsSync(entry)) {
    const err = `UI build missing: ${entry}`;
    diagLog(`[ui-server] ${err}`);
    return {
      url,
      started: false,
      root,
      error: err,
    };
  }

  const rt = runtimeDir();
  try {
    fs.mkdirSync(rt, { recursive: true });
  } catch {
    /* ignore */
  }
  // Single-writer lock + pidfile so we can reclaim dead / unhealthy backends
  const lockPath = path.join(rt, "ui.lock");
  const pidPath = path.join(rt, "ui.pid");
  for (const file of [lockPath, pidPath]) {
    try {
      if (!fs.existsSync(file)) continue;
      const prev = Number(fs.readFileSync(file, "utf8").trim());
      if (!prev || Number.isNaN(prev)) {
        try { fs.unlinkSync(file); } catch { /* ignore */ }
        continue;
      }
      if (!isOurUiPidSync(prev)) {
        // Dead or foreign — drop stamp
        try { fs.unlinkSync(file); } catch { /* ignore */ }
        continue;
      }
      // Alive: wait briefly for health, else kill and reclaim
      const waitUntil = Date.now() + 8_000;
      let healthy = false;
      while (Date.now() < waitUntil) {
        if (await probe(url + "/")) {
          healthy = true;
          break;
        }
        await new Promise((r) => setTimeout(r, 250));
      }
      if (healthy) {
        process.env.GROKHUB_URL = url;
        process.env.GROKHUB_HOME = root;
        diagLog(`[ui-server] reuse healthy pid=${prev}`);
        return { url, started: false, root, reused: true };
      }
      killOurUiPid(prev, "alive but not healthy on " + url);
      try { fs.unlinkSync(file); } catch { /* ignore */ }
    } catch {
      /* ignore */
    }
  }
  const logPath = path.join(rt, "ui.log");
  let logFd;
  try {
    logFd = fs.openSync(logPath, "a");
  } catch {
    logFd = "ignore";
  }

  const spec = nodeSpawnSpec(entry);
  const env = {
    ...process.env,
    ...spec.envExtra,
    PORT: String(port),
    NITRO_PORT: String(port),
    HOST: "127.0.0.1",
    NITRO_HOST: "127.0.0.1",
    GROKHUB_HOME: root,
  };

  try {
    const line = `\n[ui-server] spawn ${spec.bin} ${spec.args.join(" ")} cwd=${root} entry=${entry}\n`;
    fs.appendFileSync(logPath, line);
    try {
      fs.appendFileSync("/tmp/grokhub-ui-restart.log", line);
    } catch {
      /* ignore */
    }
  } catch {
    /* ignore */
  }

  const child = spawn(spec.bin, spec.args, {
    cwd: root,
    env,
    detached: true,
    stdio: logFd === "ignore" ? "ignore" : ["ignore", logFd, logFd],
    windowsHide: true,
    shell: Boolean(spec.shell),
  });
  try {
    if (child.pid) {
      fs.writeFileSync(lockPath, String(child.pid) + "\n");
      fs.writeFileSync(pidPath, String(child.pid) + "\n");
      process.env.GROKHUB_UI_PID = String(child.pid);
      diagLog(`[ui-server] spawned pid=${child.pid} entry=${entry} cwd=${root}`);
    }
  } catch {
    /* ignore */
  }
  child.unref();
  child.on("error", (err) => {
    try {
      fs.appendFileSync(
        logPath,
        `\n[ui-server] spawn failed: ${err && err.message}\n`,
      );
    } catch {
      /* ignore */
    }
  });

  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    if (await probe(`http://127.0.0.1:${port}/`)) {
      const finalUrl = `http://127.0.0.1:${port}`;
      process.env.GROKHUB_URL = finalUrl;
      return { url: finalUrl, started: true, root };
    }
    await new Promise((r) => setTimeout(r, 200));
  }

  return {
    url: `http://127.0.0.1:${port}`,
    started: true,
    root,
    error: `UI server did not become ready on port ${port}. See ${logPath}`,
  };
}

async function resolveStartUrl(desktopDir) {
  if (process.env.GROKHUB_URL) {
    const u = process.env.GROKHUB_URL.replace(/\/$/, "");
    if (await probe(u + "/")) return { url: u, ok: true };
  }

  const ensured = await ensureUiServer(desktopDir);
  if (await probe(ensured.url + "/")) {
    return { url: ensured.url, ok: true, started: ensured.started };
  }

  if (await probe("http://127.0.0.1:8080/")) {
    return { url: "http://127.0.0.1:8080", ok: true };
  }

  return {
    url: ensured.url,
    ok: false,
    error: ensured.error || "UI not reachable",
    root: ensured.root,
  };
}

module.exports = {
  ensureUiServer,
  resolveStartUrl,
  appRootFrom,
  pickPort,
  isOurUiPidSync,
  killOurUiPid,
  rotateDiagLog,
  runtimeDir,
  serverEntry,
};
