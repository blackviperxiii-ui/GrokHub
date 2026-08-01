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
    model?: string;
    apiKey?: string;
    accessToken?: string;
    tokens?: XaiOAuthTokens | null;
  }) => Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }>;
  chatStream?: (
    payload: {
      messages: GrokChatMessage[];
      mode?: GrokModeId;
      model?: string;
      apiKey?: string;
      accessToken?: string;
      tokens?: XaiOAuthTokens | null;
    },
    handlers: {
      onDelta: (piece: string) => void;
      onStatus?: (status: string) => void;
      signal?: AbortSignal;
    },
  ) => Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }>;
  stopChatStream?: (streamId?: string) => void;
  imagine?: (payload: {
    prompt: string;
    apiKey?: string;
    accessToken?: string;
    tokens?: XaiOAuthTokens | null;
  }) => Promise<{
    ok: boolean;
    imageDataUrl?: string;
    model?: string;
    source?: string;
    error?: string;
  }>;
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
  applyUpdate?: (opts?: {
    token?: string;
    force?: boolean;
    restart?: boolean;
  }) => Promise<UpdateResult>;
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
    headers: { "content-type": "application/json", accept: "application/json" },
    body: JSON.stringify({ action, ...body }),
  });
  const text = await res.text();
  // Production bug guard: SPA HTML fallback means the host API route is missing
  if (
    text.trimStart().startsWith("<!DOCTYPE") ||
    text.trimStart().startsWith("<html") ||
    (res.headers.get("content-type") || "").includes("text/html")
  ) {
    throw new Error(
      "Host API returned HTML instead of JSON — desktop bridge is offline. Relaunch GrokHub (Electron) or update to a build that includes /api/host.",
    );
  }
  let data: T & { error?: string };
  try {
    data = JSON.parse(text) as T & { error?: string };
  } catch {
    throw new Error(`Host API invalid JSON (${res.status}): ${text.slice(0, 160)}`);
  }
  if (!res.ok) {
    throw new Error(data.error || `host rpc ${res.status}`);
  }
  if (data && typeof data === "object" && "error" in data && data.error) {
    throw new Error(String(data.error));
  }
  return data;
}

export async function hostInfo(): Promise<HostInfo> {
  const e = electronHost();
  if (e?.info) {
    try {
      const info = await e.info();
      return { ...info, bridge: "electron", unsandboxed: true };
    } catch (err) {
      // Fall through to HTTP — surface IPC failure if that also fails
      console.warn("[host] electron info failed, trying HTTP", err);
    }
  }
  try {
    return await rpc<HostInfo>("info");
  } catch (err) {
    const msg = err instanceof Error ? err.message : "host unavailable";
    console.error("[host]", msg);
    return {
      platform: typeof navigator !== "undefined" ? navigator.platform : "unknown",
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
