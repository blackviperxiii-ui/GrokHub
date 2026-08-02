import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, shell, screen } from "electron";
import path from "node:path";
import fs from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const host = require("./host-bridge.cjs");
const grokBridge = require("./grok-bridge.cjs");
const websiteSession = require("./website-session.cjs");
const secretsStore = require("./secrets-store.cjs");
const stateStore = require("./state-store.cjs");
const selfMod = require("./self-mod.cjs");
const desktopEntry = require("./desktop-entry.cjs");
const uiServer = require("./ui-server.cjs");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = !app.isPackaged;

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

/** Ensure saved bounds are still on a connected display */
function sanitizeBounds(state) {
  const displays = screen.getAllDisplays();
  if (!state || !displays.length) return null;
  const width = Math.max(880, state.width || 1200);
  const height = Math.max(600, state.height || 800);
  let x = typeof state.x === "number" ? state.x : displays[0].workArea.x + 40;
  let y = typeof state.y === "number" ? state.y : displays[0].workArea.y + 40;

  // Must intersect some display work area (at least 100px visible)
  const intersects = displays.some((d) => {
    const wa = d.workArea;
    const overlapW =
      Math.min(x + width, wa.x + wa.width) - Math.max(x, wa.x);
    const overlapH =
      Math.min(y + height, wa.y + wa.height) - Math.max(y, wa.y);
    return overlapW > 100 && overlapH > 100;
  });
  if (!intersects) {
    const wa = screen.getPrimaryDisplay().workArea;
    x = wa.x + Math.max(0, Math.floor((wa.width - width) / 2));
    y = wa.y + Math.max(0, Math.floor((wa.height - height) / 2));
  }
  return { x, y, width, height, isMaximized: Boolean(state.isMaximized) };
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
      sandbox: false,
      webSecurity: true,
    },
  });

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

  void mainWindow.loadURL(startUrl).catch((err) => {
    const msg = err instanceof Error ? err.message : String(err);
    void mainWindow.loadURL(
      "data:text/html;charset=utf-8," +
        encodeURIComponent(
          `<!doctype html><html><body style="font-family:system-ui;background:#0a0a0b;color:#eee;padding:2rem">
          <h1>GrokHub UI failed to load</h1>
          <p>${msg}</p>
          <p>Tried: <code>${startUrl}</code></p>
          <p>Ensure the app was installed with a built <code>.output</code> folder, or run <code>npm run desktop:build</code>.</p>
          </body></html>`,
        ),
    );
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
  tray.setContextMenu(
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
        label: "Quit",
        click: () => {
          tray?.destroy();
          tray = null;
          app.exit(0);
        },
      },
    ]),
  );
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
    wrap((_e, command, cwd, timeoutMs) => host.runExec(command, cwd, timeoutMs)),
  );
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
      setTimeout(() => {
        try {
          tray?.destroy();
        } catch {
          /* ignore */
        }
        app.exit(0);
      }, 900);
    }
    return r;
  });

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
      return await websiteSession.linkWebsiteSession();
    } catch (e) {
      return { error: e instanceof Error ? e.message : "link failed" };
    }
  });

  safeHandle("grok:injectWebsiteCookie", async (_e, raw) => {
    try {
      return await websiteSession.injectCookieHeader(String(raw || ""));
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "inject failed" };
    }
  });

  safeHandle("grok:websiteUsage", async (_e, opts) => {
    return websiteSession.fetchWebsiteUsage({
      ssoCookie: String(opts?.ssoCookie || ""),
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
  safeHandle("state:import", (_e, payload) => stateStore.importAll(payload));

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

  // Non-stream chat already registered; expose stream buffer helper if bridge supports it
  if (typeof grokBridge.callXaiChatStream === "function") {
    safeHandle("grok:chatStream", async (_e, payload) => {
      let content = "";
      const result = await grokBridge.callXaiChatStream(payload || {}, {
        onDelta: (d) => {
          content += d;
        },
      });
      return { ...result, content: result.content || content };
    });
  }
}

if (process.platform === "linux" && process.env.GROKHUB_WAYLAND !== "0") {
  app.commandLine.appendSwitch("enable-features", "UseOzonePlatform,WaylandWindowDecorations");
  app.commandLine.appendSwitch("ozone-platform-hint", "auto");
}

// Linux system electron often needs this; Windows packaged Electron does not.
if (process.platform === "linux") {
  app.commandLine.appendSwitch("no-sandbox");
}

app.whenReady().then(async () => {
  if (!gotLock) return;
  // Prefer home as process cwd so relative shell paths match a real desktop session
  try {
    const home =
      process.env.HOME || process.env.USERPROFILE || require("node:os").homedir();
    if (home) process.chdir(home);
  } catch {
    /* ignore */
  }

  // Ensure Nitro UI is up before opening the window (fixes Windows install)
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
  createWindow();
  createTray();
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
