import type { GrokModeId } from "./types";
import { modelIdForMode, resolveMode } from "./modes";
import { parseRateLimitHeaders } from "./usage";

export const XAI_BASE = "https://api.x.ai/v1";

/** Map GrokHub modes → xAI model IDs */
export function modelForMode(mode: GrokModeId, prompt = ""): string {
  return modelIdForMode(mode, prompt);
}

export function systemPromptForMode(mode: GrokModeId, prompt = ""): string {
  const base = `You are Grok, running inside GrokHub (a desktop agent control plane on the user's Linux machine).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.

You have unsandboxed host access when the desktop gateway is connected.
When you need real filesystem / shell data (Downloads, home, processes, etc.), do NOT invent results.
Put the host command on its OWN line, alone, like:
HOST_CMD: ls -la "$HOME/Downloads"
Never glue HOST_CMD onto a prose sentence. Prefer one simple command (ls, head, cat, find, stat).
The runtime executes it and returns HOST_RESULT — then summarize clearly for the user.
You may use multiple HOST_CMD rounds if needed.

## Connectors
Only use tools listed as LIVE in the connector context below. Do not invent mail, calendar, or Notion results.
For live cloud tools, put a command on its OWN line:
CONNECTOR_CMD: github search_code query:useState repo:facebook/react
CONNECTOR_CMD: github list_issues repo:owner/name
CONNECTOR_CMD: github list_repos
CONNECTOR_CMD: github user
The runtime returns CONNECTOR_RESULT. Summarize for the user; do not invent GitHub data.
Website-linked connectors (Gmail, Notion, etc.) may be marked connected for status only — if tools are not LIVE, say so and suggest the Grok website for those tools.

## Self-modification (optional)
When the user enables self-modification and asks you to change GrokHub itself, you may edit the install tree with:
SELF_MOD: list src/components
SELF_MOD: read src/lib/version.ts
SELF_MOD: write relative/path.ts
<<<CONTENT
// full file body
CONTENT>>>
SELF_MOD: patch relative/path.ts
<<<FIND
exact old text
FIND>>>
<<<REPLACE
new text
REPLACE>>>
SELF_MOD: snapshot note before risky change
Allowed roots: src/, desktop/, scripts/, packaging/, package.json, vite.config.ts, etc. Never touch node_modules, secrets, or user memory.
Always snapshot before multi-file edits. If something breaks, tell the user: Settings → Factory reinstall from GitHub restores stock code (memory can be kept).`;

  const id = resolveMode(mode, prompt);
  switch (id) {
    case "fast":
      return `${base}\nMode: Fast — concise answers, minimal preamble.`;
    case "expert":
      return `${base}\nMode: Expert — reason carefully, surface tradeoffs, cite assumptions.`;
    case "heavy":
      return `${base}\nMode: Heavy (team of experts) — consider multiple angles (ops, research, build, critique), then synthesize a clear recommendation.`;
    case "build":
      return `${base}\nMode: Build — prioritize working code, file paths, and implementable steps. Prefer complete snippets.`;
    default:
      return base;
  }
}

/** Extra system block describing which connectors are actually usable. */
export function connectorContextBlock(
  connectors: Array<{
    id: string;
    name: string;
    status: string;
    tools: string[];
    accountLabel?: string | null;
    liveTools?: boolean;
    source?: string;
  }>,
): string {
  const connected = connectors.filter((c) => c.status === "connected");
  if (!connected.length) {
    return "\n\n## Connector status\nNone connected. User can link Grok website or GitHub token in Settings.";
  }
  const lines = connected.map((c) => {
    const live = c.liveTools || c.id === "github" || c.id === "desktop-host" || c.id === "grok-xai";
    const acct = c.accountLabel ? ` · ${c.accountLabel}` : "";
    const src = c.source ? ` · via ${c.source}` : "";
    return `- ${c.name} (${c.id}): ${live ? "LIVE tools" : "status only (website)"}${acct}${src} · tools: ${c.tools.join(", ")}`;
  });
  return `\n\n## Connector status\n${lines.join("\n")}\nOnly call CONNECTOR_CMD for LIVE tools.`;
}

export type GrokChatMessage = {
  role: "system" | "user" | "assistant";
  content: string;
};

export type GrokChatRequest = {
  /** Console API key (xai-…) */
  apiKey?: string;
  /** OAuth access token from SuperGrok / X Premium device-code login */
  accessToken?: string;
  mode?: GrokModeId;
  messages: GrokChatMessage[];
  model?: string;
  temperature?: number;
  maxTokens?: number;
  signal?: AbortSignal;
  /** OpenClaw workspace / standing agent context */
  workspaceContext?: string;
};

export type GrokChatResult = {
  ok: boolean;
  content?: string;
  model?: string;
  usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
  error?: string;
  status?: number;
  aborted?: boolean;
  rateLimit?: {
    remaining: number | null;
    limit: number | null;
    resetAt: number | null;
  };
};

function resolveBearer(req: GrokChatRequest): { bearer: string; source: "oauth" | "key" | "env" } | null {
  if (req.accessToken?.trim()) {
    return { bearer: req.accessToken.trim(), source: "oauth" };
  }
  if (req.apiKey?.trim()) {
    return { bearer: req.apiKey.trim(), source: "key" };
  }
  const env =
    process.env.XAI_API_KEY?.trim() || process.env.GROK_API_KEY?.trim() || "";
  if (env) return { bearer: env, source: "env" };
  return null;
}

function buildBody(req: GrokChatRequest, stream: boolean) {
  const mode = req.mode ?? "auto";
  const lastUser = [...req.messages].reverse().find((m) => m.role === "user")?.content ?? "";
  const routed = resolveMode(mode, lastUser);
  const model = req.model || modelForMode(mode, lastUser);
  const system =
    systemPromptForMode(mode, lastUser) +
    (req.workspaceContext?.trim()
      ? `\n\n## Imported OpenClaw workspace context\n${req.workspaceContext.trim().slice(0, 24_000)}`
      : "");
  const messages: GrokChatMessage[] = [
    { role: "system", content: system },
    ...req.messages.filter((m) => m.role !== "system"),
  ];
  const temperature =
    req.temperature ??
    (routed === "fast" ? 0.5 : routed === "build" ? 0.4 : routed === "heavy" ? 0.8 : 0.7);
  const max_tokens =
    req.maxTokens ??
    (routed === "heavy" ? 4096 : routed === "build" ? 8192 : routed === "expert" ? 3072 : 2048);
  return {
    model,
    body: {
      model,
      messages,
      temperature,
      max_tokens,
      stream,
    },
    routed,
  };
}

export async function callXaiChat(req: GrokChatRequest): Promise<GrokChatResult> {
  const auth = resolveBearer(req);
  if (!auth) {
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
    };
  }

  const { model, body } = buildBody(req, false);

  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${auth.bearer}`,
      },
      body: JSON.stringify(body),
      signal: req.signal,
    });

    const data = (await res.json().catch(() => ({}))) as {
      error?: { message?: string } | string;
      choices?: Array<{ message?: { content?: string } }>;
      model?: string;
      usage?: GrokChatResult["usage"];
    };
    const rateLimit = parseRateLimitHeaders(res.headers);

    if (!res.ok) {
      if (
        res.status === 404 ||
        (typeof data.error === "object" &&
          /model|not found|invalid/i.test(data.error?.message || ""))
      ) {
        if (model === "grok-4.5" || model === "grok-4-5") {
          return callXaiChat({ ...req, model: "grok-4.3" });
        }
        if (model === "grok-4.3") {
          return callXaiChat({ ...req, model: "grok-4" });
        }
        if (model === "grok-code-fast-1" || /build/i.test(model)) {
          return callXaiChat({ ...req, model: "grok-code-fast-1" });
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChat({ ...req, model: "grok-3-mini-fast" });
        }
      }
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `xAI error ${res.status}`;
      return { ok: false, status: res.status, error: msg, model, rateLimit };
    }

    const content = data.choices?.[0]?.message?.content?.trim();
    if (!content) {
      return { ok: false, status: res.status, error: "Empty response from Grok", model, rateLimit };
    }

    return {
      ok: true,
      content,
      model: data.model || model,
      usage: data.usage,
      status: res.status,
      rateLimit,
    };
  } catch (e) {
    if (req.signal?.aborted || (e instanceof Error && e.name === "AbortError")) {
      return { ok: false, aborted: true, error: "Stopped" };
    }
    return {
      ok: false,
      error: e instanceof Error ? e.message : "Network error calling xAI",
    };
  }
}

export type StreamHandlers = {
  onDelta?: (text: string) => void;
  onStatus?: (status: string) => void;
  signal?: AbortSignal;
};

/** Stream Grok tokens (SSE). Calls onDelta for each piece of content. */
export async function callXaiChatStream(
  req: GrokChatRequest,
  handlers: StreamHandlers = {},
): Promise<GrokChatResult> {
  const auth = resolveBearer(req);
  if (!auth) {
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Use Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
    };
  }

  const signal = handlers.signal || req.signal;
  const { model, body } = buildBody(req, true);
  handlers.onStatus?.("connecting");

  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${auth.bearer}`,
        accept: "text/event-stream",
      },
      body: JSON.stringify(body),
      signal,
    });

    if (!res.ok) {
      // Fall back to non-stream once for model aliases / older accounts
      const errText = await res.text().catch(() => "");
      if (res.status === 404 || /model|not found|invalid/i.test(errText)) {
        if (model === "grok-4.5" || model === "grok-4-5") {
          return callXaiChatStream({ ...req, model: "grok-4.3" }, handlers);
        }
        if (model === "grok-4.3") {
          return callXaiChatStream({ ...req, model: "grok-4" }, handlers);
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChatStream({ ...req, model: "grok-3-mini-fast" }, handlers);
        }
      }
      // Non-stream fallback
      handlers.onStatus?.("fallback");
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    }

    if (!res.body) {
      handlers.onStatus?.("fallback");
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    }

    handlers.onStatus?.("streaming");
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = "";
    let content = "";
    let usedModel = model;
    let streamUsage: GrokChatResult["usage"] | undefined;
    const rateLimit = parseRateLimitHeaders(res.headers);

    while (true) {
      if (signal?.aborted) {
        try {
          await reader.cancel();
        } catch {
          /* ignore */
        }
        return {
          ok: false,
          aborted: true,
          error: "Stopped",
          content,
          model: usedModel,
          usage: streamUsage,
          rateLimit,
        };
      }
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split("\n");
      buffer = lines.pop() || "";
      for (const raw of lines) {
        const line = raw.trim();
        if (!line || line.startsWith(":")) continue;
        if (!line.startsWith("data:")) continue;
        const data = line.slice(5).trim();
        if (data === "[DONE]") continue;
        try {
          const json = JSON.parse(data) as {
            model?: string;
            usage?: GrokChatResult["usage"];
            choices?: Array<{ delta?: { content?: string }; message?: { content?: string } }>;
          };
          if (json.model) usedModel = json.model;
          if (json.usage) streamUsage = json.usage;
          const piece =
            json.choices?.[0]?.delta?.content ||
            json.choices?.[0]?.message?.content ||
            "";
          if (piece) {
            content += piece;
            handlers.onDelta?.(piece);
          }
        } catch {
          /* skip bad chunk */
        }
      }
    }

    if (!content.trim()) {
      return { ok: false, error: "Empty stream from Grok", model: usedModel, rateLimit };
    }
    handlers.onStatus?.("done");
    return { ok: true, content, model: usedModel, usage: streamUsage, rateLimit };
  } catch (e) {
    if (signal?.aborted || (e instanceof Error && e.name === "AbortError")) {
      return { ok: false, aborted: true, error: "Stopped" };
    }
    // Network / stream failure → one non-stream retry
    handlers.onStatus?.("fallback");
    try {
      const full = await callXaiChat({ ...req, model, signal });
      if (full.ok && full.content) handlers.onDelta?.(full.content);
      return full;
    } catch (e2) {
      if (signal?.aborted || (e2 instanceof Error && e2.name === "AbortError")) {
        return { ok: false, aborted: true, error: "Stopped" };
      }
      return {
        ok: false,
        error: e instanceof Error ? e.message : "Network error calling xAI",
      };
    }
  }
}

/** Parse HOST_CMD commands the model emits for desktop execution (own line or inline). */
export function extractHostCommands(text: string): string[] {
  const cmds: string[] = [];
  // Own-line form
  for (const line of text.split("\n")) {
    const m = line.match(/^\s*HOST_CMD:\s*(.+?)\s*$/i);
    if (m?.[1]) cmds.push(m[1].trim());
  }
  // Inline form: "... now. HOST_CMD: ls ..."
  const inline = [...text.matchAll(/(?:^|[\s.])HOST_CMD:\s*(.+?)(?=\n|$)/gi)];
  for (const m of inline) {
    const cmd = (m[1] || "").trim();
    if (cmd && !cmds.includes(cmd)) cmds.push(cmd);
  }
  // Fenced form: ```host\nls\n```
  const fenced = [...text.matchAll(/```(?:host|bash|sh)\s*\n([\s\S]*?)```/gi)];
  for (const m of fenced) {
    for (const line of (m[1] || "").split("\n")) {
      const cmd = line.trim();
      if (cmd && !cmd.startsWith("#") && !cmds.includes(cmd)) cmds.push(cmd);
    }
  }
  return cmds.filter(Boolean);
}

/** Remove HOST_CMD markers from text shown to the user. */
export function stripHostCommands(text: string): string {
  let out = text;
  // Drop own-line HOST_CMD
  out = out
    .split("\n")
    .filter((line) => !/^\s*HOST_CMD:\s*/i.test(line))
    .join("\n");
  // Drop inline HOST_CMD: ... to end of line
  out = out.replace(/\s*HOST_CMD:\s*.+$/gim, "");
  // Drop host fences
  out = out.replace(/```(?:host|bash|sh)\s*\n[\s\S]*?```/gi, "");
  return out.replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * If the user clearly asks about local files/folders and the model forgot HOST_CMD,
 * invent a safe listing command.
 */
export function inferHostCommandsFromUser(prompt: string): string[] {
  const p = prompt.toLowerCase();
  const wantsList =
    /\b(list|show|what('|’)?s|whats|what do i have|contents?|files?|inside|in my)\b/.test(p) ||
    /\b(check|look at|open)\b/.test(p);
  if (!wantsList && !/\b(download|downloads|desktop|documents|home|folder|directory)\b/.test(p)) {
    return [];
  }

  if (/\bdownloads?\b/.test(p)) {
    return [
      'ls -la "${HOME}/Downloads" 2>/dev/null || ls -la ~/Downloads 2>/dev/null || ls -la "$HOME/Descargas" 2>/dev/null || echo "Downloads folder not found"',
    ];
  }
  if (/\bdocuments?\b/.test(p)) {
    return [
      'ls -la "${HOME}/Documents" 2>/dev/null || ls -la ~/Documents 2>/dev/null || echo "Documents folder not found"',
    ];
  }
  if (/\bdesktop\b/.test(p)) {
    return [
      'ls -la "${HOME}/Desktop" 2>/dev/null || ls -la ~/Desktop 2>/dev/null || echo "Desktop folder not found"',
    ];
  }
  if (/\bhome\b/.test(p) && wantsList) {
    return ['ls -la "$HOME" | head -80'];
  }
  return [];
}

export async function probeXaiKey(apiKey: string): Promise<{ ok: boolean; detail: string }> {
  const key = apiKey.trim();
  if (!key) return { ok: false, detail: "API key is empty" };
  try {
    const res = await fetch(`${XAI_BASE}/models`, {
      headers: { authorization: `Bearer ${key}` },
    });
    if (res.ok) return { ok: true, detail: "Connected to xAI · models reachable" };
    const text = await res.text();
    return { ok: false, detail: `xAI ${res.status}: ${text.slice(0, 160)}` };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "probe failed" };
  }
}

export async function probeXaiBearer(bearer: string): Promise<{ ok: boolean; detail: string }> {
  return probeXaiKey(bearer);
}

export type GrokImagineResult = {
  ok: boolean;
  imageDataUrl?: string;
  videoDataUrl?: string;
  model?: string;
  source?: "xai" | "local";
  error?: string;
  mediaKind?: "image" | "video";
};

function sizeForAspect(aspect?: string): string | undefined {
  switch (aspect) {
    case "16:9":
      return "1792x1024";
    case "9:16":
      return "1024x1792";
    case "3:2":
      return "1536x1024";
    case "2:3":
      return "1024x1536";
    case "4:3":
      return "1536x1152";
    case "1:1":
      return "1024x1024";
    default:
      return undefined; // auto
  }
}

function qualityHint(quality?: string, kind?: string): string {
  if (kind === "video") {
    return quality === "quality"
      ? "cinematic motion, high detail, smooth camera, 720p look"
      : "fast motion sketch, simple scene";
  }
  return quality === "quality"
    ? "ultra detailed, sharp focus, professional lighting, high fidelity"
    : "clean composition, efficient render";
}

/** Live Grok / xAI image (and best-effort video) generation. */
export async function callXaiImagine(req: {
  prompt: string;
  accessToken?: string;
  apiKey?: string;
  model?: string;
  aspect?: string;
  quality?: "speed" | "quality";
  mediaKind?: "image" | "video";
  n?: number;
  /** data URL or https URL for reference / img2img style guidance */
  referenceDataUrl?: string;
}): Promise<GrokImagineResult> {
  const auth = resolveBearer({
    accessToken: req.accessToken,
    apiKey: req.apiKey,
    messages: [],
  });
  if (!auth) {
    return {
      ok: false,
      error: "Not connected — Grok OAuth or API key required for live Imagine",
    };
  }
  const prompt = req.prompt.trim();
  if (!prompt) return { ok: false, error: "empty prompt" };

  const mediaKind = req.mediaKind || "image";
  const qHint = qualityHint(req.quality, mediaKind);
  const fullPrompt = `${prompt}\n\n[${qHint}]`.trim();
  const size = sizeForAspect(req.aspect);
  const n = Math.min(4, Math.max(1, req.n || 1));

  if (mediaKind === "video") {
    // Best-effort video endpoints — xAI surface evolves; try known patterns
    const videoModels = [
      req.model,
      "grok-imagine-video",
      "grok-imagine-video-1.5",
      "grok-2-video",
    ].filter(Boolean) as string[];
    const endpoints = [
      `${XAI_BASE}/videos/generations`,
      `${XAI_BASE}/video/generations`,
      `${XAI_BASE}/images/generations`,
    ];
    let lastErr = "video generation unavailable on this API path";
    for (const model of videoModels) {
      for (const url of endpoints) {
        try {
          const body: Record<string, unknown> = {
            model,
            prompt: fullPrompt,
            n: 1,
          };
          if (size) body.size = size;
          if (req.aspect && req.aspect !== "auto") body.aspect_ratio = req.aspect;
          if (req.referenceDataUrl) {
            body.image = req.referenceDataUrl.startsWith("data:")
              ? req.referenceDataUrl
              : req.referenceDataUrl;
            body.image_url = req.referenceDataUrl;
          }
          const res = await fetch(url, {
            method: "POST",
            headers: {
              "content-type": "application/json",
              authorization: `Bearer ${auth.bearer}`,
            },
            body: JSON.stringify(body),
          });
          const data = (await res.json().catch(() => ({}))) as {
            error?: { message?: string } | string;
            data?: Array<{
              b64_json?: string;
              url?: string;
              video_url?: string;
              video?: string;
            }>;
            model?: string;
          };
          if (!res.ok) {
            lastErr =
              typeof data.error === "string"
                ? data.error
                : data.error?.message || `xAI video ${res.status} (${model})`;
            continue;
          }
          const row = data.data?.[0];
          const vid = row?.video_url || row?.video || row?.url || "";
          if (vid) {
            return {
              ok: true,
              videoDataUrl: vid,
              model: data.model || model,
              source: "xai",
              mediaKind: "video",
            };
          }
          // Some responses return image frames only
          const b64 = row?.b64_json || "";
          if (b64) {
            return {
              ok: true,
              imageDataUrl: b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`,
              model: data.model || model,
              source: "xai",
              mediaKind: "image",
              error: "API returned an image frame instead of video for this model",
            };
          }
        } catch (e) {
          lastErr = e instanceof Error ? e.message : "network error";
        }
      }
    }
    return {
      ok: false,
      error: `${lastErr}. Video may require SuperGrok + website Imagine; try Image mode or Grok web.`,
      mediaKind: "video",
    };
  }

  const models = [
    req.model,
    req.quality === "quality" ? "grok-imagine-image" : "grok-2-image",
    "grok-2-image",
    "grok-2-image-1212",
    "grok-imagine-image",
  ].filter(Boolean) as string[];

  let lastErr = "image generation failed";
  for (const model of models) {
    try {
      const body: Record<string, unknown> = {
        model,
        prompt: fullPrompt,
        n,
        response_format: "b64_json",
      };
      if (size) body.size = size;
      if (req.aspect && req.aspect !== "auto") {
        body.aspect_ratio = req.aspect;
      }
      // Reference image for edit / restyle style flows when API accepts it
      if (req.referenceDataUrl) {
        body.image = req.referenceDataUrl;
      }
      const res = await fetch(`${XAI_BASE}/images/generations`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: `Bearer ${auth.bearer}`,
        },
        body: JSON.stringify(body),
      });
      const data = (await res.json().catch(() => ({}))) as {
        error?: { message?: string } | string;
        data?: Array<{ b64_json?: string; b64?: string; url?: string; image?: string }>;
        model?: string;
      };
      if (!res.ok) {
        lastErr =
          typeof data.error === "string"
            ? data.error
            : data.error?.message || `xAI image ${res.status} (${model})`;
        continue;
      }
      const row = data.data?.[0];
      const b64 = row?.b64_json || row?.b64 || row?.image || "";
      if (b64) {
        return {
          ok: true,
          imageDataUrl: b64.startsWith("data:") ? b64 : `data:image/png;base64,${b64}`,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      if (row?.url) {
        return {
          ok: true,
          imageDataUrl: row.url,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      lastErr = "empty image response";
    } catch (e) {
      lastErr = e instanceof Error ? e.message : "network error";
    }
  }
  return { ok: false, error: lastErr, mediaKind: "image" };
}
