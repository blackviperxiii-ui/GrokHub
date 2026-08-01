/**
 * Unsandboxed host operations for Electron main process.
 * Runs as the desktop user — full filesystem, shell, and app launch.
 */
const { exec, spawn } = require("node:child_process");
const { promisify } = require("node:util");
const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { shell } = require("electron");

const execAsync = promisify(exec);
const MAX_STDOUT = 200_000;

function clip(s, max = MAX_STDOUT) {
  if (!s) return "";
  if (s.length <= max) return s;
  return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}

async function info() {
  return {
    platform: process.platform,
    arch: process.arch,
    homedir: os.homedir(),
    cwd: process.cwd(),
    user: os.userInfo().username,
    shell: process.env.SHELL || "/bin/bash",
    hostname: os.hostname(),
    bridge: "electron",
    unsandboxed: true,
  };
}

async function listDir(dirPath) {
  const target = path.resolve(dirPath || os.homedir());
  const names = await fs.readdir(target);
  const entries = [];
  for (const name of names.slice(0, 500)) {
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
      cwd: cwd || process.cwd(),
      command: "",
      ms: 0,
    };
  }
  const workdir = cwd ? path.resolve(cwd) : process.cwd();
  const started = Date.now();
  try {
    const { stdout, stderr } = await execAsync(cmd, {
      cwd: workdir,
      timeout: timeoutMs,
      maxBuffer: MAX_STDOUT,
      shell: process.env.SHELL || "/bin/bash",
      env: process.env,
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
      code: typeof err.code === "number" ? err.code : 1,
      stdout: clip(String(err.stdout || "")),
      stderr: clip(String(err.stderr || err.message || "exec failed")),
      cwd: workdir,
      command: cmd,
      ms: Date.now() - started,
    };
  }
}

async function listApps() {
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
    for (const file of files.slice(0, 400)) {
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
          id: file,
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
    .slice(0, 400);
}

async function openApp(opts = {}) {
  try {
    if (opts.path) {
      const err = await shell.openPath(opts.path);
      if (err) {
        spawn("xdg-open", [opts.path], { detached: true, stdio: "ignore" }).unref();
      }
      return { ok: true, detail: `opened path ${opts.path}` };
    }
    if (opts.desktopFile) {
      spawn("gtk-launch", [path.basename(opts.desktopFile, ".desktop")], {
        detached: true,
        stdio: "ignore",
        env: process.env,
      }).unref();
      spawn("xdg-open", [opts.desktopFile], {
        detached: true,
        stdio: "ignore",
        env: process.env,
      }).unref();
      return { ok: true, detail: `launched ${opts.desktopFile}` };
    }
    if (opts.exec) {
      spawn(opts.exec, {
        shell: true,
        detached: true,
        stdio: "ignore",
        env: process.env,
      }).unref();
      return { ok: true, detail: `exec ${opts.exec}` };
    }
    return { ok: false, detail: "no target" };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "open failed" };
  }
}

module.exports = {
  info,
  listDir,
  readFile,
  writeFile,
  runExec,
  listApps,
  openApp,
};
