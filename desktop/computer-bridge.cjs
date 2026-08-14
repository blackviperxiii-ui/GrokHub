/**
 * Linux desktop computer-use — same stack Cursor uses:
 * ffmpeg x11grab (or grim on Wayland) + xdotool/ydotool.
 * Never opens a screenshot/portal app.
 * Safe to require without Electron — screenshot/input then return errors.
 */
const { execFile, execFileSync, spawn } = require("node:child_process");
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
let previewTimer = null;
let previewBusy = false;
/** @type {import('node:child_process').ChildProcess | null} */
let previewProc = null;
/** @type {"user" | "session" | null} */
let previewOwner = null;
let previewGotFrame = false;
let previewSkipCapture = "";

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

function vendorBinDirs() {
  const dirs = [
    path.join(__dirname, "..", "vendor", "linux-x64"),
    path.join(process.cwd(), "vendor", "linux-x64"),
  ];
  try {
    if (process.resourcesPath) dirs.unshift(path.join(process.resourcesPath, "vendor", "linux-x64"));
  } catch {
    /* ignore */
  }
  return dirs;
}

function whichSync(bin, extraDirs) {
  const dirs = [...(Array.isArray(extraDirs) ? extraDirs : []), ...vendorBinDirs()];
  for (const d of String(process.env.PATH || "/usr/bin:/bin").split(":")) dirs.push(d);
  const seen = new Set();
  for (const d of dirs) {
    if (!d || seen.has(d)) continue;
    seen.add(d);
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

function preferredInjectorKind(session, bins) {
  const x = bins && bins.xdotool;
  const y = bins && bins.ydotool;
  if (session === "wayland") {
    if (y) return "ydotool";
    if (x) return "xdotool";
    return "";
  }
  if (x) return "xdotool";
  if (y) return "ydotool";
  return "";
}

function pickInjector() {
  const sess = sessionType();
  const xdotool = whichSync("xdotool");
  const ydotool = whichSync("ydotool");
  const kind = preferredInjectorKind(sess, { xdotool, ydotool });
  lastInjector = kind;
  if (kind === "ydotool") return { kind, path: ydotool, session: sess };
  if (kind === "xdotool") return { kind, path: xdotool, session: sess };
  return { kind: "", path: "", session: sess };
}

/** Same grab Cursor uses: ffmpeg x11grab → one JPEG, no screenshot GUI. */
function ffmpegX11grabArgs(display, out, opts) {
  const args = ["-y", "-hide_banner", "-loglevel", "error", "-f", "x11grab"];
  const w = opts && Number(opts.width);
  const h = opts && Number(opts.height);
  if (w > 0 && h > 0) args.push("-video_size", `${Math.round(w)}x${Math.round(h)}`);
  args.push(
    "-i",
    display || ":0",
    "-frames:v",
    "1",
    "-vf",
    `scale=${SCREENSHOT_MAX_WIDTH}:-1`,
    "-q:v",
    "5",
    out,
  );
  return args;
}

function ffmpegMjpegArgs(display, opts) {
  const args = ["-hide_banner", "-loglevel", "error", "-f", "x11grab"];
  const w = opts && Number(opts.width);
  const h = opts && Number(opts.height);
  if (w > 0 && h > 0) args.push("-video_size", `${Math.round(w)}x${Math.round(h)}`);
  args.push(
    "-i",
    display || ":0",
    "-vf",
    `fps=2,scale=${SCREENSHOT_MAX_WIDTH}:-1`,
    "-f",
    "image2pipe",
    "-vcodec",
    "mjpeg",
    "-q:v",
    "5",
    "pipe:1",
  );
  return args;
}

/** End offset (exclusive) of a complete JPEG starting at `start`, or -1. */
function jpegEndExclusive(buf, start) {
  if (start + 2 > buf.length || buf[start] !== 0xff || buf[start + 1] !== 0xd8) return -1;
  let i = start + 2;
  while (i < buf.length - 1) {
    if (buf[i] !== 0xff) {
      i += 1;
      continue;
    }
    while (i < buf.length && buf[i] === 0xff) i += 1;
    if (i >= buf.length) return -1;
    const marker = buf[i];
    if (marker === 0xd9) return i + 1;
    if (marker === 0xd8) continue;
    if (marker === 0xda) {
      i += 1;
      while (i < buf.length - 1) {
        if (buf[i] !== 0xff) {
          i += 1;
          continue;
        }
        const nxt = buf[i + 1];
        if (nxt === 0x00 || nxt === 0xff) {
          i += 1;
          continue;
        }
        if (nxt === 0xd9) return i + 2;
        if (nxt >= 0xd0 && nxt <= 0xd7) {
          i += 2;
          continue;
        }
        i += 2;
      }
      return -1;
    }
    if (marker >= 0xd0 && marker <= 0xd7) continue;
    if (i + 2 >= buf.length) return -1;
    const len = buf.readUInt16BE(i + 1);
    if (len < 2) return -1;
    i += 1 + len;
  }
  return -1;
}

/** Split concatenated JPEGs from an ffmpeg image2pipe. */
function splitJpegFrames(onJpeg) {
  let buf = Buffer.alloc(0);
  return (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    if (buf.length > 8 * 1024 * 1024) buf = buf.subarray(-1024);
    while (true) {
      const soi = buf.indexOf(Buffer.from([0xff, 0xd8]));
      if (soi < 0) {
        buf = buf.length && buf[buf.length - 1] === 0xff ? buf.subarray(-1) : Buffer.alloc(0);
        return;
      }
      if (soi > 0) buf = buf.subarray(soi);
      const end = jpegEndExclusive(buf, 0);
      if (end < 0) return;
      onJpeg(buf.subarray(0, end));
      buf = buf.subarray(end);
    }
  };
}

function jpegSize(buf) {
  let i = 2;
  while (i < buf.length - 8) {
    if (buf[i] !== 0xff) {
      i += 1;
      continue;
    }
    while (i < buf.length && buf[i] === 0xff) i += 1;
    if (i >= buf.length) break;
    const marker = buf[i];
    if (marker === 0xc0 || marker === 0xc1 || marker === 0xc2) {
      if (i + 6 >= buf.length) break;
      return { height: buf.readUInt16BE(i + 4), width: buf.readUInt16BE(i + 6) };
    }
    if (marker === 0xd8 || marker === 0xd9 || (marker >= 0xd0 && marker <= 0xd7)) continue;
    if (marker === 0xda) break;
    if (i + 2 >= buf.length) break;
    const len = buf.readUInt16BE(i + 1);
    if (len < 2) break;
    i += 1 + len;
  }
  return { width: 0, height: 0 };
}

function previewAllowsDesktopCapturer() {
  return false;
}

function previewCaptureKind(session, bins) {
  return silentCaptureTools(session, bins)[0]?.name || "";
}

/** Full-screen capture tools that do not open a picker / screenshot GUI. */
function silentCaptureTools(session, bins) {
  const b = bins || {};
  const out = [];
  if ((session === "wayland" || b.wayland) && b.grim) {
    out.push({ name: "grim", bin: b.grim, args: ["OUT"], ext: ".png" });
  }
  const waylandHasGrim = (session === "wayland" || b.wayland) && b.grim;
  if (b.ffmpeg && !waylandHasGrim && (b.display || session === "x11")) {
    out.push({
      name: "ffmpeg-x11grab",
      bin: b.ffmpeg,
      args: ffmpegX11grabArgs(b.display || ":0", "OUT", { width: b.width, height: b.height }),
      ext: ".jpg",
    });
  }
  if (b.maim) out.push({ name: "maim", bin: b.maim, args: ["-u", "OUT"], ext: ".png" });
  if (b.scrot) out.push({ name: "scrot", bin: b.scrot, args: ["-o", "-z", "OUT"], ext: ".png" });
  return out;
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
  if (previewOwner === "session") stopPreview({ force: true });
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

function probeUinput() {
  const p = "/dev/uinput";
  try {
    const st = fs.statSync(p);
    let writable = false;
    try {
      fs.accessSync(p, fs.constants.R_OK | fs.constants.W_OK);
      writable = true;
    } catch {
      writable = false;
    }
    return { path: p, exists: true, writable, mode: st.mode };
  } catch {
    return { path: p, exists: false, writable: false };
  }
}

function missingComputerTools(session, bins) {
  const missing = [];
  if (session === "wayland") {
    if (!bins.ydotool) missing.push("ydotool");
    if (!bins.grim) missing.push("grim");
    if (!bins.xdotool) missing.push("xdotool");
  } else {
    if (!bins.xdotool) missing.push("xdotool");
    if (!bins.ffmpeg && !bins.grim) missing.push("ffmpeg");
  }
  if (!bins.ffmpeg) missing.push("ffmpeg");
  return [...new Set(missing)];
}

function info() {
  const inj = pickInjector();
  const electron = Boolean(electronApis());
  const geo = liveGeometry();
  const bins = captureToolBins();
  const plans = silentCaptureTools(inj.session, bins);
  const missingTools = missingComputerTools(inj.session, {
    ydotool: whichSync("ydotool"),
    xdotool: whichSync("xdotool"),
    grim: bins.grim,
    ffmpeg: bins.ffmpeg,
  });
  return {
    ok: true,
    platform: process.platform,
    session: inj.session,
    injector: inj.kind || null,
    injectorPath: inj.path || null,
    display: process.env.DISPLAY || null,
    waylandDisplay: process.env.WAYLAND_DISPLAY || null,
    electron,
    screen: geo.width
      ? { width: geo.width, height: geo.height, scaleFactor: geo.scaleFactor || 1 }
      : null,
    geometry: geo,
    lastScreenshot: lastShotMeta,
    capture: plans[0]?.name || null,
    captureTools: plans.map((p) => p.name),
    missingTools,
    uinput: probeUinput(),
    ydotoold: whichSync("ydotoold") || null,
    vendorDir: vendorBinDirs()[0] || null,
    previewing: isPreviewing(),
    hint: buildHint(inj, plans, missingTools),
  };
}

let x11SizeCache = { width: 0, height: 0, at: 0, source: "" };

function readCliDimensions(bin, args, parse) {
  const exe = whichSync(bin);
  if (!exe) return { width: 0, height: 0 };
  try {
    const out = execFileSync(exe, args, {
      encoding: "utf8",
      timeout: 2000,
      env: process.env,
    });
    return parse(out);
  } catch {
    return { width: 0, height: 0 };
  }
}

function x11PixelSize() {
  if (x11SizeCache.width && Date.now() - x11SizeCache.at < 5000) {
    return { width: x11SizeCache.width, height: x11SizeCache.height, source: x11SizeCache.source };
  }
  const tries = [
    () => {
      const xdotool = whichSync("xdotool");
      if (!xdotool) return { width: 0, height: 0 };
      const out = execFileSync(xdotool, ["getdisplaygeometry"], {
        encoding: "utf8",
        timeout: 2000,
        env: process.env,
      });
      const m = String(out).trim().match(/^(\d+)\s+(\d+)/);
      return m
        ? { width: Number(m[1]), height: Number(m[2]), source: "xdotool" }
        : { width: 0, height: 0 };
    },
    () => ({ ...readCliDimensions("xdpyinfo", [], parseXdpyinfoDimensions), source: "xdpyinfo" }),
    () => ({ ...readCliDimensions("xrandr", [], parseXrandrCurrentDimensions), source: "xrandr" }),
  ];
  for (const tryOne of tries) {
    try {
      const sz = tryOne();
      if (sz.width > 0 && sz.height > 0) {
        x11SizeCache = { width: sz.width, height: sz.height, at: Date.now(), source: sz.source || "" };
        return { width: sz.width, height: sz.height, source: sz.source || "" };
      }
    } catch {
      /* next probe */
    }
  }
  return { width: 0, height: 0, source: "" };
}

function captureToolBins() {
  const px = x11PixelSize();
  const bounds = screenBounds();
  return {
    grim: whichSync("grim"),
    ffmpeg: whichSync("ffmpeg"),
    display: process.env.DISPLAY || "",
    width: px.width || (bounds.scaleFactor > 1 ? Math.round(bounds.width * bounds.scaleFactor) : 0),
    height: px.height || (bounds.scaleFactor > 1 ? Math.round(bounds.height * bounds.scaleFactor) : 0),
    maim: whichSync("maim"),
    scrot: whichSync("scrot"),
  };
}

function buildHint(inj, plans, missingTools) {
  const bits = [];
  const missing = Array.isArray(missingTools) ? missingTools : [];
  if (inj.kind === "ydotool") {
    bits.push("ydotool found — /dev/uinput must be writable (KDE uaccess ACL or input group).");
  } else if (inj.kind === "xdotool") {
    bits.push(
      inj.session === "wayland"
        ? "xdotool on Wayland only moves XWayland apps. Install ydotool + grim for native windows."
        : "xdotool found.",
    );
  } else {
    bits.push(
      inj.session === "wayland"
        ? "Install ydotool (and grim) for this Wayland session: pacman -S --needed ydotool grim xdotool ffmpeg"
        : "Install xdotool (X11/XWayland) or ydotool (Wayland) to inject mouse/keyboard.",
    );
  }
  if (!plans.length) {
    bits.push(
      "Install grim (Wayland) or ffmpeg (X11). GrokHub will not open a screenshot app.",
    );
  } else if (plans[0].name === "ffmpeg-x11grab") {
    bits.push(
      inj.session === "wayland"
        ? "Live view is ffmpeg x11grab (XWayland). Install grim for the native Wayland framebuffer."
        : "Live view uses ffmpeg x11grab (no screenshot picker).",
    );
  } else if (plans[0].name === "grim") {
    bits.push("Live view uses grim (no screenshot picker).");
  }
  if (missing.length) bits.push(`Missing: ${missing.join(", ")}.`);
  return bits.join(" ");
}

function parseXdpyinfoDimensions(text) {
  const m = String(text || "").match(/dimensions:\s+(\d+)x(\d+)\s+pixels/i);
  if (!m) return { width: 0, height: 0 };
  return { width: Number(m[1]), height: Number(m[2]) };
}

function parseXrandrCurrentDimensions(text) {
  const m = String(text || "").match(/current\s+(\d+)\s+x\s+(\d+)/i);
  if (!m) return { width: 0, height: 0 };
  return { width: Number(m[1]), height: Number(m[2]) };
}

function unionDisplayBounds(displays) {
  const boxes = (Array.isArray(displays) ? displays : [])
    .map((d) => d && d.bounds)
    .filter((b) => b && Number(b.width) > 0 && Number(b.height) > 0);
  if (!boxes.length) return { x: 0, y: 0, width: 0, height: 0 };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const b of boxes) {
    const x = Number(b.x) || 0;
    const y = Number(b.y) || 0;
    const w = Number(b.width);
    const h = Number(b.height);
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    maxX = Math.max(maxX, x + w);
    maxY = Math.max(maxY, y + h);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

function injectorScreenSize(opts) {
  const o = opts || {};
  const x11 = o.x11 || { width: 0, height: 0 };
  const electron = o.electron || { width: 0, height: 0, scaleFactor: 1 };
  const sf = Number(electron.scaleFactor) > 0 ? Number(electron.scaleFactor) : 1;
  if (x11.width > 0 && x11.height > 0) {
    return { width: Math.round(x11.width), height: Math.round(x11.height) };
  }
  if (electron.width > 0 && electron.height > 0) {
    return {
      width: Math.round(electron.width * sf),
      height: Math.round(electron.height * sf),
    };
  }
  return { width: 0, height: 0 };
}

function electronVirtualScreen() {
  try {
    const e = electronApis();
    if (!e || !e.screen) return { width: 0, height: 0, scaleFactor: 1 };
    const displays = typeof e.screen.getAllDisplays === "function" ? e.screen.getAllDisplays() : [];
    const union = unionDisplayBounds(displays);
    const primary = e.screen.getPrimaryDisplay();
    const sf = (primary && primary.scaleFactor) || 1;
    if (union.width > 0 && union.height > 0) {
      return { width: union.width, height: union.height, scaleFactor: sf };
    }
    const b = primary && primary.bounds;
    if (b && b.width) return { width: b.width, height: b.height, scaleFactor: sf };
  } catch {
    /* ignore */
  }
  return { width: 0, height: 0, scaleFactor: 1 };
}

function liveGeometry() {
  const x11 = x11PixelSize();
  const electron = electronVirtualScreen();
  const size = injectorScreenSize({ x11, electron });
  const source = x11.source || (electron.width ? "electron-virtual" : "");
  return {
    width: size.width,
    height: size.height,
    scaleFactor: electron.scaleFactor || 1,
    source,
  };
}

function liveInjectorScreenSize() {
  const geo = liveGeometry();
  return { width: geo.width, height: geo.height };
}

function shotMetaForMapping(shot, screen) {
  return {
    width: Number(shot && shot.width) || 0,
    height: Number(shot && shot.height) || 0,
    screenWidth: Number(screen && screen.width) || 0,
    screenHeight: Number(screen && screen.height) || 0,
  };
}

function pointHitsScaledBounds(x, y, bounds, scaleFactor) {
  if (!bounds) return false;
  const s = Number(scaleFactor) > 0 ? Number(scaleFactor) : 1;
  const bx = bounds.x * s;
  const by = bounds.y * s;
  const bw = bounds.width * s;
  const bh = bounds.height * s;
  return x >= bx && x <= bx + bw && y >= by && y <= by + bh;
}

function previewStartVerdict(opts) {
  const o = opts || {};
  if (o.gotFrame) return "ready";
  if (!o.procAlive) return "dead";
  if (Number(o.elapsedMs) >= Number(o.waitMs)) return "timeout-alive";
  return "wait";
}

function nextPreviewPlan(plans, failedName) {
  const list = Array.isArray(plans) ? plans : [];
  const i = list.findIndex((p) => p && p.name === failedName);
  if (i < 0) return list[0] || null;
  return list[i + 1] || null;
}

function filterCapturePlans(plans, skipName) {
  const list = Array.isArray(plans) ? plans : [];
  if (!skipName) return list;
  return list.filter((p) => p && p.name !== skipName);
}

function mapToScreen(x, y, meta) {
  const m = meta || lastShotMeta;
  const sx = Number(x);
  const sy = Number(y);
  if (
    !m ||
    !m.width ||
    !m.height ||
    !m.screenWidth ||
    !m.screenHeight
  ) {
    return { x: Math.round(sx), y: Math.round(sy), mapped: false };
  }
  return {
    x: Math.round((sx * m.screenWidth) / m.width),
    y: Math.round((sy * m.screenHeight) / m.height),
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
  let scale = 1;
  try {
    const e = electronApis();
    if (e && e.screen) scale = e.screen.getPrimaryDisplay().scaleFactor || 1;
  } catch {
    /* ignore */
  }
  return pointHitsScaledBounds(x, y, b, scale);
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

function screenBounds() {
  const geo = liveGeometry();
  if (geo.width && geo.height) {
    return { width: geo.width, height: geo.height, scaleFactor: geo.scaleFactor || 1 };
  }
  return { width: lastShotMeta?.screenWidth || 0, height: lastShotMeta?.screenHeight || 0, scaleFactor: 1 };
}

function encodeShotFile(filePath) {
  const electron = electronApis();
  if (electron && electron.nativeImage) {
    let img = electron.nativeImage.createFromPath(filePath);
    const size = img.getSize();
    if (size.width > SCREENSHOT_MAX_WIDTH) {
      img = img.resize({ width: SCREENSHOT_MAX_WIDTH, quality: "better" });
    }
    const jpeg = img.toJPEG(72);
    const shot = img.getSize();
    return {
      dataUrl: `data:image/jpeg;base64,${Buffer.from(jpeg).toString("base64")}`,
      screenshot: { width: shot.width, height: shot.height },
    };
  }
  const buf = fs.readFileSync(filePath);
  const isJpeg = buf.length > 2 && buf[0] === 0xff && buf[1] === 0xd8;
  const size = isJpeg ? jpegSize(buf) : { width: lastShotMeta?.width || 0, height: lastShotMeta?.height || 0 };
  return {
    dataUrl: `data:image/${isJpeg ? "jpeg" : "png"};base64,${buf.toString("base64")}`,
    screenshot: { width: size.width || lastShotMeta?.width || 0, height: size.height || lastShotMeta?.height || 0 },
  };
}

function emitFrame(payload) {
  if (payload && payload.ok && payload.dataUrl) previewGotFrame = true;
  try {
    if (mainWindow && !mainWindow.isDestroyed()) {
      mainWindow.webContents.send("computer:frame", payload);
    }
  } catch {
    /* ignore */
  }
}

async function captureViaCli() {
  const sess = sessionType();
  const plans = filterCapturePlans(silentCaptureTools(sess, captureToolBins()), previewSkipCapture);
  for (const plan of plans) {
    const ext = plan.ext || ".png";
    const tmp = path.join(
      os.tmpdir(),
      `grokhub-cap-${process.pid}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}${ext}`,
    );
    const args = plan.args.map((a) => (a === "OUT" ? tmp : a));
    const r = await runTool(plan.bin, args, 12000);
    try {
      if (!r.ok || !fs.existsSync(tmp) || fs.statSync(tmp).size < 80) continue;
      const encoded = encodeShotFile(tmp);
      const bounds = screenBounds();
      const inj = liveInjectorScreenSize();
      lastShotMeta = shotMetaForMapping(
        {
          width: encoded.screenshot.width || inj.width || bounds.width,
          height: encoded.screenshot.height || inj.height || bounds.height,
        },
        inj,
      );
      return {
        ok: true,
        dataUrl: encoded.dataUrl,
        screen: {
          width: inj.width || bounds.width,
          height: inj.height || bounds.height,
          scaleFactor: bounds.scaleFactor,
        },
        screenshot: encoded.screenshot,
        capture: plan.name,
      };
    } catch {
      /* try next tool */
    } finally {
      try {
        fs.unlinkSync(tmp);
      } catch {
        /* ignore */
      }
    }
  }
  return null;
}

async function captureScreenshot() {
  const cli = await captureViaCli();
  if (cli && cli.ok) {
    emitFrame(cli);
    return cli;
  }
  return {
    ok: false,
    error:
      "Silent capture failed. Install ffmpeg (X11) or grim (Wayland). GrokHub will not open a screenshot app.",
  };
}

function isPreviewing() {
  return Boolean(previewTimer || previewProc);
}

function stopFfmpegPreview() {
  const child = previewProc;
  if (!child) return;
  previewProc = null;
  try {
    child.kill("SIGTERM");
  } catch {
    /* ignore */
  }
  const killer = setTimeout(() => {
    try {
      child.kill("SIGKILL");
    } catch {
      /* ignore */
    }
  }, 1000);
  if (typeof killer.unref === "function") killer.unref();
}

function emitJpegFrame(jpeg, capture) {
  const shot = jpegSize(jpeg);
  const bounds = screenBounds();
  const inj = liveInjectorScreenSize();
  if (shot.width && shot.height) {
    lastShotMeta = shotMetaForMapping(shot, {
      width: inj.width || lastShotMeta?.screenWidth || 0,
      height: inj.height || lastShotMeta?.screenHeight || 0,
    });
  }
  emitFrame({
    ok: true,
    dataUrl: `data:image/jpeg;base64,${jpeg.toString("base64")}`,
    screen: {
      width: inj.width || bounds.width,
      height: inj.height || bounds.height,
      scaleFactor: bounds.scaleFactor,
    },
    screenshot: { width: shot.width || lastShotMeta?.width || 0, height: shot.height || lastShotMeta?.height || 0 },
    capture,
  });
}

function startFfmpegPreview(display) {
  previewGotFrame = false;
  const bins = captureToolBins();
  const bin = bins.ffmpeg || whichSync("ffmpeg") || "ffmpeg";
  const child = spawn(bin, ffmpegMjpegArgs(display, bins), {
    env: { ...process.env, DISPLAY: display },
    stdio: ["ignore", "pipe", "ignore"],
  });
  const onJpeg = splitJpegFrames((jpeg) => emitJpegFrame(jpeg, "ffmpeg-x11grab"));
  child.stdout.on("data", onJpeg);
  child.on("error", () => {
    if (previewProc === child) previewProc = null;
  });
  child.on("exit", () => {
    if (previewProc === child) previewProc = null;
  });
  previewProc = child;
  return child;
}

function waitForPreviewReady(waitMs) {
  return new Promise((resolve) => {
    const t0 = Date.now();
    const tick = () => {
      const v = previewStartVerdict({
        gotFrame: previewGotFrame,
        procAlive: Boolean(previewProc),
        elapsedMs: Date.now() - t0,
        waitMs,
      });
      if (v === "wait") {
        setTimeout(tick, 40);
        return;
      }
      resolve(v);
    };
    tick();
  });
}

function startTimerPreview(ms) {
  const tick = async () => {
    if (previewBusy) return;
    previewBusy = true;
    try {
      await captureScreenshot();
    } finally {
      previewBusy = false;
    }
  };
  void tick();
  previewTimer = setInterval(() => void tick(), ms);
}

function stopPreview(opts) {
  const reason = opts && opts.reason;
  const force = Boolean(opts && opts.force);
  if (!force && reason === "session" && previewOwner === "user") {
    return { ok: true, previewing: isPreviewing(), owner: previewOwner };
  }
  if (previewTimer) {
    clearInterval(previewTimer);
    previewTimer = null;
  }
  stopFfmpegPreview();
  previewOwner = null;
  previewSkipCapture = "";
  previewGotFrame = false;
  return { ok: true, previewing: false };
}

async function startPreview(intervalMs, owner) {
  const ms = Math.max(250, Math.min(2000, Number(intervalMs) || 450));
  const who = owner === "session" ? "session" : "user";
  if (isPreviewing() && previewOwner === "user" && who === "session") {
    return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner };
  }
  stopPreview({ force: true });
  const plans = silentCaptureTools(sessionType(), captureToolBins());
  if (!plans.length) {
    return {
      ok: false,
      previewing: false,
      error:
        "No silent capture tool. Install ffmpeg (pacman -S ffmpeg) or grim. Live view will not open a screenshot app.",
    };
  }
  previewOwner = who;
  previewGotFrame = false;
  const plan = plans[0];
  if (plan.name === "ffmpeg-x11grab") {
    startFfmpegPreview(process.env.DISPLAY || ":0");
    const verdict = await waitForPreviewReady(1500);
    if (previewOwner !== who) {
      return { ok: true, previewing: isPreviewing(), owner: previewOwner };
    }
    if (verdict === "ready" || verdict === "timeout-alive") {
      return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner, capture: "ffmpeg-x11grab" };
    }
    stopFfmpegPreview();
    const next = nextPreviewPlan(plans, "ffmpeg-x11grab");
    if (next) {
      previewSkipCapture = "ffmpeg-x11grab";
      startTimerPreview(ms);
      return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner, capture: next.name };
    }
    previewOwner = null;
    return {
      ok: false,
      previewing: false,
      error:
        "ffmpeg x11grab exited before the first frame. Install grim (Wayland) or check DISPLAY.",
    };
  }
  startTimerPreview(ms);
  return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner, capture: plan.name };
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
      capture: shot.capture,
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
  let follow = null;
  try {
    await sleep(90);
    follow = await captureScreenshot();
  } catch {
    follow = null;
  }
  return {
    ok: true,
    op,
    injector: inj.kind,
    mapped: op === "type" || op === "key" || op === "scroll" ? undefined : mapped,
    dataUrl: follow && follow.ok ? follow.dataUrl : undefined,
    screen: follow && follow.ok ? follow.screen : undefined,
    screenshot: follow && follow.ok ? follow.screenshot : undefined,
  };
}

async function beginSession() {
  aborted = false;
  showStopBar();
  if (!isPreviewing()) await startPreview(450, "session");
  return { ok: true, ...info(), previewing: isPreviewing() };
}

function endSession() {
  stopPreview({ reason: "session" });
  hideStopBar();
  return { ok: true, previewing: isPreviewing() };
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
  preferredInjectorKind,
  silentCaptureTools,
  restoreHiddenWindow,
  ydotoolScrollArgs,
  startPreview,
  stopPreview,
  isPreviewing,
  ffmpegX11grabArgs,
  ffmpegMjpegArgs,
  splitJpegFrames,
  previewAllowsDesktopCapturer,
  previewCaptureKind,
  mapToScreen,
  injectorScreenSize,
  shotMetaForMapping,
  pointHitsScaledBounds,
  previewStartVerdict,
  nextPreviewPlan,
  filterCapturePlans,
  whichSync,
  vendorBinDirs,
  parseXdpyinfoDimensions,
  parseXrandrCurrentDimensions,
  unionDisplayBounds,
};
