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

function fitToWorkArea(win) {
  if (!win || win.isDestroyed()) return;
  const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
  const { x, y, width, height } = display.workArea;
  // Fill the full work area (ultrawide / multi-monitor safe)
  win.setBounds({
    x,
    y,
    width: Math.max(880, width),
    height: Math.max(600, height),
  });
  // Maximize so WM decorations / tiling still treat us as fullscreened window
  if (process.env.GROKHUB_MAXIMIZE !== "0") {
    try {
      if (!win.isMaximized()) win.maximize();
    } catch {
      /* ignore */
    }
  }
}

function createWindow() {
  const display = screen.getPrimaryDisplay();
  const { x, y, width: aw, height: ah } = display.workArea;
  const icon = loadAppIcon();

  mainWindow = new BrowserWindow({
    x,
    y,
    width: Math.max(880, aw),
    height: Math.max(600, ah),
    minWidth: 880,
    minHeight: 600,
    show: false,
    backgroundColor: "#0a0a0b",
    title: "GrokHub",
    icon: icon.isEmpty() ? undefined : icon,
    frame: false,
    titleBarStyle: "hidden",
    autoHideMenuBar: true,
    // Fill the monitor; content is CSS-fluid for ultrawide
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

  fitToWorkArea(mainWindow);

  const startUrl =
    process.env.GROKHUB_URL ||
    (isDev
      ? "http://127.0.0.1:8080"
      : `file://${path.join(__dirname, "../dist/client/index.html")}`);

  void mainWindow.loadURL(startUrl);

  mainWindow.once("ready-to-show", () => {
    fitToWorkArea(mainWindow);
    if (!icon.isEmpty()) {
      try {
        mainWindow?.setIcon(icon);
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
  });

  // Keep filling the active display when monitors change (dock, resolution, ultrawide switch)
  const reflow = () => {
    if (mainWindow && !mainWindow.isDestroyed() && mainWindow.isVisible()) {
      // Only re-fit when maximized / full work-area mode
      if (mainWindow.isMaximized() || process.env.GROKHUB_MAXIMIZE !== "0") {
        fitToWorkArea(mainWindow);
      }
    }
  };
  screen.on("display-metrics-changed", reflow);
  screen.on("display-added", reflow);
  screen.on("display-removed", reflow);
  mainWindow.on("closed", () => {
    screen.removeListener("display-metrics-changed", reflow);
    screen.removeListener("display-added", reflow);
    screen.removeListener("display-removed", reflow);
  });

  mainWindow.on("close", (e) => {
    if (process.env.GROKHUB_TRAY !== "0" && tray) {
      e.preventDefault();
      mainWindow?.hide();
    }
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

  ipcMain.handle("host:info", () => host.info());
  ipcMain.handle("host:listDir", (_e, p) => host.listDir(p));
  ipcMain.handle("host:readFile", (_e, p, maxBytes) => host.readFile(p, maxBytes));
  ipcMain.handle("host:writeFile", (_e, p, content) => host.writeFile(p, content));
  ipcMain.handle("host:exec", (_e, command, cwd, timeoutMs) =>
    host.runExec(command, cwd, timeoutMs),
  );
  ipcMain.handle("host:listApps", () => host.listApps());
  ipcMain.handle("host:openApp", (_e, opts) => host.openApp(opts || {}));

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
      // Give the renderer a moment to show "Restarting…" then quit this process.
      // scheduleAppRestart already spawned the replacement launcher.
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
