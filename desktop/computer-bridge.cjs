/**
 * Linux desktop computer-use: screenshot (Electron desktopCapturer) +
 * mouse/keyboard via xdotool (X11/XWayland) or ydotool (Wayland).
 * Safe to require without Electron — screenshot/input then return errors.
 */
const { execFile, spawn } = require("node:child_process");
const { promisify } = require("node:util");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");

const execFileAsync = promisify(execFile);

const SCREENSHOT_MAX_WIDTH = 1280;
const MIN_ACTION_DELAY_MS = 80;
const MAX_TYPE_CHARS = 4000;

/** @type {import('electron').BrowserWindow | null} */
let mainWindow = null;
/** @type {import('electron').BrowserWindow | null} */
let stopWin = null;
let aborted = false;
let lastActAt = 0;
/** @type {{ width: number, height: number, screenWidth: number, screenHeight: number } | null} */
let lastShotMeta = null;
let lastInjector = "";

function sleep(ms) {
  return new Promise((r) => setTimeout(r, Math.max(0, ms)));
}

async function sleepAbortable(ms) {
  const end = Date.now() + Math.max(0, ms);
  while (!aborted && Date.now() < end) {
    await sleep(Math.min(50, end - Date.now()));
  }
}

function restoreHiddenWindow(win) {
  if (!win || (typeof win.isDestroyed === "function" && win.isDestroyed())) return;
  if (typeof win.showInactive === "function") {
    win.showInactive();
    return;
  }
  if (typeof win.show === "function") win.show();
}

function ydotoolScrollArgs(direction, amount) {
  const n = Math.max(1, Math.min(20, Number(amount) || 1));
  const y = direction === "up" ? n : -n;
  return ["mousemove", "--wheel", "-x", "0", "-y", String(y)];
}

function whichSync(bin) {
  const dirs = String(process.env.PATH || "/usr/bin:/bin").split(":");
  for (const d of dirs) {
    try {
      const p = path.join(d, bin);
      if (fs.existsSync(p)) return p;
    } catch {
      /* ignore */
    }
  }
  return "";
}

function sessionType() {
  const raw = String(process.env.XDG_SESSION_TYPE || "").toLowerCase();
  if (raw === "wayland" || raw === "x11") return raw;
  if (process.env.WAYLAND_DISPLAY) return "wayland";
  if (process.env.DISPLAY) return "x11";
  return "unknown";
}

function pickInjector() {
  const sess = sessionType();
  const xdotool = whichSync("xdotool");
  const ydotool = whichSync("ydotool");
  if (xdotool && (sess === "x11" || process.env.DISPLAY)) {
    lastInjector = "xdotool";
    return { kind: "xdotool", path: xdotool, session: sess };
  }
  if (ydotool) {
    lastInjector = "ydotool";
    return { kind: "ydotool", path: ydotool, session: sess };
  }
  if (xdotool) {
    lastInjector = "xdotool";
    return { kind: "xdotool", path: xdotool, session: sess };
  }
  lastInjector = "";
  return { kind: "", path: "", session: sess };
}

function electronApis() {
  try {
    return require("electron");
  } catch {
    return null;
  }
}

function setMainWindow(win) {
  mainWindow = win && !win.isDestroyed?.() ? win : null;
}

function userStop() {
  aborted = true;
  hideStopBar();
  try {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send("computer:userStop");
    }
  } catch {
    /* ignore */
  }
  return { ok: true, aborted: true };
}

function resetAbort() {
  aborted = false;
}

function isAborted() {
  return aborted;
}

function showStopBar() {
  const electron = electronApis();
  if (!electron || !electron.BrowserWindow) return;
  try {
    if (stopWin && !stopWin.isDestroyed()) {
      stopWin.show();
      return;
    }
    const { BrowserWindow, screen } = electron;
    const wa = screen.getPrimaryDisplay().workArea;
    const width = 520;
    stopWin = new BrowserWindow({
      width,
      height: 44,
      x: Math.round(wa.x + (wa.width - width) / 2),
      y: Math.max(8, wa.y + 8),
      frame: false,
      alwaysOnTop: true,
      skipTaskbar: true,
      resizable: false,
      minimizable: false,
      maximizable: false,
      fullscreenable: false,
      backgroundColor: "#161616",
      webPreferences: {
        preload: path.join(__dirname, "computer-stop-preload.cjs"),
        sandbox: true,
        contextIsolation: true,
      },
    });
    stopWin.setAlwaysOnTop(true, "screen-saver");
    try {
      stopWin.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true });
    } catch {
      /* ignore */
    }
    const html = `<!doctype html>
<html><body style="margin:0;font:13px/44px ui-sans-serif,system-ui,sans-serif;background:#161616;color:#f5f5f5;display:flex;align-items:center;justify-content:space-between;padding:0 14px;user-select:none;-webkit-app-region:drag">
<span>GrokHub is controlling this computer</span>
<button id="s" style="-webkit-app-region:no-drag;background:#eee;color:#111;border:0;border-radius:8px;padding:6px 12px;font-weight:600;cursor:pointer">Stop</button>
<script>document.getElementById('s').onclick=function(){window.grokhubStop&&window.grokhubStop.stop()}</script>
</body></html>`;
    stopWin.loadURL("data:text/html;charset=utf-8," + encodeURIComponent(html));
  } catch (e) {
    console.warn("[computer] stop bar", e);
  }
}

function hideStopBar() {
  try {
    if (stopWin && !stopWin.isDestroyed()) stopWin.close();
  } catch {
    /* ignore */
  }
  stopWin = null;
}

async function runTool(bin, args, timeoutMs = 8000) {
  try {
    const { stdout, stderr } = await execFileAsync(bin, args, {
      timeout: timeoutMs,
      env: process.env,
      maxBuffer: 2_000_000,
    });
    return { ok: true, stdout: String(stdout || ""), stderr: String(stderr || "") };
  } catch (e) {
    return {
      ok: false,
      stdout: String(e.stdout || ""),
      stderr: String(e.stderr || e.message || e),
    };
  }
}

function info() {
  const inj = pickInjector();
  const electron = Boolean(electronApis());
  let screenSize = null;
  try {
    const e = electronApis();
    if (e && e.screen) {
      const b = e.screen.getPrimaryDisplay().bounds;
      screenSize = { width: b.width, height: b.height, scaleFactor: e.screen.getPrimaryDisplay().scaleFactor };
    }
  } catch {
    /* ignore */
  }
  return {
    ok: true,
    platform: process.platform,
    session: inj.session,
    injector: inj.kind || null,
    injectorPath: inj.path || null,
    display: process.env.DISPLAY || null,
    waylandDisplay: process.env.WAYLAND_DISPLAY || null,
    electron,
    screen: screenSize,
    lastScreenshot: lastShotMeta,
    hint:
      inj.kind
        ? inj.kind === "ydotool"
          ? "ydotool found — user may need to be in the input group (uinput)."
          : "xdotool found."
        : "Install xdotool (X11/XWayland) or ydotool (Wayland) to inject mouse/keyboard.",
  };
}

function mapToScreen(x, y) {
  const sx = Number(x);
  const sy = Number(y);
  if (!lastShotMeta || !lastShotMeta.width || !lastShotMeta.height) {
    return { x: Math.round(sx), y: Math.round(sy), mapped: false };
  }
  return {
    x: Math.round((sx * lastShotMeta.screenWidth) / lastShotMeta.width),
    y: Math.round((sy * lastShotMeta.screenHeight) / lastShotMeta.height),
    mapped: true,
  };
}

function grokhubBounds() {
  try {
    if (!mainWindow || mainWindow.isDestroyed()) return null;
    return mainWindow.getBounds();
  } catch {
    return null;
  }
}

function pointHitsGrokHub(x, y) {
  const b = grokhubBounds();
  if (!b) return false;
  return x >= b.x && x <= b.x + b.width && y >= b.y && y <= b.y + b.height;
}

async function maybeHideGrokHubForClick(x, y) {
  if (!pointHitsGrokHub(x, y)) return false;
  try {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.minimize();
      await sleep(120);
      return true;
    }
  } catch {
    /* ignore */
  }
  return false;
}

async function captureScreenshot() {
  const electron = electronApis();
  if (!electron || !electron.desktopCapturer) {
    return { ok: false, error: "Screenshot requires the Electron desktop shell." };
  }
  const { desktopCapturer, screen, nativeImage } = electron;
  const display = screen.getPrimaryDisplay();
  const bounds = display.bounds;
  const sf = display.scaleFactor || 1;
  const thumbW = Math.max(1, Math.round(bounds.width * sf));
  const thumbH = Math.max(1, Math.round(bounds.height * sf));
  let hidden = false;
  try {
    if (mainWindow && !mainWindow.isDestroyed() && mainWindow.isVisible()) {
      mainWindow.hide();
      hidden = true;
      await sleep(80);
    }
  } catch {
    /* ignore */
  }
  try {
    const sources = await desktopCapturer.getSources({
      types: ["screen"],
      thumbnailSize: { width: thumbW, height: thumbH },
    });
    const src =
      sources.find((s) => String(s.id).includes(String(display.id))) || sources[0];
    if (!src || !src.thumbnail) {
      return { ok: false, error: "No screen source from desktopCapturer." };
    }
    let img = src.thumbnail;
    if (typeof img.toJPEG !== "function" && nativeImage && nativeImage.createFromDataURL) {
      img = nativeImage.createFromDataURL(img.toDataURL());
    }
    const size = img.getSize();
    if (size.width > SCREENSHOT_MAX_WIDTH) {
      img = img.resize({ width: SCREENSHOT_MAX_WIDTH, quality: "better" });
    }
    const jpeg = img.toJPEG(72);
    const dataUrl = `data:image/jpeg;base64,${Buffer.from(jpeg).toString("base64")}`;
    const shot = img.getSize();
    lastShotMeta = {
      width: shot.width,
      height: shot.height,
      screenWidth: bounds.width,
      screenHeight: bounds.height,
    };
    return {
      ok: true,
      dataUrl,
      screen: { width: bounds.width, height: bounds.height, scaleFactor: sf },
      screenshot: { width: shot.width, height: shot.height },
    };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  } finally {
    if (hidden) {
      try {
        restoreHiddenWindow(mainWindow);
      } catch {
        /* ignore */
      }
    }
  }
}

async function injectXdotool(bin, step, mapped) {
  const op = step.op;
  if (op === "move" || op === "click" || op === "double_click") {
    const args = ["mousemove", "--sync", String(mapped.x), String(mapped.y)];
    const mv = await runTool(bin, args);
    if (!mv.ok) return mv;
    if (op === "move") return mv;
    if (op === "double_click") return runTool(bin, ["click", "--repeat", "2", "1"]);
    return runTool(bin, ["click", "1"]);
  }
  if (op === "scroll") {
    const btn = step.direction === "up" ? "4" : "5";
    const n = Math.max(1, Math.min(20, Number(step.amount) || 1));
    return runTool(bin, ["click", "--repeat", String(n), btn]);
  }
  if (op === "type") {
    const text = String(step.text || "").slice(0, MAX_TYPE_CHARS);
    return runTool(bin, ["type", "--clearmodifiers", "--delay", "8", "--", text]);
  }
  if (op === "key") {
    const key = String(step.key || "Return").replace(/\s+/g, "+");
    return runTool(bin, ["key", "--clearmodifiers", key]);
  }
  return { ok: false, stderr: "unknown op" };
}

async function injectYdotool(bin, step, mapped) {
  const op = step.op;
  if (op === "move" || op === "click" || op === "double_click") {
    const mv = await runTool(bin, ["mousemove", "--absolute", String(mapped.x), String(mapped.y)]);
    if (!mv.ok) {
      const alt = await runTool(bin, ["mousemove", "-a", String(mapped.x), String(mapped.y)]);
      if (!alt.ok) return mv;
    }
    if (op === "move") return { ok: true, stdout: "", stderr: "" };
    if (op === "double_click") {
      const one = await runTool(bin, ["click", "0xC0"]);
      if (!one.ok) return one;
      await sleep(60);
      return runTool(bin, ["click", "0xC0"]);
    }
    return runTool(bin, ["click", "0xC0"]);
  }
  if (op === "scroll") {
    return runTool(bin, ydotoolScrollArgs(step.direction, step.amount));
  }
  if (op === "type") {
    const text = String(step.text || "").slice(0, MAX_TYPE_CHARS);
    return runTool(bin, ["type", text]);
  }
  if (op === "key") {
    const key = String(step.key || "Return").replace(/\s+/g, "+");
    return runTool(bin, ["key", key]);
  }
  return { ok: false, stderr: "unknown op" };
}

async function act(step) {
  if (aborted) return { ok: false, error: "Computer use stopped." };
  const op = step && step.op;
  if (!op) return { ok: false, error: "Missing op" };
  if (op === "wait") {
    const ms = Math.max(0, Math.min(15_000, Number(step.ms) || 400));
    await sleepAbortable(ms);
    if (aborted) return { ok: false, error: "Computer use stopped." };
    return { ok: true, op, ms };
  }
  if (op === "screenshot") {
    const shot = await captureScreenshot();
    if (!shot.ok) return shot;
    return {
      ok: true,
      op,
      dataUrl: shot.dataUrl,
      screen: shot.screen,
      screenshot: shot.screenshot,
    };
  }
  const inj = pickInjector();
  if (!inj.kind) {
    return {
      ok: false,
      error:
        inj.session === "wayland"
          ? "No input injector. Install ydotool (and grant uinput) or use an X11/XWayland session with xdotool."
          : "No input injector. Install xdotool (pacman -S xdotool).",
      session: inj.session,
    };
  }
  const now = Date.now();
  const waitFor = MIN_ACTION_DELAY_MS - (now - lastActAt);
  if (waitFor > 0) await sleep(waitFor);
  lastActAt = Date.now();

  const mapped = mapToScreen(step.x, step.y);
  if (op === "click" || op === "double_click" || op === "move") {
    await maybeHideGrokHubForClick(mapped.x, mapped.y);
  }

  const r =
    inj.kind === "ydotool"
      ? await injectYdotool(inj.path, step, mapped)
      : await injectXdotool(inj.path, step, mapped);
  if (aborted) return { ok: false, error: "Computer use stopped." };
  if (!r.ok) {
    return {
      ok: false,
      op,
      error: (r.stderr || "injector failed").slice(0, 400),
      injector: inj.kind,
      mapped,
    };
  }
  return {
    ok: true,
    op,
    injector: inj.kind,
    mapped: op === "type" || op === "key" || op === "scroll" ? undefined : mapped,
  };
}

async function beginSession() {
  aborted = false;
  showStopBar();
  return { ok: true, ...info() };
}

function endSession() {
  hideStopBar();
  return { ok: true };
}

module.exports = {
  setMainWindow,
  info,
  act,
  screenshot: captureScreenshot,
  beginSession,
  endSession,
  userStop,
  resetAbort,
  isAborted,
  pickInjector,
  restoreHiddenWindow,
  ydotoolScrollArgs,
};
