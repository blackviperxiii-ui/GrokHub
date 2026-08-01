/**
 * Shared host handlers used by Vite middleware (preview) and Electron main.
 * Always runs in a Node process — never import from React client components.
 *
 * Intentionally unsandboxed: full shell, filesystem, and app launch as the
 * process user (root in this preview container; your user under Electron on Arch).
 */
import { exec as execCb, spawn } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import type { HostApp, HostExecResult, HostFileEntry, HostInfo } from "./host-types";

const execAsync = promisify(execCb);
const MAX_STDOUT = 200_000;
const MAX_TIMEOUT = 120_000;

function clip(s: string, max = MAX_STDOUT): string {
  if (!s) return "";
  if (s.length <= max) return s;
  return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}

export async function handleHostInfo(): Promise<HostInfo> {
  return {
    platform: process.platform,
    arch: process.arch,
    homedir: os.homedir(),
    cwd: process.cwd(),
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
      cwd: cwd || process.cwd(),
      command: "",
      ms: 0,
    };
  }
  const workdir = cwd ? path.resolve(cwd) : process.cwd();
  const started = Date.now();
  const timeout = Math.min(Math.max(timeoutMs || 30_000, 1_000), MAX_TIMEOUT);
  try {
    const { stdout, stderr } = await execAsync(cmd, {
      cwd: workdir,
      timeout,
      maxBuffer: MAX_STDOUT,
      shell: process.env.SHELL || "/bin/bash",
      env: { ...process.env, GROKCLAW_HOST: "1" },
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
      code?: number;
      stdout?: string;
      stderr?: string;
      message?: string;
      killed?: boolean;
    };
    return {
      ok: false,
      code: typeof e.code === "number" ? e.code : 1,
      stdout: clip(String(e.stdout || "")),
      stderr: clip(
        String(
          e.stderr ||
            e.message ||
            (e.killed ? "command timed out" : "exec failed"),
        ),
      ),
      cwd: workdir,
      command: cmd,
      ms: Date.now() - started,
    };
  }
}

async function ensureDemoDesktopEntries() {
  const dir = path.join(os.homedir(), ".local/share/applications");
  try {
    await fs.mkdir(dir, { recursive: true });
  } catch {
    return;
  }
  const entries: Record<string, string> = {
    "grokclaw-terminal.desktop": `[Desktop Entry]
Type=Application
Name=Host Terminal (bash)
Exec=bash -lc 'echo GrokClaw host shell; exec bash'
Terminal=true
Categories=System;TerminalEmulator;
`,
    "grokclaw-files.desktop": `[Desktop Entry]
Type=Application
Name=Home Files
Exec=xdg-open ${os.homedir()}
Terminal=false
Categories=System;FileManager;
`,
    "grokclaw-workspace.desktop": `[Desktop Entry]
Type=Application
Name=GrokClaw Workspace
Exec=xdg-open ${os.homedir()}
Terminal=false
Categories=Development;
`,
  };
  // Also pick up packaging desktop file if present
  try {
    const packaging = path.resolve(process.cwd(), "packaging/grokclaw.desktop");
    const raw = await fs.readFile(packaging, "utf8");
    entries["grokclaw.desktop"] = raw;
  } catch {
    /* optional */
  }
  for (const [file, body] of Object.entries(entries)) {
    const full = path.join(dir, file);
    try {
      await fs.access(full);
    } catch {
      try {
        await fs.writeFile(full, body, "utf8");
      } catch {
        /* ignore */
      }
    }
  }
}

export async function handleListApps(): Promise<HostApp[]> {
  await ensureDemoDesktopEntries();
  const dirs = [
    "/usr/share/applications",
    "/usr/local/share/applications",
    path.join(os.homedir(), ".local/share/applications"),
    path.resolve(process.cwd(), "packaging"),
  ];
  const apps: HostApp[] = [];
  for (const dir of dirs) {
    let files: string[] = [];
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
        const name = raw.match(/^Name\s*=\s*(.+)$/m)?.[1]?.trim() || file;
        const execLine = raw.match(/^Exec\s*=\s*(.+)$/m)?.[1]?.trim() || "";
        const terminal = /^Terminal\s*=\s*true/im.test(raw);
        const execCmd = execLine.replace(/\s+%[a-zA-Z]/g, "").trim();
        if (!execCmd) continue;
        apps.push({ id: `${dir}:${file}`, name, exec: execCmd, desktopFile, terminal });
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
    .slice(0, 300);
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
        env: process.env,
      }).unref();
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
        detached: true,
        stdio: "ignore",
        shell: true,
        env: process.env,
      }).unref();
      return { ok: true, detail: `exec ${opts.exec}` };
    }
    return { ok: false, detail: "no target" };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "open failed" };
  }
}

export async function dispatchHost(
  action: string,
  body: Record<string, unknown> = {},
): Promise<unknown> {
  switch (action) {
    case "info":
      return handleHostInfo();
    case "listDir":
      return handleListDir(body.path as string | undefined);
    case "readFile":
      return handleReadFile(String(body.path || ""), body.maxBytes as number | undefined);
    case "writeFile":
      return handleWriteFile(String(body.path || ""), String(body.content ?? ""));
    case "exec":
      return handleExec(
        String(body.command || ""),
        body.cwd as string | undefined,
        body.timeoutMs as number | undefined,
      );
    case "listApps":
      return handleListApps();
    case "openApp":
      return handleOpenApp({
        exec: body.exec as string | undefined,
        desktopFile: body.desktopFile as string | undefined,
        path: body.path as string | undefined,
      });
    default:
      throw new Error(`unknown host action: ${action}`);
  }
}
