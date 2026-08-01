import type { GrokModeId } from "./types";
import type { GrokChatMessage, GrokChatResult } from "./grok";
import type { UpdateResult, UpdateStatus } from "./update";

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
  apiKey?: string;
}): Promise<GrokChatResult> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.chat) {
    return desktop.chat(opts);
  }
  return rpc<GrokChatResult>("/api/grok", "chat", opts as unknown as Record<string, unknown>);
}

export async function grokProbe(apiKey?: string) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.probe) {
    return desktop.probe(apiKey);
  }
  return rpc<{ ok: boolean; detail: string; envConfigured?: boolean }>("/api/grok", "probe", {
    apiKey: apiKey || "",
  });
}

export async function checkUpdate(token?: string) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.checkUpdate) {
    return desktop.checkUpdate({ token });
  }
  return rpc<UpdateStatus>("/api/update", "check", { token: token || "" });
}

export async function applyUpdate(token?: string) {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.applyUpdate) {
    return desktop.applyUpdate({ token });
  }
  return rpc<UpdateResult>("/api/update", "apply", { token: token || "" });
}
