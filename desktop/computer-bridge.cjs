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

/** Split concatenated JPEGs from an ffmpeg image2pipe. */
function splitJpegFrames(onJpeg) {
  let buf = Buffer.alloc(0);
  return (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    while (true) {
      const soi = buf.indexOf(Buffer.from([0xff, 0xd8]));
      if (soi < 0) {
        buf = Buffer.alloc(0);
        return;
      }
      if (soi > 0) buf = buf.subarray(soi);
      const eoi = buf.indexOf(Buffer.from([0xff, 0xd9]), 2);
      if (eoi < 0) return;
      onJpeg(buf.subarray(0, eoi + 2));
      buf = buf.subarray(eoi + 2);
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
    const marker = buf[i + 1];
    if (marker === 0xc0 || marker === 0xc1 || marker === 0xc2) {
      return { height: buf.readUInt16BE(i + 5), width: buf.readUInt16BE(i + 7) };
    }
    if (marker === 0xd8 || marker === 0xd9) {
      i += 2;
      continue;
    }
    const len = buf.readUInt16BE(i + 2);
    i += 2 + len;
  }
  return { width: SCREENSHOT_MAX_WIDTH, height: 0 };
}

function previewAllowsDesktopCapturer() {
  return false;
}

/** Full-screen capture tools that do not open a picker / screenshot GUI. */
function silentCaptureTools(session, bins) {
  const b = bins || {};
  const out = [];
  if ((session === "wayland" || b.wayland) && b.grim) {
    out.push({ name: "grim", bin: b.grim, args: ["OUT"], ext: ".png" });
  }
  if (b.ffmpeg && (b.display || session === "x11")) {
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
  const plans = silentCaptureTools(inj.session, captureToolBins());
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
    capture: plans[0]?.name || null,
    captureTools: plans.map((p) => p.name),
    previewing: isPreviewing(),
    hint: buildHint(inj, plans),
  };
}

function captureToolBins() {
  const bounds = screenBounds();
  return {
    grim: whichSync("grim"),
    ffmpeg: whichSync("ffmpeg"),
    display: process.env.DISPLAY || "",
    width: bounds.width,
    height: bounds.height,
    maim: whichSync("maim"),
    scrot: whichSync("scrot"),
  };
}

function buildHint(inj, plans) {
  const bits = [];
  if (inj.kind === "ydotool") {
    bits.push("ydotool found — user may need to be in the input group (uinput).");
  } else if (inj.kind === "xdotool") {
    bits.push(
      inj.session === "wayland"
        ? "xdotool on Wayland only moves XWayland apps. Install ydotool + uinput for native windows."
        : "xdotool found.",
    );
  } else {
    bits.push("Install xdotool (X11/XWayland) or ydotool (Wayland) to inject mouse/keyboard.");
  }
  if (!plans.length) {
    bits.push(
      "Install ffmpeg (X11, same silent grab Cursor uses) or grim (Wayland). GrokHub will not open a screenshot app.",
    );
  } else if (plans[0].name === "ffmpeg-x11grab") {
    bits.push("Live view uses ffmpeg x11grab (no screenshot picker).");
  } else if (plans[0].name === "grim") {
    bits.push("Live view uses grim (no screenshot picker).");
  }
  return bits.join(" ");
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

function screenBounds() {
  try {
    const e = electronApis();
    if (e && e.screen) {
      const d = e.screen.getPrimaryDisplay();
      return { width: d.bounds.width, height: d.bounds.height, scaleFactor: d.scaleFactor || 1 };
    }
  } catch {
    /* ignore */
  }
  try {
    const xdotool = whichSync("xdotool");
    if (xdotool) {
      const out = execFileSync(xdotool, ["getdisplaygeometry"], {
        encoding: "utf8",
        timeout: 2000,
        env: process.env,
      });
      const m = String(out).trim().match(/^(\d+)\s+(\d+)/);
      if (m) return { width: Number(m[1]), height: Number(m[2]), scaleFactor: 1 };
    }
  } catch {
    /* ignore */
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
  const plans = silentCaptureTools(sess, captureToolBins());
  for (const plan of plans) {
    const ext = plan.ext || ".png";
    const tmp = path.join(os.tmpdir(), `grokhub-cap-${process.pid}-${Date.now()}${ext}`);
    const args = plan.args.map((a) => (a === "OUT" ? tmp : a));
    const r = await runTool(plan.bin, args, 12000);
    if (!r.ok) continue;
    try {
      if (!fs.existsSync(tmp) || fs.statSync(tmp).size < 80) continue;
      const encoded = encodeShotFile(tmp);
      try {
        fs.unlinkSync(tmp);
      } catch {
        /* ignore */
      }
      const bounds = screenBounds();
      lastShotMeta = {
        width: encoded.screenshot.width || bounds.width,
        height: encoded.screenshot.height || bounds.height,
        screenWidth: bounds.width,
        screenHeight: bounds.height,
      };
      return {
        ok: true,
        dataUrl: encoded.dataUrl,
        screen: bounds,
        screenshot: encoded.screenshot,
        capture: plan.name,
      };
    } catch {
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
  if (!previewProc) return;
  try {
    previewProc.kill("SIGTERM");
  } catch {
    /* ignore */
  }
  previewProc = null;
}

function emitJpegFrame(jpeg, capture) {
  const shot = jpegSize(jpeg);
  const bounds = screenBounds();
  lastShotMeta = {
    width: shot.width || SCREENSHOT_MAX_WIDTH,
    height: shot.height || lastShotMeta?.height || bounds.height,
    screenWidth: bounds.width,
    screenHeight: bounds.height,
  };
  emitFrame({
    ok: true,
    dataUrl: `data:image/jpeg;base64,${jpeg.toString("base64")}`,
    screen: bounds,
    screenshot: { width: lastShotMeta.width, height: lastShotMeta.height },
    capture,
  });
}

function startFfmpegPreview(display) {
  const bins = captureToolBins();
  const child = spawn("ffmpeg", ffmpegMjpegArgs(display, bins), {
    env: { ...process.env, DISPLAY: display },
    stdio: ["ignore", "pipe", "pipe"],
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
  return { ok: true, previewing: false };
}

function startPreview(intervalMs, owner) {
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
  const display = process.env.DISPLAY || "";
  const ffmpeg = whichSync("ffmpeg");
  if (ffmpeg && display && plans.some((p) => p.name === "ffmpeg-x11grab")) {
    startFfmpegPreview(display);
    return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner, capture: "ffmpeg-x11grab" };
  }
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
  return { ok: true, previewing: true, intervalMs: ms, owner: previewOwner, capture: plans[0].name };
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
  if (!isPreviewing()) startPreview(450, "session");
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
};
