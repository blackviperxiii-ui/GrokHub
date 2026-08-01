import type {
  HostApp,
  HostExecResult,
  HostFileEntry,
  HostInfo,
} from "./host-types";
import type { GrokChatMessage, GrokChatResult } from "./grok";
import type { GrokModeId } from "./types";
import type { UpdateResult, UpdateStatus } from "./update";
import type { DeviceCodeStart, PollResult, XaiOAuthTokens } from "./xai-oauth";

/**
 * Client host bridge.
 * 1) Electron unsandboxed IPC when packaged
 * 2) POST /api/host JSON RPC (Vite middleware / production adapter)
 */

export type DesktopGrokBridge = {
  chat?: (payload: {
    messages: GrokChatMessage[];
    mode?: GrokModeId;
    apiKey?: string;
    accessToken?: string;
    tokens?: XaiOAuthTokens | null;
  }) => Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }>;
  probe?: (
    apiKey?: string,
    accessToken?: string,
  ) => Promise<{ ok: boolean; detail: string; envConfigured?: boolean; authMode?: string }>;
  oauthStart?: () => Promise<DeviceCodeStart & { ok: boolean }>;
  oauthPoll?: (deviceCode: string) => Promise<PollResult>;
  oauthEnsure?: (tokens: XaiOAuthTokens) => Promise<{
    ok: boolean;
    detail: string;
    refreshed: boolean;
    tokens: XaiOAuthTokens;
  }>;
  checkUpdate?: (opts?: { token?: string }) => Promise<UpdateStatus>;
  applyUpdate?: (opts?: { token?: string }) => Promise<UpdateResult>;
};

export type DesktopBridge = {
  minimize?: () => void;
  maximize?: () => void;
  close?: () => void;
  fit?: () => void;
  platform?: string;
  host?: {
    info: () => Promise<HostInfo>;
    listDir: (p?: string) => Promise<{ path: string; entries: HostFileEntry[] }>;
    readFile: (
      p: string,
      maxBytes?: number,
    ) => Promise<{ path: string; content: string; truncated: boolean }>;
    writeFile: (p: string, content: string) => Promise<{ path: string; bytes: number }>;
    exec: (command: string, cwd?: string, timeoutMs?: number) => Promise<HostExecResult>;
    listApps: () => Promise<HostApp[]>;
    openApp: (opts: {
      exec?: string;
      desktopFile?: string;
      path?: string;
    }) => Promise<{ ok: boolean; detail: string }>;
  };
  grok?: DesktopGrokBridge;
};

declare global {
  interface Window {
    grokhubDesktop?: DesktopBridge;
  }
}

function electronHost() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.host : undefined;
}

async function rpc<T>(action: string, body: Record<string, unknown> = {}): Promise<T> {
  const res = await fetch("/api/host", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ action, ...body }),
  });
  const data = (await res.json()) as T & { error?: string };
  if (!res.ok) {
    throw new Error((data as { error?: string }).error || `host rpc ${res.status}`);
  }
  if (data && typeof data === "object" && "error" in data && (data as { error?: string }).error) {
    throw new Error(String((data as { error?: string }).error));
  }
  return data;
}

export async function hostInfo(): Promise<HostInfo> {
  const e = electronHost();
  if (e?.info) {
    const info = await e.info();
    return { ...info, bridge: "electron", unsandboxed: true };
  }
  try {
    return await rpc<HostInfo>("info");
  } catch {
    return {
      platform: "unknown",
      arch: "unknown",
      homedir: "~",
      cwd: ".",
      user: "user",
      shell: "/bin/bash",
      hostname: "local",
      bridge: "none",
      unsandboxed: false,
    };
  }
}

export async function hostListDir(p?: string) {
  const e = electronHost();
  if (e?.listDir) return e.listDir(p);
  return rpc<{ path: string; entries: HostFileEntry[] }>("listDir", { path: p });
}

export async function hostReadFile(p: string, maxBytes?: number) {
  const e = electronHost();
  if (e?.readFile) return e.readFile(p, maxBytes);
  return rpc<{ path: string; content: string; truncated: boolean }>("readFile", {
    path: p,
    maxBytes,
  });
}

export async function hostWriteFile(p: string, content: string) {
  const e = electronHost();
  if (e?.writeFile) return e.writeFile(p, content);
  return rpc<{ path: string; bytes: number }>("writeFile", { path: p, content });
}

export async function hostExec(command: string, cwd?: string, timeoutMs?: number) {
  const e = electronHost();
  if (e?.exec) return e.exec(command, cwd, timeoutMs);
  return rpc<HostExecResult>("exec", { command, cwd, timeoutMs });
}

export async function hostListApps() {
  const e = electronHost();
  if (e?.listApps) return e.listApps();
  return rpc<HostApp[]>("listApps");
}

export async function hostOpenApp(opts: {
  exec?: string;
  desktopFile?: string;
  path?: string;
}) {
  const e = electronHost();
  if (e?.openApp) return e.openApp(opts);
  return rpc<{ ok: boolean; detail: string }>("openApp", opts);
}

export function isDesktopShell(): boolean {
  return typeof window !== "undefined" && Boolean(window.grokhubDesktop);
}
