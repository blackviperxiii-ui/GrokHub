/**
 * Start / wait for the GrokHub Nitro UI (.output/server) next to desktop/.
 * Used on Windows and Linux so Electron always has a URL even if the shell
 * launcher failed to start the server.
 */
const { spawn } = require("node:child_process");
const fs = require("node:fs");
const http = require("node:http");
const path = require("node:path");
const os = require("node:os");

function appRootFrom(desktopDir) {
  if (process.env.GROKHUB_HOME && fs.existsSync(process.env.GROKHUB_HOME)) {
    return path.resolve(process.env.GROKHUB_HOME);
  }
  // desktop/ is inside app root
  return path.resolve(desktopDir, "..");
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
 * @param {string} desktopDir absolute path to desktop/ (usually __dirname)
 * @returns {Promise<{ url: string, started: boolean, root: string, error?: string }>}
 */
async function ensureUiServer(desktopDir) {
  const root = appRootFrom(desktopDir);
  const port = pickPort();
  const url = (process.env.GROKHUB_URL || `http://127.0.0.1:${port}`).replace(
    /\/$/,
    "",
  );

  // Explicit URL already up?
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
      error: `UI build missing: ${entry}. Run npm run desktop:build or reinstall.`,
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

  const env = {
    ...process.env,
    PORT: String(port),
    NITRO_PORT: String(port),
    HOST: "127.0.0.1",
    NITRO_HOST: "127.0.0.1",
    GROKHUB_HOME: root,
  };

  // Always use `node` for Nitro (never electron.exe)
  const nodeBin =
    process.platform === "win32"
      ? "node.exe"
      : process.execPath.toLowerCase().includes("electron")
        ? "node"
        : process.execPath;

  const child = spawn(nodeBin, [entry], {
    cwd: root,
    env,
    detached: true,
    stdio: logFd === "ignore" ? "ignore" : ["ignore", logFd, logFd],
    windowsHide: true,
    shell: process.platform === "win32", // resolve node.exe on PATH
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

  const deadline = Date.now() + 25_000;
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

/**
 * Resolve which URL the BrowserWindow should load.
 */
async function resolveStartUrl(desktopDir) {
  if (process.env.GROKHUB_URL) {
    const u = process.env.GROKHUB_URL.replace(/\/$/, "");
    if (await probe(u + "/")) return { url: u, ok: true };
  }

  const ensured = await ensureUiServer(desktopDir);
  if (await probe(ensured.url + "/")) {
    return { url: ensured.url, ok: true, started: ensured.started };
  }

  // Dev fallback
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
