import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, shell, screen } from "electron";
import path from "node:path";
import fs from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const host = require("./host-bridge.cjs");
let grokBridge;
try {
  grokBridge = require("./grok-bridge.cjs");
  if (typeof grokBridge.factoryReinstall !== "function") {
    // Harden against partial exports from older installs
    grokBridge.factoryReinstall = async (opts = {}) =>
      grokBridge.applyUpdate({
        ...opts,
        factory: true,
        restart: opts.restart !== false,
      });
  }
} catch (e) {
  console.error("[GrokHub] failed to load grok-bridge.cjs", e);
  grokBridge = {
    checkForUpdate: async () => ({ updateAvailable: false, detail: "bridge missing" }),
    applyUpdate: async () => ({ ok: false, detail: "bridge missing", steps: [] }),
    factoryReinstall: async () => ({ ok: false, detail: "bridge missing", steps: [] }),
    checkRollback: async () => ({ ok: true, available: false }),
    applyRollback: async () => ({ ok: false, detail: "bridge missing", steps: [] }),
    postUpdateSelfTest: async () => ({ ok: false, detail: "bridge missing", checks: [] }),
    probeXaiKey: async () => ({ ok: false, detail: "bridge missing" }),
    oauthStart: async () => ({ ok: false }),
    oauthPoll: async () => ({ ok: false }),
    oauthEnsure: async () => ({ ok: false }),
    callXaiChatWithOAuth: async () => ({ ok: false }),
    callXaiChatStream: async () => ({ ok: false }),
    callXaiImagine: async () => ({ ok: false }),
    callXaiChat: async () => ({ ok: false }),
  };
}
const websiteSession = require("./website-session.cjs");
const secretsStore = require("./secrets-store.cjs");
const stateStore = require("./state-store.cjs");
const imagineStore = require("./imagine-store.cjs");
const selfMod = require("./self-mod.cjs");
const memoryStore = require("./memory-store.cjs");
const agentCore = require("./agent-core.cjs");
const desktopEntry = require("./desktop-entry.cjs");
const uiServer = require("./ui-server.cjs");
const appLog = require("./log.cjs");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = !app.isPackaged;
const agentMode =
  process.env.GROKHUB_AGENT === "1" ||
  process.argv.includes("--agent") ||
  process.argv.includes("--headless-agent");
let agentPaused = false;


/** Resolve app icon from desktop/icons or system theme paths. */
function resolveIconPath(candidates) {
  for (const p of candidates) {
    try {
      if (p && fs.existsSync(p)) return p;
    } catch {
      /* next */
    }
  }
  return null;
}

function iconCandidates(names) {
  const roots = [
    path.join(__dirname, "icons"),
    path.join(__dirname, "..", "packaging", "icons"),
    path.join(__dirname, "..", "packaging", "windows"),
    path.join(__dirname, "..", "packaging"),
    process.env.GROKHUB_HOME && path.join(process.env.GROKHUB_HOME, "icons"),
    process.env.GROKHUB_HOME && path.join(process.env.GROKHUB_HOME, "desktop", "icons"),
    process.env.LOCALAPPDATA && path.join(process.env.LOCALAPPDATA, "GrokHub", "icons"),
    "/usr/share/icons/hicolor/256x256/apps",
    "/usr/share/icons/hicolor/128x128/apps",
    "/usr/share/pixmaps",
  ].filter(Boolean);
  const out = [];
  for (const root of roots) {
    for (const name of names) {
      out.push(path.join(root, name));
    }
  }
  return out;
}

function loadAppIcon() {
  const file = resolveIconPath(
    iconCandidates([
      "icon.ico",
      "icon.png",
      "icon-512.png",
      "grokhub-256.png",
      "grokhub-128.png",
      "grokhub.png",
      "grokhub.svg",
    ]),
  );
  if (!file) return nativeImage.createEmpty();
  const img = nativeImage.createFromPath(file);
  return img.isEmpty() ? nativeImage.createEmpty() : img;
}

function loadTrayIcon() {
  const file = resolveIconPath(
    iconCandidates([
      "tray.png",
      "grokhub-32.png",
      "grokhub-48.png",
      "icon.png",
      "grokhub.png",
    ]),
  );
  if (!file) return loadAppIcon();
  let img = nativeImage.createFromPath(file);
  if (img.isEmpty()) return loadAppIcon();
  // Linux trays often want ~22–32px
  const size = img.getSize();
  if (size.width > 32) {
    img = img.resize({ width: 32, height: 32, quality: "best" });
  }
  return img;
}

/** @type {BrowserWindow | null} */
let mainWindow = null;
/** @type {Tray | null} */
let tray = null;

/**
 * Linux taskbar pin identity
 * ─────────────────────────
 * Desktop file id is `grokhub` (grokhub.desktop). GNOME/KDE Wayland match the
 * running window's app_id / WM_CLASS to that id (or StartupWMClass).
 * If they disagree (e.g. "electron" or "GrokHub" vs "grokhub"), the shell
 * shows a *second* icon when you relaunch from the pin.
 *
 * Canonical class / app_id: **grokhub** (lowercase, matches .desktop basename).
 * Visible title stays "GrokHub". userData stays under the previous path.
 */
const APP_DISPLAY_NAME = "GrokHub";
const APP_WM_CLASS = "grokhub"; // must match StartupWMClass + desktop file id
const APP_DESKTOP_FILE = "grokhub.desktop";

// Preserve existing userData directory (was set when name was "GrokHub")
try {
  const userDataKeep = path.join(app.getPath("appData"), APP_DISPLAY_NAME);
  if (fs.existsSync(userDataKeep)) {
    app.setPath("userData", userDataKeep);
  }
} catch {
  /* appData unavailable extremely early — default userData is fine */
}

// —— Identity before ready ——
// setName drives some Chromium paths; we still force --class separately.
app.setName(APP_DISPLAY_NAME);
try {
  app.setDesktopName(APP_DESKTOP_FILE);
} catch {
  /* older electron */
}
try {
  app.setAppUserModelId("com.grokhub.app");
} catch {
  /* non-windows */
}
if (process.platform === "linux") {
  // Wayland app_id / X11 WM_CLASS — lowercase matches grokhub.desktop
  app.commandLine.appendSwitch("class", APP_WM_CLASS);
  app.commandLine.appendSwitch("name", APP_WM_CLASS);
  try {
    process.title = APP_WM_CLASS;
  } catch {
    /* ignore */
  }
}

// Keep main process alive logs for stability diagnosis (never crash on stray rejections)
process.on("uncaughtException", (err) => {
  try {
    appLog.error("uncaughtException", { err: String(err?.stack || err) });
  } catch {
    console.error("[GrokHub] uncaughtException", err);
  }
});
process.on("unhandledRejection", (err) => {
  console.error("[GrokHub] unhandledRejection", err);
});

// Single instance: pin click while running focuses the existing window (no 2nd icon)
const gotLock = app.requestSingleInstanceLock();
if (!gotLock) {
  app.exit(0);
} else {
  app.on("second-instance", () => {
    const focus = () => {
      if (!mainWindow || mainWindow.isDestroyed()) {
        if (app.isReady()) createWindow();
        return;
      }
      try {
        mainWindow.setSkipTaskbar(false);
      } catch {
        /* ignore */
      }
      if (mainWindow.isMinimized()) mainWindow.restore();
      mainWindow.show();
      mainWindow.focus();
    };
    if (app.isReady()) focus();
    else void app.whenReady().then(focus);
  });
}

function windowStatePath() {
  return path.join(app.getPath("userData"), "window-state.json");
}

/**
 * @returns {{ x: number, y: number, width: number, height: number, isMaximized: boolean } | null}
 */
function loadWindowState() {
  try {
    const raw = fs.readFileSync(windowStatePath(), "utf8");
    const s = JSON.parse(raw);
    if (
      typeof s.width === "number" &&
      typeof s.height === "number" &&
      s.width >= 880 &&
      s.height >= 600
    ) {
      return {
        x: typeof s.x === "number" ? s.x : undefined,
        y: typeof s.y === "number" ? s.y : undefined,
        width: Math.round(s.width),
        height: Math.round(s.height),
        isMaximized: Boolean(s.isMaximized),
      };
    }
  } catch {
    /* first run */
  }
  return null;
}

function saveWindowState() {
  if (!mainWindow || mainWindow.isDestroyed()) return;
  try {
    const isMaximized = mainWindow.isMaximized();
    // When maximized, save the restored bounds so unmaximize returns to last free size
    const b = isMaximized ? mainWindow.getNormalBounds() : mainWindow.getBounds();
    const state = {
      x: b.x,
      y: b.y,
      width: b.width,
      height: b.height,
      isMaximized,
      displayId: screen.getDisplayMatching(b)?.id,
      savedAt: Date.now(),
    };
    fs.mkdirSync(path.dirname(windowStatePath()), { recursive: true });
    fs.writeFileSync(windowStatePath(), JSON.stringify(state, null, 2));
  } catch (e) {
    console.error("[window-state] save failed", e);
  }
}

/** Ensure saved bounds are still on a connected display (multi-monitor safe). */
function sanitizeBounds(state) {
  const displays = screen.getAllDisplays();
  if (!state || !displays.length) return null;
  const primary = screen.getPrimaryDisplay();
  const width = Math.min(
    Math.max(880, state.width || 1200),
    Math.max(880, primary.workArea.width),
  );
  const height = Math.min(
    Math.max(600, state.height || 800),
    Math.max(600, primary.workArea.height),
  );
  let x = typeof state.x === "number" ? state.x : primary.workArea.x + 40;
  let y = typeof state.y === "number" ? state.y : primary.workArea.y + 40;

  // Prefer the same display id when still connected
  let target = null;
  if (state.displayId != null) {
    target = displays.find((d) => d.id === state.displayId) || null;
  }
  // Must intersect some display work area (at least 80px visible on both axes)
  const visibleOn = (d) => {
    const wa = d.workArea;
    const overlapW = Math.min(x + width, wa.x + wa.width) - Math.max(x, wa.x);
    const overlapH = Math.min(y + height, wa.y + wa.height) - Math.max(y, wa.y);
    return overlapW > 80 && overlapH > 80;
  };
  const intersects = displays.some(visibleOn);
  if (!intersects) {
    const wa = (target || primary).workArea;
    x = wa.x + Math.max(0, Math.floor((wa.width - width) / 2));
    y = wa.y + Math.max(0, Math.floor((wa.height - height) / 2));
  } else if (target && !visibleOn(target)) {
    // Saved display exists but window is off its workArea — center on that display
    const wa = target.workArea;
    x = wa.x + Math.max(0, Math.floor((wa.width - width) / 2));
    y = wa.y + Math.max(0, Math.floor((wa.height - height) / 2));
  } else {
    // Clamp into the matching display work area so title bar stays reachable
    const match =
      displays.find(visibleOn) ||
      screen.getDisplayMatching({ x, y, width, height }) ||
      primary;
    const wa = match.workArea;
    x = Math.min(Math.max(x, wa.x), wa.x + Math.max(0, wa.width - 100));
    y = Math.min(Math.max(y, wa.y), wa.y + Math.max(0, wa.height - 80));
  }

  // Never maximize onto a missing displayId
  let isMaximized = Boolean(state.isMaximized);
  if (isMaximized && state.displayId != null && !displays.some((d) => d.id === state.displayId)) {
    isMaximized = false;
  }

  return { x, y, width, height, isMaximized, displayId: state.displayId };
}

function fitToWorkArea(win) {
  if (!win || win.isDestroyed()) return;
  const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
  const { x, y, width, height } = display.workArea;
  win.setBounds({
    x,
    y,
    width: Math.max(880, width),
    height: Math.max(600, height),
  });
  try {
    if (!win.isMaximized()) win.maximize();
  } catch {
    /* ignore */
  }
}

function createWindow() {
  const saved = sanitizeBounds(loadWindowState());
  const display = screen.getPrimaryDisplay();
  const { x: dx, y: dy, width: aw, height: ah } = display.workArea;
  const icon = loadAppIcon();

  // Prefer remembered size/position; first run fills primary work area
  const initial = saved || {
    x: dx,
    y: dy,
    width: Math.max(880, aw),
    height: Math.max(600, ah),
    isMaximized: process.env.GROKHUB_MAXIMIZE !== "0",
  };

  mainWindow = new BrowserWindow({
    x: initial.x,
    y: initial.y,
    width: initial.width,
    height: initial.height,
    minWidth: 880,
    minHeight: 600,
    show: false,
    backgroundColor: "#0a0a0b",
    title: APP_DISPLAY_NAME,
    icon: icon.isEmpty() ? undefined : icon,
    frame: false,
    titleBarStyle: "hidden",
    // Frameless + custom controls in AppShell (works on Windows/Linux)
    autoHideMenuBar: true,
    useContentSize: false,
    // Stay on the taskbar while open so the pin groups with this window
    skipTaskbar: false,
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      // UI renderer stays sandboxed; host-bridge runs as a separate privileged path
      sandbox: true,
      webSecurity: true,
    },
  });


  // Mic / media for voice chat (Grok STT)
  try {
    const ses = mainWindow.webContents.session;
    ses.setPermissionRequestHandler((_wc, permission, callback) => {
      if (permission === "media" || permission === "microphone" || permission === "mediaKeySystem") {
        callback(true);
        return;
      }
      callback(false);
    });
    ses.setPermissionCheckHandler((_wc, permission) => {
      if (permission === "media" || permission === "microphone") return true;
      return false;
    });
  } catch {
    /* ignore */
  }

  if (!icon.isEmpty()) {
    try {
      mainWindow.setIcon(icon);
    } catch {
      /* ignore */
    }
  }

  try {
    mainWindow.setTitle(APP_DISPLAY_NAME);
  } catch {
    /* ignore */
  }
  if (process.platform === "linux") {
    try {
      app.setDesktopName(APP_DESKTOP_FILE);
    } catch {
      /* ignore */
    }
  }

  // Only force full-screen fit when no saved state and maximize not disabled
  if (!saved && process.env.GROKHUB_MAXIMIZE !== "0") {
    fitToWorkArea(mainWindow);
  } else if (saved?.isMaximized) {
    try {
      mainWindow.maximize();
    } catch {
      /* ignore */
    }
  }

  // Prefer UI server URL set by resolveStartUrl / launcher (production Nitro on :18765)
  const startUrl =
    process.env.GROKHUB_URL ||
    `http://127.0.0.1:${uiServer.pickPort()}`;

  const showLoadError = (msg) => {
    const html = `<!doctype html><html><body style="font-family:system-ui;background:#0a0a0b;color:#eee;padding:2rem;line-height:1.5">
      <h1 style="margin:0 0 1rem">GrokHub UI failed to load</h1>
      <p>${String(msg || "unknown error").replace(/</g, "<")}</p>
      <p>Tried: <code>${startUrl}</code></p>
      <p style="color:#9ca3af">Fix: reinstall with a built <code>.output</code> folder<br/>
      Arch: <code>sudo ./scripts/install-arch.sh</code><br/>
      Windows: re-run the installer or <code>scripts\\install-windows.ps1</code></p>
      <p style="color:#6b7280;font-size:12px">Logs: $XDG_RUNTIME_DIR/grokhub/ui.log or %LOCALAPPDATA%\\GrokHub\\runtime\\ui.log</p>
    </body></html>`;
    void mainWindow.loadURL(
      "data:text/html;charset=utf-8," + encodeURIComponent(html),
    );
  };

  mainWindow.webContents.on("did-fail-load", (_e, code, desc, url, isMain) => {
    if (!isMain) return;
    if (code === -3) return; // aborted
    showLoadError(`${desc} (${code}) loading ${url}`);
  });

  void mainWindow.loadURL(startUrl).catch((err) => {
    showLoadError(err instanceof Error ? err.message : String(err));
  });

  let saveTimer = null;
  const scheduleSave = () => {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => saveWindowState(), 250);
  };

  mainWindow.on("resize", scheduleSave);
  mainWindow.on("move", scheduleSave);
  mainWindow.on("maximize", scheduleSave);
  mainWindow.on("unmaximize", scheduleSave);

  mainWindow.once("ready-to-show", () => {
    if (!icon.isEmpty()) {
      try {
        mainWindow?.setIcon(icon);
      } catch {
        /* ignore */
      }
    }
    try {
      mainWindow?.setTitle(APP_DISPLAY_NAME);
    } catch {
      /* ignore */
    }
    // Re-apply maximize after show (some WMs ignore pre-show maximize)
    if (saved?.isMaximized || (!saved && process.env.GROKHUB_MAXIMIZE !== "0")) {
      try {
        if (mainWindow && !mainWindow.isMaximized()) mainWindow.maximize();
      } catch {
        /* ignore */
      }
    }
    if (process.env.GROKHUB_START_MINIMIZED === "1") {
      try {
        mainWindow?.setSkipTaskbar(true);
      } catch {
        /* ignore */
      }
      mainWindow?.hide();
    } else {
      try {
        mainWindow?.setSkipTaskbar(false);
      } catch {
        /* ignore */
      }
      mainWindow?.show();
      mainWindow?.focus();
    }
    scheduleSave();
  });

  // Keep title stable (page title changes can split the taskbar entry)
  mainWindow.on("page-title-updated", (e) => {
    e.preventDefault();
    try {
      mainWindow?.setTitle(APP_DISPLAY_NAME);
    } catch {
      /* ignore */
    }
  });

  // If displays change and window is off-screen, nudge it back
  const reflow = () => {
    if (!mainWindow || mainWindow.isDestroyed() || !mainWindow.isVisible()) return;
    if (mainWindow.isMaximized()) return;
    const b = mainWindow.getBounds();
    const fixed = sanitizeBounds({ ...b, isMaximized: false });
    if (fixed && (fixed.x !== b.x || fixed.y !== b.y)) {
      mainWindow.setBounds({
        x: fixed.x,
        y: fixed.y,
        width: b.width,
        height: b.height,
      });
    }
  };
  screen.on("display-metrics-changed", reflow);
  screen.on("display-added", reflow);
  screen.on("display-removed", reflow);

  mainWindow.on("close", (e) => {
    saveWindowState();
    if (process.env.GROKHUB_TRAY !== "0" && tray) {
      e.preventDefault();
      // Drop the running window from the taskbar so only the *pin* remains.
      // On show we clear skipTaskbar so it re-groups with the same pin (same app_id).
      try {
        mainWindow?.setSkipTaskbar(true);
      } catch {
        /* ignore */
      }
      mainWindow?.hide();
    }
  });

  mainWindow.on("show", () => {
    try {
      mainWindow?.setSkipTaskbar(false);
    } catch {
      /* ignore */
    }
  });

  mainWindow.on("closed", () => {
    if (saveTimer) clearTimeout(saveTimer);
    screen.removeListener("display-metrics-changed", reflow);
    screen.removeListener("display-added", reflow);
    screen.removeListener("display-removed", reflow);
    mainWindow = null;
  });

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    void shell.openExternal(url);
    return { action: "deny" };
  });
}

function createTray() {
  if (process.env.GROKHUB_TRAY === "0") return;
  const icon = loadTrayIcon();
  // Empty tray icons crash / are invisible on some DEs
  if (icon.isEmpty()) {
    // 1x1 dark pixel fallback so Tray still constructs
    const fallback = nativeImage.createFromBuffer(
      Buffer.from(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==",
        "base64",
      ),
    );
    tray = new Tray(fallback);
  } else {
    tray = new Tray(icon);
  }
  tray.setToolTip(APP_DISPLAY_NAME);
  function rebuildTrayMenu() {
    const snap = agentCore.snapshot();
    agentPaused = Boolean(snap.paused);
    const due = (snap.due || []).length;
    tray?.setContextMenu(
      Menu.buildFromTemplate([
        {
          label: "Show GrokHub",
          click: () => {
            if (!mainWindow || mainWindow.isDestroyed()) {
              createWindow();
              return;
            }
            try {
              mainWindow.setSkipTaskbar(false);
            } catch {
              /* ignore */
            }
            mainWindow.show();
            mainWindow.focus();
          },
        },
        {
          label: "New chat",
          click: () => {
            if (!mainWindow || mainWindow.isDestroyed()) createWindow();
            mainWindow?.show();
            mainWindow?.webContents.send("agent:command", { type: "new-chat" });
          },
        },
        {
          label: due ? `Open app (${due} background jobs)` : "Open GrokHub",
          click: () => {
            if (!mainWindow || mainWindow.isDestroyed()) createWindow();
            mainWindow?.show();
            mainWindow?.webContents.send("agent:command", { type: "open-queue" });
          },
        },
        {
          label: agentPaused ? "Resume autonomy" : "Pause autonomy",
          click: () => {
            agentCore.setPaused(!agentPaused);
            agentPaused = !agentPaused;
            rebuildTrayMenu();
            mainWindow?.webContents.send("agent:command", {
              type: "set-paused",
              paused: agentPaused,
            });
          },
        },
        { type: "separator" },
        {
          label: "Quit fully",
          click: () => {
            tray?.destroy();
            tray = null;
            agentCore.stop();
            app.exit(0);
          },
        },
      ]),
    );
  }
  rebuildTrayMenu();
  // refresh tray label occasionally
  setInterval(() => {
    try {
      rebuildTrayMenu();
    } catch {
      /* ignore */
    }
  }, 30000);
  tray.on("click", () => {
    if (!mainWindow || mainWindow.isDestroyed()) {
      createWindow();
      return;
    }
    if (mainWindow.isVisible()) {
      try {
        mainWindow.setSkipTaskbar(true);
      } catch {
        /* ignore */
      }
      mainWindow.hide();
    } else {
      try {
        mainWindow.setSkipTaskbar(false);
      } catch {
        /* ignore */
      }
      mainWindow.show();
      mainWindow.focus();
    }
  });
}

function registerIpc() {
  const wrap = (fn) => {
    return async (...args) => {
      try {
        return await fn(...args);
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e);
        console.error("[host-ipc]", message);
        throw e;
      }
    };
  };

  // Avoid crash if handlers are registered twice (hot reload / double init)
  const safeHandle = (channel, listener) => {
    try {
      ipcMain.removeHandler(channel);
    } catch {
      /* ignore */
    }
    ipcMain.handle(channel, listener);
  };

  safeHandle("desktop:minimize", () => mainWindow?.minimize());
  safeHandle("desktop:maximize", () => {
    if (!mainWindow) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  safeHandle("desktop:close", () => mainWindow?.close());
  safeHandle("desktop:platform", () => process.platform);
  safeHandle("desktop:fit", () => {
    if (mainWindow) fitToWorkArea(mainWindow);
  });

  safeHandle(
    "host:info",
    wrap(() => host.info()),
  );
  safeHandle(
    "host:listDir",
    wrap((_e, p) => host.listDir(p)),
  );
  safeHandle(
    "host:readFile",
    wrap((_e, p, maxBytes) => host.readFile(p, maxBytes)),
  );
  safeHandle(
    "host:writeFile",
    wrap((_e, p, content) => host.writeFile(p, content)),
  );
  safeHandle(
    "host:exec",
    wrap((_e, command, cwd, timeoutMs, opts) => host.runExec(command, cwd, timeoutMs, opts || {})),
  );
  safeHandle("host:killExec", (_e, jobId) => host.killExec(jobId));
  safeHandle("host:setSafeMode", (_e, enabled) => host.setSafeMode(Boolean(enabled)));
  safeHandle("host:getSafeMode", () => host.getSafeMode());
  safeHandle(
    "host:listApps",
    wrap(() => host.listApps()),
  );
  safeHandle(
    "host:openApp",
    wrap((_e, opts) => host.openApp(opts || {})),
  );
  safeHandle(
    "host:readOpenClawWorkspace",
    wrap((_e, p) => host.readOpenClawWorkspace(p)),
  );

  safeHandle("grok:chat", (_e, payload) => grokBridge.callXaiChat(payload || {}));
  safeHandle("grok:imagine", (_e, payload) => grokBridge.callXaiImagine(payload || {}));
  safeHandle("grok:transcribe", (_e, payload) => grokBridge.callXaiStt(payload || {}));
  safeHandle("imagine:save", (_e, jobId, dataUrl, kind) =>
    imagineStore.saveMedia(jobId, dataUrl, kind || "image"),
  );
  safeHandle("imagine:load", (_e, relPath) => imagineStore.loadMedia(relPath));
  safeHandle("imagine:delete", (_e, jobId) => imagineStore.deleteJobMedia(jobId));
  safeHandle("imagine:clear", () => imagineStore.clearAll());
  safeHandle("grok:probe", async (_e, apiKey, accessToken) => {
    const bearer =
      (accessToken && String(accessToken)) ||
      (apiKey && String(apiKey)) ||
      process.env.XAI_API_KEY ||
      process.env.GROK_API_KEY ||
      "";
    const r = await grokBridge.probeXaiKey(bearer);
    return {
      ...r,
      envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
      authMode: accessToken ? "oauth" : apiKey ? "apiKey" : "env",
    };
  });
  safeHandle("grok:oauthStart", () => grokBridge.oauthStart());
  safeHandle("grok:oauthPoll", (_e, deviceCode) => grokBridge.oauthPoll(deviceCode));
  safeHandle("grok:oauthEnsure", (_e, tokens) => grokBridge.oauthEnsure(tokens));
  safeHandle("update:check", (_e, opts) => grokBridge.checkForUpdate(opts || {}));
  safeHandle("update:apply", async (_e, opts) => {
    const r = await grokBridge.applyUpdate({ ...(opts || {}), restart: true });
    if (r?.ok && r?.restarting) {
      // Give IPC time to return result to renderer; restart script waits ~2.8s after exit
      setTimeout(() => {
        try {
          tray?.destroy();
        } catch {
          /* ignore */
        }
        try {
          // Hide window so user doesn't see a half-dead shell while files swap finishes
          if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.hide();
          }
        } catch {
          /* ignore */
        }
        app.exit(0);
      }, 1600);
    }
    return r;
  });
  safeHandle("update:checkRollback", (_e, opts) => grokBridge.checkRollback(opts || {}));
  safeHandle("update:rollback", async (_e, opts) => {
    const r = await grokBridge.applyRollback({ ...(opts || {}), restart: true });
    if (r?.ok && r?.restarting) {
      setTimeout(() => {
        try {
          tray?.destroy();
        } catch {
          /* ignore */
        }
        try {
          if (mainWindow && !mainWindow.isDestroyed()) mainWindow.hide();
        } catch {
          /* ignore */
        }
        app.exit(0);
      }, 1600);
    }
    return r;
  });
  safeHandle("update:selfTest", (_e, opts) => grokBridge.postUpdateSelfTest(opts || {}));

  /** Capture grok.com SSO cookie (website Usage / weekly SuperGrok limit). */
  safeHandle("grok:getWebsiteSso", async () => {
    try {
      return await websiteSession.getStoredSso();
    } catch (e) {
      return { cookie: "", error: e instanceof Error ? e.message : "cookie read failed" };
    }
  });

  safeHandle("grok:linkWebsiteSession", async () => {
    try {
      const r = await websiteSession.linkWebsiteSession();
      // Persist only verified cookies into secrets when present
      try {
        const cookie = r?.cookie || r?.cookieHeader || "";
        if (cookie) secretsStore.set("ssoCookie", cookie);
      } catch {
        /* ignore */
      }
      return r;
    } catch (e) {
      return { error: e instanceof Error ? e.message : "link failed" };
    }
  });

  safeHandle("grok:injectWebsiteCookie", async (_e, raw) => {
    try {
      const r = await websiteSession.injectCookieHeader(String(raw || ""));
      if (r?.ok && (r.cookie || r.cookieHeader || raw)) {
        try {
          secretsStore.set("ssoCookie", String(r.cookie || r.cookieHeader || raw));
        } catch {
          /* ignore */
        }
      }
      return r;
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "inject failed" };
    }
  });

  safeHandle("grok:websiteUsage", async (_e, opts) => {
    let sso = String(opts?.ssoCookie || "").trim();
    if (!sso) {
      try {
        const sec = secretsStore.get("ssoCookie");
        sso = String(sec?.value || "").trim();
      } catch {
        /* ignore */
      }
    }
    return websiteSession.fetchWebsiteUsage({
      ssoCookie: sso,
      bearer: String(opts?.bearer || ""),
    });
  });

  safeHandle("grok:websiteConnectors", async (_e, opts) => {
    return websiteSession.fetchWebsiteConnectors({
      ssoCookie: String(opts?.ssoCookie || ""),
      bearer: String(opts?.bearer || ""),
    });
  });

  safeHandle("secrets:set", (_e, key, value) => secretsStore.set(String(key), String(value ?? "")));
  safeHandle("secrets:get", (_e, key) => secretsStore.get(String(key)));
  safeHandle("secrets:delete", (_e, key) => secretsStore.del(String(key)));

  safeHandle("state:get", (_e, name) => stateStore.get(String(name || "")));
  safeHandle("state:set", (_e, name, value) =>
    stateStore.set(String(name || ""), value == null ? "" : String(value)),
  );
  safeHandle("state:remove", (_e, name) => stateStore.remove(String(name || "")));
  safeHandle("state:info", () => stateStore.info());
  safeHandle("state:export", () => stateStore.exportAll());
  

  // Always-on agent core
  safeHandle("agent:snapshot", () => agentCore.snapshot());
  safeHandle("agent:enqueue", (_e, job) => agentCore.enqueue(job));
  safeHandle("agent:setPaused", (_e, v) => {
    const s = agentCore.setPaused(v);
    return s;
  });
  safeHandle("agent:approve", (_e, id, grant) => agentCore.approve(id, grant));
  safeHandle("agent:sync", (_e, payload) => agentCore.sync(payload || {}));

  safeHandle("state:import", (_e, payload) => stateStore.importAll(payload));

  safeHandle("memory:info", () => memoryStore.info());
  safeHandle("memory:list", () => ({ ok: true, files: memoryStore.listFiles(), root: memoryStore.memoryRoot() }));
  safeHandle("memory:read", (_e, rel) => memoryStore.read(rel));
  safeHandle("memory:write", (_e, rel, content) => memoryStore.write(rel, content));
  safeHandle("memory:append", (_e, rel, text, opts) => memoryStore.append(rel, text, opts || {}));
  safeHandle("memory:appendFacts", (_e, facts, opts) =>
    memoryStore.appendFacts(facts, opts || {}),
  );
  safeHandle("memory:pinBundle", (_e, opts) => memoryStore.buildPinBundle(opts || {}));
  safeHandle("memory:syncLearning", (_e, payload) => memoryStore.syncLearning(payload || {}));
  safeHandle("memory:ensure", () => {
    const r = memoryStore.ensureLayout();
    return { ok: true, ...r };
  });

  safeHandle("selfmod:info", () => selfMod.info());
  safeHandle("selfmod:list", (_e, rel) => selfMod.listDirRel(rel));
  safeHandle("selfmod:read", (_e, rel) => selfMod.readFileRel(rel));
  safeHandle("selfmod:write", (_e, rel, content, opts) =>
    selfMod.writeFileRel(rel, content, opts || {}),
  );
  safeHandle("selfmod:patch", (_e, rel, find, replace, opts) =>
    selfMod.patchFileRel(rel, find, replace, opts || {}),
  );
  safeHandle("selfmod:snapshot", (_e, note) => selfMod.createSnapshot(note));
  safeHandle("selfmod:restore", (_e, id) => selfMod.restoreSnapshot(id));
  safeHandle("selfmod:journal", (_e, limit) => selfMod.listJournal(limit));
  safeHandle("update:factory", async (_e, opts) => {
    const r = await grokBridge.factoryReinstall({ ...(opts || {}), restart: true });
    return r;
  });

  safeHandle("desktopEntry:status", () => desktopEntry.status());
  safeHandle("desktopEntry:install", (_e, opts) => desktopEntry.installMenuEntry(opts || {}));
  safeHandle("desktopEntry:autostart", (_e, enabled) =>
    desktopEntry.installAutostart(Boolean(enabled)),
  );

  // Non-stream chat already registered; true streaming with abort
  if (typeof grokBridge.callXaiChatStream === "function") {
    /** @type {Map<string, AbortController>} */
    const streamAborts = new Map();

    safeHandle("grok:chatStreamAbort", (_e, streamId) => {
      const id = streamId != null ? String(streamId) : "";
      if (id && streamAborts.has(id)) {
        try {
          streamAborts.get(id).abort();
        } catch {
          /* ignore */
        }
        streamAborts.delete(id);
        return { ok: true, streamId: id };
      }
      // Abort all in-flight streams
      for (const [sid, ac] of streamAborts) {
        try {
          ac.abort();
        } catch {
          /* ignore */
        }
        streamAborts.delete(sid);
      }
      return { ok: true, all: true };
    });

    safeHandle("grok:chatStream", async (e, payload) => {
      const streamId =
        String(payload?.streamId || "") ||
        `s-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
      const ac = new AbortController();
      streamAborts.set(streamId, ac);
      const sender = e.sender;
      const sendDelta = (d) => {
        try {
          if (!sender.isDestroyed()) {
            sender.send("grok:chatStream:delta", { streamId, delta: d });
          }
        } catch {
          /* ignore */
        }
      };
      const sendStatus = (st) => {
        try {
          if (!sender.isDestroyed()) {
            sender.send("grok:chatStream:status", { streamId, status: st });
          }
        } catch {
          /* ignore */
        }
      };
      try {
        const result = await grokBridge.callXaiChatStream(payload || {}, {
          signal: ac.signal,
          onDelta: (d) => sendDelta(d),
          onStatus: (st) => sendStatus(st),
        });
        return { ...result, streamId, content: result.content || "" };
      } finally {
        streamAborts.delete(streamId);
      }
    });
  }
}


// Lower main-process priority slightly so long tool loops don't starve the desktop.
// Opt out with GROKHUB_HIGH_PRIORITY=1. GPU: GROKHUB_DISABLE_GPU=1 disables HW accel.
if (process.platform === "linux" && process.env.GROKHUB_HIGH_PRIORITY !== "1") {
  try {
    const { execFile } = require("node:child_process");
    execFile("renice", ["+5", "-p", String(process.pid)], () => {});
  } catch {
    /* ignore */
  }
}
if (process.env.GROKHUB_DISABLE_GPU === "1") {
  app.disableHardwareAcceleration();
}

if (process.platform === "linux" && process.env.GROKHUB_WAYLAND !== "0") {
  app.commandLine.appendSwitch("enable-features", "UseOzonePlatform,WaylandWindowDecorations");
  app.commandLine.appendSwitch("ozone-platform-hint", "auto");
}

// Linux: system Electron often lacks setuid chrome-sandbox.
// Default: no-sandbox (compat). Opt into sandbox with GROKHUB_SANDBOX=1.
// Host bridge stays unsandboxed either way — this only affects Chromium UI.
if (process.platform === "linux" && process.env.GROKHUB_SANDBOX !== "1") {
  app.commandLine.appendSwitch("no-sandbox");
  try {
    appLog?.warn?.("renderer no-sandbox (set GROKHUB_SANDBOX=1 to try sandbox)");
  } catch {
    /* log not ready */
  }
}


// Clean exit: stop Nitro UI we own (pidfile/lock). Never fuser -k.
// Set GROKHUB_KEEP_UI=1 to leave the backend running for multi-instance.
app.on("will-quit", () => {
  try {
    appLog.info("will-quit");
  } catch {
    /* ignore */
  }
  if (process.env.GROKHUB_KEEP_UI === "1") return;
  try {
    const fsSync = require("node:fs");
    const pathSync = require("node:path");
    const rt = pathSync.join(process.env.XDG_RUNTIME_DIR || "/tmp", "grokhub");
    const candidates = [];
    if (process.env.GROKHUB_UI_PID) {
      candidates.push(Number(process.env.GROKHUB_UI_PID));
    }
    for (const name of ["ui.pid", "ui.lock"]) {
      const f = pathSync.join(rt, name);
      if (!fsSync.existsSync(f)) continue;
      try {
        candidates.push(Number(String(fsSync.readFileSync(f, "utf8")).trim()));
      } catch {
        /* ignore */
      }
    }
    const seen = new Set();
    for (const pid of candidates) {
      if (!pid || pid <= 1 || seen.has(pid)) continue;
      seen.add(pid);
      try {
        if (!uiServer.isOurUiPidSync?.(pid)) continue;
        process.kill(pid, "SIGTERM");
        appLog.info("stopped UI on quit", { pid });
      } catch {
        /* ignore */
      }
    }
    for (const name of ["ui.pid", "ui.lock"]) {
      try {
        fsSync.unlinkSync(pathSync.join(rt, name));
      } catch {
        /* ignore */
      }
    }
  } catch {
    /* ignore */
  }
});

app.whenReady().then(async () => {
  if (!gotLock) return;
  try {
    appLog.info("boot", {
      version: process.env.npm_package_version || undefined,
      electron: process.versions.electron,
      platform: process.platform,
      home: process.env.GROKHUB_HOME || null,
    });
  } catch {
    /* ignore */
  }
  // Create memory layout on every boot so HOST_CMD scans find real files
  try {
    const layout = memoryStore.ensureLayout();
    appLog.info("memory-layout", layout);
  } catch (e) {
    try {
      appLog.warn?.("memory-layout failed", String(e?.message || e));
    } catch {
      /* ignore */
    }
  }
  // Install root first — UI must never resolve .output relative to $HOME
  try {
    const root = uiServer.appRootFrom(__dirname);
    if (root) process.env.GROKHUB_HOME = root;
  } catch {
    /* ignore */
  }

  // Prefer home as process cwd so relative shell paths match a real desktop session
  // (UI server always uses absolute entry + cwd=GROKHUB_HOME — chdir is safe)
  try {
    const home =
      process.env.HOME || process.env.USERPROFILE || require("node:os").homedir();
    if (home) process.chdir(home);
  } catch {
    /* ignore */
  }

  // Ensure Nitro UI is up before opening the window
  try {
    const resolved = await uiServer.resolveStartUrl(__dirname);
    if (resolved.url) process.env.GROKHUB_URL = resolved.url;
    if (!resolved.ok && resolved.error) {
      console.error("[GrokHub]", resolved.error);
    }
  } catch (e) {
    console.error("[GrokHub] UI bootstrap failed", e);
  }

  // Dock / taskbar name + pin identity
  try {
    app.setName(APP_DISPLAY_NAME);
  } catch {
    /* ignore */
  }
  if (process.platform === "linux") {
    try {
      app.setDesktopName(APP_DESKTOP_FILE);
    } catch {
      /* ignore */
    }
  }
  if (process.platform === "win32") {
    try {
      app.setAppUserModelId("com.grokhub.app");
    } catch {
      /* ignore */
    }
  }
  // Best-effort Start Menu / app menu entry (Linux .desktop or Windows .lnk)
  if (process.platform === "linux" || process.platform === "win32") {
    try {
      desktopEntry.installMenuEntry();
    } catch {
      /* ignore */
    }
  }
  registerIpc();
  try {
    agentCore.start({
      intervalMs: 15000,
      onDueJobs: (payload) => {
        try {
          if (mainWindow && !mainWindow.isDestroyed()) {
            mainWindow.webContents.send("agent:due", payload);
          }
        } catch {
          /* ignore */
        }
      },
    });
  } catch (e) {
    console.error("[GrokHub] agent-core start failed", e);
  }
  try {
    if (typeof websiteSession.hydrateWebsiteSession === "function") {
      const h = await websiteSession.hydrateWebsiteSession();
      try {
        appLog.info?.(
          "usage",
          `hydrate ${JSON.stringify({ ok: h?.ok, signedIn: h?.signedIn, fromSecrets: h?.fromSecrets })}`,
        );
      } catch {
        /* ignore */
      }
    }
  } catch (e) {
    console.error("[GrokHub] website session hydrate failed", e);
  }
  createWindow();
  createTray();
  if (agentMode) {
    // Headless / always-on: keep tray, hide main window after create
    try {
      mainWindow?.hide();
      mainWindow?.setSkipTaskbar(true);
    } catch {
      /* ignore */
    }
    console.log("[GrokHub] agent mode — window hidden, tray/queue active");
  }
  app.on("activate", () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
    else mainWindow?.show();
  });
});

app.on("window-all-closed", () => {
  if (process.platform !== "darwin") {
    if (!tray) app.quit();
  }
});
