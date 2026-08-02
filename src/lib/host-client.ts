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
    factory?: boolean;
  }) => Promise<UpdateResult>;
  factoryReinstall?: (opts?: {
    token?: string;
    wipeMemory?: boolean;
    clearSelfMod?: boolean;
    restart?: boolean;
  }) => Promise<{ ok?: boolean; detail?: string; steps?: string[]; error?: string }>;
  /** grok.com website SSO for weekly SuperGrok usage */
  linkWebsiteSession?: () => Promise<{
    cookie?: string;
    cookieHeader?: string;
    error?: string;
    names?: string[];
  }>;
  getWebsiteSso?: () => Promise<{ cookie?: string; signedIn?: boolean }>;
  injectWebsiteCookie?: (
    raw: string,
  ) => Promise<{ ok?: boolean; cookie?: string; error?: string }>;
  websiteUsage?: (opts: {
    ssoCookie?: string;
    bearer?: string;
  }) => Promise<import("./grok-website-usage").GrokWebsiteUsage>;
  websiteConnectors?: (opts: {
    ssoCookie?: string;
    bearer?: string;
  }) => Promise<{
    ok: boolean;
    connectors: Array<{
      id: string;
      name: string;
      accountLabel?: string | null;
      status: string;
    }>;
    detail?: string;
  }>;
};

export type DesktopBridge = {
  minimize?: () => void;
  maximize?: () => void;
  close?: () => void;
  fit?: () => void;
  platform?: string;
  secrets?: {
    set: (key: string, value: string) => Promise<{ ok?: boolean }>;
    get: (key: string) => Promise<{ value?: string }>;
    delete: (key: string) => Promise<{ ok?: boolean }>;
  };
  state?: {
    get: (name: string) => Promise<{ value?: string | null; path?: string; updatedAt?: number }>;
    set: (name: string, value: string) => Promise<{ ok?: boolean; path?: string }>;
    remove: (name: string) => Promise<{ ok?: boolean }>;
    info: () => Promise<{
      path?: string;
      userData?: string;
      bytes?: number;
      updatedAt?: number;
      keys?: string[];
    }>;
    exportAll: () => Promise<unknown>;
    importAll: (payload: string | unknown) => Promise<{ ok: boolean; error?: string }>;
  };
  selfmod?: {
    info: () => Promise<import("./self-mod-client").SelfModInfo>;
    list: (rel?: string) => Promise<{ ok: boolean; path?: string; entries?: Array<{ name: string; type: string }>; error?: string }>;
    read: (rel: string) => Promise<{ ok: boolean; path?: string; content?: string; error?: string }>;
    write: (
      rel: string,
      content: string,
      opts?: { note?: string; snapshot?: boolean },
    ) => Promise<{ ok: boolean; path?: string; error?: string }>;
    patch: (
      rel: string,
      find: string,
      replace: string,
      opts?: { replaceAll?: boolean; note?: string },
    ) => Promise<{ ok: boolean; path?: string; error?: string }>;
    snapshot: (note?: string) => Promise<{ ok: boolean; id?: string; fileCount?: number; error?: string }>;
    restore: (id: string) => Promise<{ ok: boolean; error?: string }>;
    journal: (limit?: number) => Promise<{ ok: boolean; entries?: unknown[] }>;
  };
  desktopEntry?: {
    status: () => Promise<{
      ok: boolean;
      menuInstalled?: boolean;
      menuPath?: string;
      autostartInstalled?: boolean;
      autostartPath?: string;
      exec?: string;
    }>;
    install: (opts?: { exec?: string }) => Promise<{ ok: boolean; path?: string; detail?: string; error?: string }>;
    autostart: (enabled: boolean) => Promise<{ ok: boolean; path?: string; detail?: string; error?: string }>;
  };
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
    readOpenClawWorkspace?: (
      path?: string,
    ) => Promise<import("./openclaw-import").OpenClawWorkspaceRaw & {
      ok: boolean;
      error?: string;
      candidates?: string[];
    }>;
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
  const token =
    (typeof process !== "undefined" && process.env?.GROKHUB_HOST_TOKEN) ||
    (typeof window !== "undefined"
      ? (window as unknown as { __GROKHUB_HOST_TOKEN__?: string }).__GROKHUB_HOST_TOKEN__
      : "") ||
    "";
  const headers: Record<string, string> = {
    "content-type": "application/json",
    accept: "application/json",
  };
  if (token) headers["x-grokhub-host-token"] = token;
  const res = await fetch("/api/host", {
    method: "POST",
    headers,
    body: JSON.stringify({ action, ...body, ...(token ? { hostToken: token } : {}) }),
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

export async function hostReadOpenClawWorkspace(path?: string) {
  const e = electronHost();
  if (e?.readOpenClawWorkspace) return e.readOpenClawWorkspace(path);
  return rpc<
    import("./openclaw-import").OpenClawWorkspaceRaw & {
      ok: boolean;
      error?: string;
      candidates?: string[];
    }
  >("readOpenClawWorkspace", { path });
}

export function isDesktopShell(): boolean {
  return typeof window !== "undefined" && Boolean(window.grokhubDesktop);
}
