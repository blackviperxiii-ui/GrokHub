/**
 * Unsandboxed host operations for Electron main process.
 * Runs as the desktop user — full filesystem, shell, and app launch.
 * Supports Linux and Windows (PowerShell / cmd).
 */
const { exec, spawn } = require("node:child_process");
const { promisify } = require("node:util");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { shell } = require("electron");

const isWin = process.platform === "win32";

const execAsync = promisify(exec);
const MAX_STDOUT = 200_000;
const MAX_TIMEOUT = 120_000;

function clip(s, max = MAX_STDOUT) {
  if (!s) return "";
  if (s.length <= max) return s;
  return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}

function defaultCwd() {
  try {
    return os.homedir() || process.cwd();
  } catch {
    return process.cwd();
  }
}

function defaultShell() {
  if (isWin) {
    return (
      process.env.ComSpec ||
      process.env.COMSPEC ||
      require("node:path").join(process.env.SystemRoot || "C:\Windows", "System32", "cmd.exe")
    );
  }
  return process.env.SHELL || "/bin/bash";
}

function hostEnv() {
  const home = process.env.HOME || process.env.USERPROFILE || os.homedir();
  const base = {
    ...process.env,
    GROKHUB_HOST: "1",
    HOME: home,
    USERPROFILE: process.env.USERPROFILE || home,
    USER:
      process.env.USER ||
      process.env.USERNAME ||
      (() => {
        try {
          return os.userInfo().username;
        } catch {
          return "user";
        }
      })(),
    USERNAME: process.env.USERNAME || process.env.USER || "user",
    SHELL: defaultShell(),
    LANG: process.env.LANG || "en_US.UTF-8",
  };
  if (isWin) {
    base.PATH =
      process.env.PATH ||
      process.env.Path ||
      "C:\Windows\System32;C:\Windows;C:\Windows\System32\WindowsPowerShell\v1.0";
    return base;
  }
  base.PATH =
    process.env.PATH ||
    "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
  base.DISPLAY = process.env.DISPLAY || ":0";
  base.WAYLAND_DISPLAY = process.env.WAYLAND_DISPLAY || "";
  base.XDG_RUNTIME_DIR =
    process.env.XDG_RUNTIME_DIR ||
    `/run/user/${typeof process.getuid === "function" ? process.getuid() : 1000}`;
  base.DBUS_SESSION_BUS_ADDRESS = process.env.DBUS_SESSION_BUS_ADDRESS || "";
  return base;
}


async function info() {
  return {
    platform: process.platform,
    arch: process.arch,
    homedir: os.homedir(),
    cwd: defaultCwd(),
    user: (() => {
      try {
        return os.userInfo().username;
      } catch {
        return process.env.USER || "user";
      }
    })(),
    shell: defaultShell(),
    windows: isWin,
    hostname: os.hostname(),
    bridge: "electron",
    unsandboxed: true,
  };
}

async function listDir(dirPath) {
  const target = path.resolve(dirPath || os.homedir());
  const names = await fs.readdir(target);
  const entries = [];
  for (const name of names.slice(0, 800)) {
    const full = path.join(target, name);
    try {
      const st = await fs.stat(full);
      entries.push({
        name,
        path: full,
        isDir: st.isDirectory(),
        size: st.size,
        mtimeMs: st.mtimeMs,
      });
    } catch {
      /* skip */
    }
  }
  entries.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  return { path: target, entries };
}

async function readFile(filePath, maxBytes = 256_000) {
  const target = path.resolve(filePath);
  const buf = await fs.readFile(target);
  const truncated = buf.length > maxBytes;
  return {
    path: target,
    content: buf.subarray(0, maxBytes).toString("utf8"),
    truncated,
  };
}

async function writeFile(filePath, content) {
  const target = path.resolve(filePath);
  await fs.mkdir(path.dirname(target), { recursive: true });
  await fs.writeFile(target, content, "utf8");
  return { path: target, bytes: Buffer.byteLength(content, "utf8") };
}

async function runExec(command, cwd, timeoutMs = 30_000) {
  const cmd = String(command || "").trim();
  if (!cmd) {
    return {
      ok: false,
      code: 1,
      stdout: "",
      stderr: "empty command",
      cwd: cwd || defaultCwd(),
      command: "",
      ms: 0,
    };
  }
  const workdir = cwd ? path.resolve(cwd) : defaultCwd();
  try {
    await fs.mkdir(workdir, { recursive: true });
  } catch {
    /* ignore */
  }
  const started = Date.now();
  const timeout = Math.min(Math.max(timeoutMs || 30_000, 1_000), MAX_TIMEOUT);
  let execCmd = cmd;
  let shellBin = process.env.SHELL || "/bin/bash";
  if (isWin) {
    shellBin = "powershell.exe";
    execCmd = [
      "-NoProfile",
      "-NonInteractive",
      "-ExecutionPolicy",
      "Bypass",
      "-Command",
      cmd,
    ].join(" ");
  }
  try {
    const { stdout, stderr } = await execAsync(execCmd, {
      cwd: workdir,
      timeout,
      maxBuffer: MAX_STDOUT,
      shell: shellBin,
      windowsHide: true,
      env: hostEnv(),
    });
    return {
      ok: true,
      code: 0,
      stdout: clip(String(stdout || "")),
      stderr: clip(String(stderr || "")),
      cwd: workdir,
      command: cmd,
      ms: Date.now() - started,
    };
  } catch (err) {
    return {
      ok: false,
      code: typeof err.code === "number" ? err.code : err.killed ? 124 : 1,
      stdout: clip(String(err.stdout || "")),
      stderr: clip(
        String(
          err.stderr ||
            (err.killed ? `command timed out after ${timeout}ms` : err.message || "exec failed"),
        ),
      ),
      cwd: workdir,
      command: cmd,
      ms: Date.now() - started,
    };
  }
}

async function listAppsWindows() {
  const apps = [];
  const roots = [
    path.join(process.env.ProgramData || "C:\\ProgramData", "Microsoft", "Windows", "Start Menu", "Programs"),
    path.join(process.env.APPDATA || path.join(os.homedir(), "AppData", "Roaming"), "Microsoft", "Windows", "Start Menu", "Programs"),
  ];
  async function walk(dir, depth) {
    if (depth > 4) return;
    let names = [];
    try { names = await fs.readdir(dir); } catch { return; }
    for (const name of names.slice(0, 400)) {
      const full = path.join(dir, name);
      try {
        const st = await fs.stat(full);
        if (st.isDirectory()) { await walk(full, depth + 1); continue; }
        if (!/\.(lnk|exe)$/i.test(name)) continue;
        apps.push({
          id: full,
          name: name.replace(/\.(lnk|exe)$/i, ""),
          exec: full,
          desktopFile: full,
          terminal: false,
        });
      } catch { /* skip */ }
    }
  }
  for (const r of roots) await walk(r, 0);
  return apps;
}

async function listApps() {
  if (isWin) {
    const apps = await listAppsWindows();
    apps.sort((a, b) => a.name.localeCompare(b.name));
    const seen = new Set();
    return apps.filter((a) => {
      const k = a.name.toLowerCase();
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    }).slice(0, 500);
  }
  const dirs = [
    "/usr/share/applications",
    "/usr/local/share/applications",
    path.join(os.homedir(), ".local/share/applications"),
  ];
  const apps = [];
  for (const dir of dirs) {
    let files = [];
    try {
      files = (await fs.readdir(dir)).filter((f) => f.endsWith(".desktop"));
    } catch {
      continue;
    }
    for (const file of files.slice(0, 500)) {
      const desktopFile = path.join(dir, file);
      try {
        const raw = await fs.readFile(desktopFile, "utf8");
        if (/^NoDisplay\s*=\s*true/im.test(raw)) continue;
        if (/^Hidden\s*=\s*true/im.test(raw)) continue;
        const name = (raw.match(/^Name\s*=\s*(.+)$/m) || [])[1]?.trim() || file;
        const execLine = (raw.match(/^Exec\s*=\s*(.+)$/m) || [])[1]?.trim() || "";
        const terminal = /^Terminal\s*=\s*true/im.test(raw);
        const execCmd = execLine.replace(/\s+%[a-zA-Z]/g, "").trim();
        if (!execCmd) continue;
        apps.push({
          id: `${dir}:${file}`,
          name,
          exec: execCmd,
          desktopFile,
          terminal,
        });
      } catch {
        /* skip */
      }
    }
  }
  apps.sort((a, b) => a.name.localeCompare(b.name));
  const seen = new Set();
  return apps
    .filter((a) => {
      const k = a.name.toLowerCase();
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .slice(0, 500);
}

async function openApp(opts = {}) {
  try {
    if (opts.path) {
      const err = await shell.openPath(opts.path);
      if (err) {
        if (isWin) {
          spawn("cmd.exe", ["/c", "start", "", opts.path], {
            detached: true, stdio: "ignore", windowsHide: true, env: hostEnv(),
          }).unref();
        } else {
          spawn("xdg-open", [opts.path], {
            detached: true, stdio: "ignore", env: hostEnv(),
          }).unref();
        }
      }
      return { ok: true, detail: `opened path ${opts.path}` };
    }
    if (opts.desktopFile) {
      if (isWin) {
        const err = await shell.openPath(opts.desktopFile);
        if (err) {
          spawn("cmd.exe", ["/c", "start", "", opts.desktopFile], {
            detached: true, stdio: "ignore", windowsHide: true, env: hostEnv(),
          }).unref();
        }
        return { ok: true, detail: `launched ${opts.desktopFile}` };
      }
      const base = path.basename(opts.desktopFile, ".desktop");
      spawn("gtk-launch", [base], {
        detached: true, stdio: "ignore", env: hostEnv(),
      }).unref();
      spawn("xdg-open", [opts.desktopFile], {
        detached: true, stdio: "ignore", env: hostEnv(),
      }).unref();
      return { ok: true, detail: `launched ${opts.desktopFile}` };
    }
    if (opts.exec) {
      spawn(opts.exec, {
        shell: true,
        detached: true,
        stdio: "ignore",
        windowsHide: true,
        env: hostEnv(),
      }).unref();
      return { ok: true, detail: `exec ${opts.exec}` };
    }
    return { ok: false, detail: "no target" };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "open failed" };
  }
}


async function walkSkillMds(root, relBase, depth, out) {
  if (depth > 6 || out.length >= 200) return;
  let names;
  try {
    names = await fs.readdir(root);
  } catch {
    return;
  }
  for (const name of names) {
    if (name.startsWith(".") || name === "node_modules") continue;
    const full = path.join(root, name);
    const rel = relBase ? `${relBase}/${name}` : name;
    let st;
    try {
      st = await fs.stat(full);
    } catch {
      continue;
    }
    if (st.isDirectory()) {
      await walkSkillMds(full, rel, depth + 1, out);
    } else if (st.isFile() && /^skill\.md$/i.test(name) && st.size < 512000) {
      try {
        const content = await fs.readFile(full, "utf8");
        out.push({ dirName: path.basename(path.dirname(full)), relativePath: rel, content });
      } catch {
        /* skip */
      }
    }
  }
}

async function readOpenClawWorkspace(dirPath) {
  const home = os.homedir();
  const candidates = [
    path.join(home, ".openclaw", "workspace"),
    path.join(home, ".openclaw", "workspace-default"),
    path.join(home, "openclaw", "workspace"),
  ];
  let target = dirPath && String(dirPath).trim()
    ? path.resolve(String(dirPath).trim().replace(/^~(?=\/|$)/, home))
    : "";
  if (!target) {
    for (const c of candidates) {
      try {
        const st = await fs.stat(c);
        if (st.isDirectory()) {
          target = c;
          break;
        }
      } catch {
        /* next */
      }
    }
  }
  if (!target) {
    return {
      ok: false,
      error: "No OpenClaw workspace found. Pass a path or create ~/.openclaw/workspace",
      root: "",
      files: [],
      skills: [],
      candidates,
    };
  }
  let st;
  try {
    st = await fs.stat(target);
  } catch {
    return { ok: false, error: `Path not found: ${target}`, root: target, files: [], skills: [], candidates };
  }
  if (!st.isDirectory()) {
    return { ok: false, error: `Not a directory: ${target}`, root: target, files: [], skills: [], candidates };
  }
  const files = [];
  const core = ["AGENTS.md","SOUL.md","USER.md","IDENTITY.md","TOOLS.md","HEARTBEAT.md","MEMORY.md","BOOT.md","BOOTSTRAP.md"];
  for (const name of core) {
    try {
      const buf = await fs.readFile(path.join(target, name));
      if (buf.length > 400000) continue;
      files.push({ name, relativePath: name, content: buf.toString("utf8") });
    } catch {
      /* missing */
    }
  }
  try {
    const memDir = path.join(target, "memory");
    const memNames = await fs.readdir(memDir);
    const days = memNames.filter((n) => /^\d{4}-\d{2}-\d{2}\.md$/.test(n)).sort().reverse().slice(0, 3);
    for (const n of days) {
      try {
        const buf = await fs.readFile(path.join(memDir, n));
        if (buf.length > 200000) continue;
        files.push({ name: n, relativePath: `memory/${n}`, content: buf.toString("utf8") });
      } catch {
        /* skip */
      }
    }
  } catch {
    /* no memory */
  }
  const skills = [];
  for (const skillRoot of [path.join(target, "skills"), path.join(target, ".agents", "skills")]) {
    await walkSkillMds(skillRoot, path.relative(target, skillRoot) || "skills", 0, skills);
  }
  if (!skills.length) {
    await walkSkillMds(path.join(home, ".openclaw", "skills"), "managed-skills", 0, skills);
  }
  let configHint = null;
  try {
    const buf = await fs.readFile(path.join(home, ".openclaw", "openclaw.json"));
    configHint = buf.subarray(0, 80000).toString("utf8");
  } catch {
    configHint = null;
  }
  return { ok: true, root: target, files, skills, configHint, candidates };
}


module.exports = {
  info,
  listDir,
  readFile,
  writeFile,
  runExec,
  listApps,
  openApp,
  readOpenClawWorkspace,
};
