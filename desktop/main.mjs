import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, shell, screen } from "electron";
import path from "node:path";
import fs from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const host = require("./host-bridge.cjs");
const grokBridge = require("./grok-bridge.cjs");

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
    path.join(__dirname, "..", "packaging"),
    "/usr/share/icons/hicolor/256x256/apps",
    "/usr/share/icons/hicolor/128x128/apps",
    "/usr/share/pixmaps",
  ];
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

// Must run before app ready — sets WM_CLASS / taskbar identity on Linux
app.setName("GrokHub");
if (process.platform === "linux") {
  app.commandLine.appendSwitch("class", "GrokHub");
}
try {
  app.setAppUserModelId("com.grokhub.app");
} catch {
  /* non-windows */
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
    title: "GrokHub",
    icon: icon.isEmpty() ? undefined : icon,
    frame: false,
    titleBarStyle: "hidden",
    autoHideMenuBar: true,
    useContentSize: false,
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

  const startUrl =
    process.env.GROKHUB_URL ||
    (isDev
      ? "http://127.0.0.1:8080"
      : `file://${path.join(__dirname, "../dist/client/index.html")}`);

  void mainWindow.loadURL(startUrl);

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
    // Re-apply maximize after show (some WMs ignore pre-show maximize)
    if (saved?.isMaximized || (!saved && process.env.GROKHUB_MAXIMIZE !== "0")) {
      try {
        if (mainWindow && !mainWindow.isMaximized()) mainWindow.maximize();
      } catch {
        /* ignore */
      }
    }
    if (process.env.GROKHUB_START_MINIMIZED === "1") {
      mainWindow?.hide();
    } else {
      mainWindow?.show();
      mainWindow?.focus();
    }
    scheduleSave();
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
      mainWindow?.hide();
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
  tray.setToolTip("GrokHub");
  tray.setContextMenu(
    Menu.buildFromTemplate([
      {
        label: "Show GrokHub",
        click: () => {
          mainWindow?.show();
          mainWindow?.focus();
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
    if (!mainWindow) return;
    if (mainWindow.isVisible()) mainWindow.hide();
    else {
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

  ipcMain.handle("desktop:minimize", () => mainWindow?.minimize());
  ipcMain.handle("desktop:maximize", () => {
    if (!mainWindow) return;
    if (mainWindow.isMaximized()) mainWindow.unmaximize();
    else mainWindow.maximize();
  });
  ipcMain.handle("desktop:close", () => mainWindow?.close());
  ipcMain.handle("desktop:platform", () => process.platform);
  ipcMain.handle("desktop:fit", () => {
    if (mainWindow) fitToWorkArea(mainWindow);
  });

  ipcMain.handle(
    "host:info",
    wrap(() => host.info()),
  );
  ipcMain.handle(
    "host:listDir",
    wrap((_e, p) => host.listDir(p)),
  );
  ipcMain.handle(
    "host:readFile",
    wrap((_e, p, maxBytes) => host.readFile(p, maxBytes)),
  );
  ipcMain.handle(
    "host:writeFile",
    wrap((_e, p, content) => host.writeFile(p, content)),
  );
  ipcMain.handle(
    "host:exec",
    wrap((_e, command, cwd, timeoutMs) => host.runExec(command, cwd, timeoutMs)),
  );
  ipcMain.handle(
    "host:listApps",
    wrap(() => host.listApps()),
  );
  ipcMain.handle(
    "host:openApp",
    wrap((_e, opts) => host.openApp(opts || {})),
  );

  ipcMain.handle("grok:chat", (_e, payload) => grokBridge.callXaiChat(payload || {}));
  ipcMain.handle("grok:probe", async (_e, apiKey, accessToken) => {
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
  ipcMain.handle("grok:oauthStart", () => grokBridge.oauthStart());
  ipcMain.handle("grok:oauthPoll", (_e, deviceCode) => grokBridge.oauthPoll(deviceCode));
  ipcMain.handle("grok:oauthEnsure", (_e, tokens) => grokBridge.oauthEnsure(tokens));
  ipcMain.handle("update:check", (_e, opts) => grokBridge.checkForUpdate(opts || {}));
  ipcMain.handle("update:apply", async (_e, opts) => {
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
}

if (process.env.GROKHUB_WAYLAND !== "0") {
  app.commandLine.appendSwitch("enable-features", "UseOzonePlatform,WaylandWindowDecorations");
  app.commandLine.appendSwitch("ozone-platform-hint", "auto");
}

app.commandLine.appendSwitch("no-sandbox");

app.whenReady().then(() => {
  // Prefer home as process cwd so relative shell paths match a real desktop session
  try {
    const home = process.env.HOME || require("node:os").homedir();
    if (home) process.chdir(home);
  } catch {
    /* ignore */
  }
  // Dock / taskbar name
  if (process.platform === "linux") {
    try {
      app.setName("GrokHub");
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
