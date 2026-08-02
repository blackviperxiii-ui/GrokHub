/**
 * Shared host handlers used by Vite middleware (preview), production Nitro,
 * and Electron main (via host-bridge). Always runs in Node — never import from
 * React client components.
 *
 * Unsandboxed: full shell, filesystem, and app launch as the process user.
 */
import { exec as execCb, spawn } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { HostApp, HostExecResult, HostFileEntry, HostInfo } from "./host-types";
import type { OpenClawWorkspaceRaw } from "./openclaw-import";
import { defaultOpenClawPaths } from "./openclaw-import";

const execAsync = promisify(execCb);
const MAX_STDOUT = 2_000_000;
const MAX_TIMEOUT = 300_000;

function clip(s: string, max = MAX_STDOUT): string {
  if (!s) return "";
  if (s.length <= max) return s;
  return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}

/** Prefer home dir as the user-facing workspace, not the app install path. */
function defaultCwd(): string {
  try {
    return os.homedir() || process.cwd();
  } catch {
    return process.cwd();
  }
}

function hostEnv(): NodeJS.ProcessEnv {
  return {
    ...process.env,
    GROKHUB_HOST: "1",
    // Ensure typical desktop PATH even if launched from a minimal .desktop env
    PATH:
      process.env.PATH ||
      "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
    HOME: process.env.HOME || os.homedir(),
    USER: process.env.USER || os.userInfo().username,
    SHELL: process.env.SHELL || "/bin/bash",
    LANG: process.env.LANG || "en_US.UTF-8",
    DISPLAY: process.env.DISPLAY || ":0",
    WAYLAND_DISPLAY: process.env.WAYLAND_DISPLAY || "",
    XDG_RUNTIME_DIR: process.env.XDG_RUNTIME_DIR || `/run/user/${process.getuid?.() ?? 1000}`,
    DBUS_SESSION_BUS_ADDRESS: process.env.DBUS_SESSION_BUS_ADDRESS || "",
  };
}

export async function handleHostInfo(): Promise<HostInfo> {
  return {
    platform: process.platform,
    arch: process.arch,
    homedir: os.homedir(),
    cwd: defaultCwd(),
    user: os.userInfo().username,
    shell: process.env.SHELL || "/bin/bash",
    hostname: os.hostname(),
    bridge: "server",
    unsandboxed: true,
  };
}

export async function handleListDir(dirPath?: string): Promise<{
  path: string;
  entries: HostFileEntry[];
}> {
  const target = path.resolve(dirPath || os.homedir());
  const names = await fs.readdir(target);
  const entries: HostFileEntry[] = [];
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
      /* skip unreadable */
    }
  }
  entries.sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
  return { path: target, entries };
}

export async function handleReadFile(
  filePath: string,
  maxBytes = 256_000,
): Promise<{ path: string; content: string; truncated: boolean }> {
  const target = path.resolve(filePath);
  const buf = await fs.readFile(target);
  return {
    path: target,
    content: buf.subarray(0, maxBytes).toString("utf8"),
    truncated: buf.length > maxBytes,
  };
}

export async function handleWriteFile(
  filePath: string,
  content: string,
): Promise<{ path: string; bytes: number }> {
  const target = path.resolve(filePath);
  await fs.mkdir(path.dirname(target), { recursive: true });
  await fs.writeFile(target, content, "utf8");
  return { path: target, bytes: Buffer.byteLength(content, "utf8") };
}

export async function handleExec(
  command: string,
  cwd?: string,
  timeoutMs = 30_000,
): Promise<HostExecResult> {
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
  // Ensure workdir exists
  try {
    await fs.mkdir(workdir, { recursive: true });
  } catch {
    /* ignore */
  }
  const started = Date.now();
  const timeout = Math.min(Math.max(timeoutMs || 30_000, 1_000), MAX_TIMEOUT);
  const shell = process.env.SHELL || "/bin/bash";
  try {
    // Run via login-capable shell so PATH / aliases match a real user terminal
    const { stdout, stderr } = await execAsync(cmd, {
      cwd: workdir,
      timeout,
      maxBuffer: MAX_STDOUT,
      shell,
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
  } catch (err: unknown) {
    const e = err as {
      code?: number | string;
      killed?: boolean;
      signal?: string;
      stdout?: string | Buffer;
      stderr?: string | Buffer;
      message?: string;
    };
    const msg = String(e.message || "exec failed");
    const maxBuf = /maxBuffer|ERR_CHILD_PROCESS_STDIO_MAXBUFFER/i.test(msg);
    const timedOut = Boolean(e.killed) || /timed out/i.test(msg);
    const code =
      typeof e.code === "number"
        ? e.code
        : timedOut
          ? 124
          : typeof e.code === "string"
            ? 1
            : 1;
    return {
      ok: false,
      code,
      stdout: clip(String(e.stdout || "")),
      stderr: clip(
        String(
          e.stderr ||
            (timedOut
              ? `command timed out after ${timeout}ms`
              : maxBuf
                ? `output exceeded buffer — partial stdout returned`
                : msg),
        ),
      ),
      cwd: workdir,
      command: cmd,
      ms: Date.now() - started,
    };
  }
}

export async function handleListApps(): Promise<HostApp[]> {
  const dirs = [
    "/usr/share/applications",
    "/usr/local/share/applications",
    path.join(os.homedir(), ".local/share/applications"),
  ];
  const apps: HostApp[] = [];
  for (const dir of dirs) {
    let files: string[] = [];
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
        if (/^Type\s*=\s*(?!Application)/im.test(raw) && /^Type\s*=/m.test(raw)) {
          if (!/^Type\s*=\s*Application/im.test(raw)) continue;
        }
        const name = raw.match(/^Name\s*=\s*(.+)$/m)?.[1]?.trim() || file;
        const execLine = raw.match(/^Exec\s*=\s*(.+)$/m)?.[1]?.trim() || "";
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
  const seen = new Set<string>();
  return apps
    .filter((a) => {
      const k = a.name.toLowerCase();
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    })
    .slice(0, 500);
}

export async function handleOpenApp(opts: {
  exec?: string;
  desktopFile?: string;
  path?: string;
}): Promise<{ ok: boolean; detail: string }> {
  try {
    if (opts.path) {
      spawn("xdg-open", [opts.path], {
        detached: true,
        stdio: "ignore",
        env: hostEnv(),
      }).unref();
      return { ok: true, detail: `opened path ${opts.path}` };
    }
    if (opts.desktopFile) {
      const base = path.basename(opts.desktopFile, ".desktop");
      // Prefer gtk-launch, fall back to xdg-open / gio
      spawn("gtk-launch", [base], {
        detached: true,
        stdio: "ignore",
        env: hostEnv(),
      }).unref();
      spawn("xdg-open", [opts.desktopFile], {
        detached: true,
        stdio: "ignore",
        env: hostEnv(),
      }).unref();
      return { ok: true, detail: `launched ${opts.desktopFile}` };
    }
    if (opts.exec) {
      spawn(opts.exec, {
        shell: true,
        detached: true,
        stdio: "ignore",
        env: hostEnv(),
      }).unref();
      return { ok: true, detail: `exec ${opts.exec}` };
    }
    return { ok: false, detail: "no target" };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "open failed" };
  }
}

async function walkSkillMds(
  root: string,
  relBase: string,
  depth: number,
  out: { dirName: string; relativePath: string; content: string }[],
): Promise<void> {
  if (depth > 6 || out.length >= 200) return;
  let names: string[];
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
    } else if (st.isFile() && /^skill\.md$/i.test(name) && st.size < 512_000) {
      try {
        const content = await fs.readFile(full, "utf8");
        const dirName = path.basename(path.dirname(full));
        out.push({ dirName, relativePath: rel, content });
      } catch {
        /* skip */
      }
    }
  }
}

/** Read an OpenClaw workspace directory into a portable payload. */
export async function handleReadOpenClawWorkspace(
  dirPath?: string,
): Promise<OpenClawWorkspaceRaw & { ok: boolean; error?: string; candidates?: string[] }> {
  const home = os.homedir();
  const candidates = defaultOpenClawPaths(home);
  let target = dirPath?.trim()
    ? path.resolve(dirPath.trim().replace(/^~(?=\/|$)/, home))
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
      error:
        "No OpenClaw workspace found. Pass a path or create ~/.openclaw/workspace",
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
    return {
      ok: false,
      error: `Path not found: ${target}`,
      root: target,
      files: [],
      skills: [],
      candidates,
    };
  }
  if (!st.isDirectory()) {
    return {
      ok: false,
      error: `Not a directory: ${target}`,
      root: target,
      files: [],
      skills: [],
      candidates,
    };
  }

  const files: OpenClawWorkspaceRaw["files"] = [];
  const core = [
    "AGENTS.md",
    "SOUL.md",
    "USER.md",
    "IDENTITY.md",
    "TOOLS.md",
    "HEARTBEAT.md",
    "MEMORY.md",
    "BOOT.md",
    "BOOTSTRAP.md",
  ];
  for (const name of core) {
    const full = path.join(target, name);
    try {
      const buf = await fs.readFile(full);
      if (buf.length > 400_000) continue;
      files.push({
        name,
        relativePath: name,
        content: buf.toString("utf8"),
      });
    } catch {
      /* missing ok */
    }
  }

  try {
    const memDir = path.join(target, "memory");
    const memNames = await fs.readdir(memDir);
    const days = memNames
      .filter((n) => /^\d{4}-\d{2}-\d{2}\.md$/.test(n))
      .sort()
      .reverse()
      .slice(0, 3);
    for (const n of days) {
      try {
        const buf = await fs.readFile(path.join(memDir, n));
        if (buf.length > 200_000) continue;
        files.push({
          name: n,
          relativePath: `memory/${n}`,
          content: buf.toString("utf8"),
        });
      } catch {
        /* skip */
      }
    }
  } catch {
    /* no memory dir */
  }

  const skills: OpenClawWorkspaceRaw["skills"] = [];
  for (const skillRoot of [
    path.join(target, "skills"),
    path.join(target, ".agents", "skills"),
  ]) {
    await walkSkillMds(
      skillRoot,
      path.relative(target, skillRoot) || "skills",
      0,
      skills,
    );
  }

  if (skills.length === 0) {
    const managed = path.join(home, ".openclaw", "skills");
    await walkSkillMds(managed, "managed-skills", 0, skills);
  }

  let configHint: string | null = null;
  try {
    const cfg = path.join(home, ".openclaw", "openclaw.json");
    const buf = await fs.readFile(cfg);
    configHint = buf.subarray(0, 80_000).toString("utf8");
  } catch {
    configHint = null;
  }

  return {
    ok: true,
    root: target,
    files,
    skills,
    configHint,
    candidates,
  };
}

export async function dispatchHost(
  action: string,
  body: Record<string, unknown>,
): Promise<unknown> {
  switch (action) {
    case "info":
      return handleHostInfo();
    case "listDir":
      return handleListDir(body.path as string | undefined);
    case "readFile":
      return handleReadFile(
        String(body.path || ""),
        typeof body.maxBytes === "number" ? body.maxBytes : undefined,
      );
    case "writeFile":
      return handleWriteFile(String(body.path || ""), String(body.content ?? ""));
    case "exec":
      return handleExec(
        String(body.command || ""),
        body.cwd as string | undefined,
        typeof body.timeoutMs === "number" ? body.timeoutMs : undefined,
      );
    case "listApps":
      return handleListApps();
    case "openApp":
      return handleOpenApp({
        exec: body.exec as string | undefined,
        desktopFile: body.desktopFile as string | undefined,
        path: body.path as string | undefined,
      });
    case "readOpenClawWorkspace":
      return handleReadOpenClawWorkspace(
        body.path ? String(body.path) : undefined,
      );
    default:
      throw new Error(`unknown host action: ${action}`);
  }
}
