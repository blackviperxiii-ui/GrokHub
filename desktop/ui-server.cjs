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
  // 3) desktop/ sibling of .output
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
function nodeSpawnSpec(entry) {
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
  return { bin: process.execPath, args: [entry], envExtra: {} };
}

/**
 * @param {string} desktopDir absolute path to desktop/ (usually __dirname)
 */
async function ensureUiServer(desktopDir) {
  const root = appRootFrom(desktopDir);
  const port = pickPort();
  const url = (process.env.GROKHUB_URL || `http://127.0.0.1:${port}`).replace(
    /\/$/,
    "",
  );

  if (await probe(url + "/")) {
    process.env.GROKHUB_URL = url;
    return { url, started: false, root };
  }

  const entry = serverEntry(root);
  if (!fs.existsSync(entry)) {
    return {
      url,
      started: false,
      root,
      error: `UI build missing: ${entry}`,
    };
  }

  const rt = runtimeDir();
  try {
    fs.mkdirSync(rt, { recursive: true });
  } catch {
    /* ignore */
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
    fs.appendFileSync(
      logPath,
      `\n[ui-server] spawn ${spec.bin} ${spec.args.join(" ")} cwd=${root}\n`,
    );
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
};
