import type { GrokModeId } from "./types";
import type { GrokChatMessage, GrokChatResult } from "./grok";
import type { UpdateResult, UpdateStatus } from "./update";
import type { DeviceCodeStart, PollResult, XaiOAuthTokens } from "./xai-oauth";

async function rpc<T>(
  path: "/api/grok" | "/api/update",
  action: string,
  body: Record<string, unknown> = {},
  init?: { signal?: AbortSignal },
): Promise<T> {
  const res = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ action, ...body }),
    signal: init?.signal,
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
  signal?: AbortSignal;
}): Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.chat) {
    return desktop.chat(opts);
  }
  return rpc("/api/grok", "chat", opts as unknown as Record<string, unknown>, {
    signal: opts.signal,
  });
}

export type StreamHandlers = {
  onDelta: (piece: string) => void;
  onStatus?: (status: string) => void;
  signal?: AbortSignal;
};

/**
 * Stream chat tokens. Prefer Electron IPC stream; fall back to SSE /api/grok,
 * then non-stream chat.
 */
export async function grokChatStream(
  opts: {
    messages: GrokChatMessage[];
    mode: GrokModeId;
    model?: string;
    apiKey?: string;
    accessToken?: string;
    tokens?: XaiOAuthTokens | null;
    workspaceContext?: string;
  },
  handlers: StreamHandlers,
): Promise<GrokChatResult & { tokens?: XaiOAuthTokens; refreshed?: boolean }> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.chatStream) {
    return desktop.chatStream(opts, handlers);
  }

  // SSE via production / vite /api/grok
  try {
    const res = await fetch("/api/grok", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        accept: "text/event-stream",
      },
      body: JSON.stringify({ action: "chatStream", ...opts }),
      signal: handlers.signal,
    });

    const ctype = res.headers.get("content-type") || "";
    if (res.ok && (ctype.includes("text/event-stream") || ctype.includes("ndjson"))) {
      handlers.onStatus?.("streaming");
      const reader = res.body?.getReader();
      if (!reader) throw new Error("no stream body");
      const decoder = new TextDecoder();
      let buffer = "";
      let content = "";
      let model: string | undefined;
      let tokens: XaiOAuthTokens | undefined;
      let usage: GrokChatResult["usage"] | undefined;
      let rateLimit: GrokChatResult["rateLimit"] | undefined;
      while (true) {
        if (handlers.signal?.aborted) {
          try {
            await reader.cancel();
          } catch {
            /* ignore */
          }
          return { ok: false, aborted: true, error: "Stopped", content, model, usage, rateLimit };
        }
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const parts = buffer.split("\n");
        buffer = parts.pop() || "";
        for (const raw of parts) {
          const line = raw.trim();
          if (!line || line.startsWith(":")) continue;
          let payload = line;
          if (line.startsWith("data:")) payload = line.slice(5).trim();
          if (payload === "[DONE]") continue;
          try {
            const evt = JSON.parse(payload) as {
              type?: string;
              delta?: string;
              content?: string;
              model?: string;
              error?: string;
              tokens?: XaiOAuthTokens;
              ok?: boolean;
              usage?: GrokChatResult["usage"];
              rateLimit?: GrokChatResult["rateLimit"];
            };
            if (evt.type === "delta" && evt.delta) {
              content += evt.delta;
              handlers.onDelta(evt.delta);
            } else if (evt.type === "status" && evt.content) {
              handlers.onStatus?.(evt.content);
            } else if (evt.type === "done") {
              model = evt.model || model;
              tokens = evt.tokens || tokens;
              usage = evt.usage || usage;
              rateLimit = evt.rateLimit || rateLimit;
              if (evt.content && !content) {
                content = evt.content;
                handlers.onDelta(evt.content);
              }
            } else if (evt.type === "error") {
              return { ok: false, error: evt.error || "stream error", content, model, tokens, usage, rateLimit };
            } else if (evt.delta) {
              content += evt.delta;
              handlers.onDelta(evt.delta);
            }
          } catch {
            /* skip */
          }
        }
      }
      if (!content.trim()) return { ok: false, error: "Empty stream", model, tokens, usage, rateLimit };
      return { ok: true, content, model, tokens, usage, rateLimit };
    }

    // JSON fallback (non-SSE)
    if (res.ok) {
      const data = (await res.json()) as GrokChatResult & { tokens?: XaiOAuthTokens };
      if (data.ok && data.content) handlers.onDelta(data.content);
      return data;
    }
  } catch (e) {
    if (handlers.signal?.aborted || (e instanceof Error && e.name === "AbortError")) {
      return { ok: false, aborted: true, error: "Stopped" };
    }
    // fall through to non-stream
  }

  handlers.onStatus?.("fallback");
  const full = await grokChat({ ...opts, signal: handlers.signal });
  if (full.ok && full.content) handlers.onDelta(full.content);
  return full;
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

export async function grokImagine(opts: {
  prompt: string;
  apiKey?: string;
  accessToken?: string;
  tokens?: XaiOAuthTokens | null;
  aspect?: string;
  quality?: "speed" | "quality";
  mediaKind?: "image" | "video";
  n?: number;
  referenceDataUrl?: string;
  model?: string;
}): Promise<{
  ok: boolean;
  imageDataUrl?: string;
  videoDataUrl?: string;
  model?: string;
  source?: string;
  error?: string;
  mediaKind?: string;
  tokens?: XaiOAuthTokens;
}> {
  const desktop = typeof window !== "undefined" ? window.grokhubDesktop?.grok : undefined;
  if (desktop?.imagine) {
    return desktop.imagine(opts);
  }
  return rpc("/api/grok", "imagine", opts as unknown as Record<string, unknown>);
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
    return desktop.applyUpdate({ token, force, restart: true });
  }
  return rpc<UpdateResult>("/api/update", "apply", {
    token: token || "",
    force,
    restart: false,
  });
}
