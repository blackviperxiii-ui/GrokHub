import type { GrokModeId } from "./types";
import type { GrokChatMessage, GrokChatResult } from "./grok";
import type { UpdateResult, UpdateStatus } from "./update";
import type { DeviceCodeStart, PollResult, XaiOAuthTokens } from "./xai-oauth";

async function rpc<T>(
  path: "/api/grok" | "/api/update",
  action: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ action, ...body }),
  });
  const data = (await res.json().catch(() => ({}))) as T & { error?: string };
  if (!res.ok) {
    throw new Error((data as { error?: string }).error || `HTTP ${res.status}`);
  }
  return data;
}

export async function grokChat(opts: {
  messages: GrokChatMessage[];
  mode: GrokModeId;
  model?: string;
  apiKey?: string;
  accessToken?: string;
  tokens?: XaiOAuthTokens | null;
}): Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.chat) {
    return desktop.chat(opts);
  }
  return rpc("/api/grok", "chat", opts as unknown as Record<string, unknown>);
}

export async function grokProbe(opts?: {
  apiKey?: string;
  accessToken?: string;
}) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.probe) {
    return desktop.probe(opts?.apiKey, opts?.accessToken);
  }
  return rpc<{
    ok: boolean;
    detail: string;
    envConfigured?: boolean;
    authMode?: string;
  }>("/api/grok", "probe", {
    apiKey: opts?.apiKey || "",
    accessToken: opts?.accessToken || "",
  });
}

export async function oauthStart(): Promise<DeviceCodeStart & { ok: boolean }> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.oauthStart) return desktop.oauthStart();
  return rpc("/api/grok", "oauthStart", {});
}

export async function oauthPoll(deviceCode: string): Promise<PollResult> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.oauthPoll) return desktop.oauthPoll(deviceCode);
  return rpc("/api/grok", "oauthPoll", { deviceCode });
}

export async function oauthEnsure(tokens: XaiOAuthTokens) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.oauthEnsure) return desktop.oauthEnsure(tokens);
  return rpc<{
    ok: boolean;
    detail: string;
    refreshed: boolean;
    tokens: XaiOAuthTokens;
  }>("/api/grok", "oauthEnsure", { tokens });
}

export async function checkUpdate(token?: string) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.checkUpdate) {
    return desktop.checkUpdate({ token });
  }
  return rpc<UpdateStatus>("/api/update", "check", { token: token || "" });
}

export async function applyUpdate(token?: string, force = true) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.applyUpdate) {
    // Desktop: always restart after a successful install
    return desktop.applyUpdate({ token, force, restart: true });
  }
  // Browser preview: no process restart
  return rpc<UpdateResult>("/api/update", "apply", {
    token: token || "",
    force,
    restart: false,
  });
}
