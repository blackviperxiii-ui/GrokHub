import { app, BrowserWindow, Tray, Menu, nativeImage, ipcMain, shell, screen } from "electron";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const host = require("./host-bridge.cjs");
const grokBridge = require("./grok-bridge.cjs");

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const isDev = !app.isPackaged;

/** @type {BrowserWindow | null} */
let mainWindow = null;
/** @type {Tray | null} */
let tray = null;

function fitToWorkArea(win) {
  const display = screen.getDisplayNearestPoint(screen.getCursorScreenPoint());
  const { x, y, width, height } = display.workArea;
  win.setBounds({
    x,
    y,
    width: Math.max(880, width),
    height: Math.max(600, height),
  });
  if (process.env.GROKHUB_MAXIMIZE !== "0") {
    win.maximize();
  }
}

function createWindow() {
  const display = screen.getPrimaryDisplay();
  const { width: aw, height: ah } = display.workAreaSize;

  mainWindow = new BrowserWindow({
    width: Math.min(1600, aw),
    height: Math.min(1000, ah),
    minWidth: 880,
    minHeight: 600,
    show: false,
    backgroundColor: "#0a0a0b",
    title: "GrokHub",
    frame: false,
    titleBarStyle: "hidden",
    webPreferences: {
      preload: path.join(__dirname, "preload.cjs"),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
      webSecurity: true,
    },
  });

  fitToWorkArea(mainWindow);

  const startUrl =
    process.env.GROKHUB_URL ||
    (isDev
      ? "http://127.0.0.1:8080"
      : `file://${path.join(__dirname, "../dist/client/index.html")}`);

  void mainWindow.loadURL(startUrl);

  mainWindow.once("ready-to-show", () => {
    fitToWorkArea(mainWindow);
    if (process.env.GROKHUB_START_MINIMIZED === "1") {
      mainWindow?.hide();
    } else {
      mainWindow?.show();
      mainWindow?.focus();
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
  ipcMain.handle("update:apply", (_e, opts) => grokBridge.applyUpdate(opts || {}));
}

if (process.env.GROKHUB_WAYLAND !== "0") {
  app.commandLine.appendSwitch("enable-features", "UseOzonePlatform,WaylandWindowDecorations");
  app.commandLine.appendSwitch("ozone-platform-hint", "auto");
}

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
