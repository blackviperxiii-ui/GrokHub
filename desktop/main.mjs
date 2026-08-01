import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, shell } from "electron";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const host = require("./host-bridge.cjs");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = !app.isPackaged;

/** @type {BrowserWindow | null} */
let mainWindow = null;
/** @type {Tray | null} */
let tray = null;

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1320,
    height: 860,
    minWidth: 920,
    minHeight: 640,
    show: false,
    backgroundColor: "#0a0a0b",
    title: "GrokHub",
    frame: false,
    titleBarStyle: "hidden",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      // Intentionally off: personal agent needs host FS/shell/apps
      sandbox: false,
      webSecurity: true,
    },
  });

  const startUrl =
    process.env.GROKHUB_URL ||
    (isDev
      ? "http://127.0.0.1:8080"
      : `file://${path.join(__dirname, "../dist/client/index.html")}`);

  void mainWindow.loadURL(startUrl);

  mainWindow.once("ready-to-show", () => {
    if (process.env.GROKHUB_START_MINIMIZED === "1") {
      mainWindow?.hide();
    } else {
      mainWindow?.show();
    }
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
  const icon = nativeImage.createEmpty();
  tray = new Tray(icon);
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

  // Unsandboxed host bridge
  ipcMain.handle("host:info", () => host.info());
  ipcMain.handle("host:listDir", (_e, p) => host.listDir(p));
  ipcMain.handle("host:readFile", (_e, p, maxBytes) => host.readFile(p, maxBytes));
  ipcMain.handle("host:writeFile", (_e, p, content) => host.writeFile(p, content));
  ipcMain.handle("host:exec", (_e, command, cwd, timeoutMs) =>
    host.runExec(command, cwd, timeoutMs),
  );
  ipcMain.handle("host:listApps", () => host.listApps());
  ipcMain.handle("host:openApp", (_e, opts) => host.openApp(opts || {}));
}

// Wayland-friendly Chromium flags (Arch / Hyprland / KDE)
if (process.env.GROKHUB_WAYLAND !== "0") {
  app.commandLine.appendSwitch("enable-features", "UseOzonePlatform,WaylandWindowDecorations");
  app.commandLine.appendSwitch("ozone-platform-hint", "auto");
}

// Do not enable Chromium sandbox for host agent workloads
app.commandLine.appendSwitch("no-sandbox");

app.whenReady().then(() => {
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
