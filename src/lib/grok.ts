import type { GrokModeId } from "./types";
import { modelIdForMode, resolveMode } from "./modes";

export const XAI_BASE = "https://api.x.ai/v1";

/** Map GrokHub modes → xAI model IDs */
export function modelForMode(mode: GrokModeId, prompt = ""): string {
  return modelIdForMode(mode, prompt);
}

export function systemPromptForMode(mode: GrokModeId, prompt = ""): string {
  const base = `You are Grok, running inside GrokHub (a desktop agent control plane).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
The user may have unsandboxed host access ($ shell, files, apps) on their Linux desktop.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.`;

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
};

export type GrokChatResult = {
  ok: boolean;
  content?: string;
  model?: string;
  usage?: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number };
  error?: string;
  status?: number;
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

  const mode = req.mode ?? "auto";
  const lastUser = [...req.messages].reverse().find((m) => m.role === "user")?.content ?? "";
  const routed = resolveMode(mode, lastUser);
  const model = req.model || modelForMode(mode, lastUser);
  const system = systemPromptForMode(mode, lastUser);

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

  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${auth.bearer}`,
      },
      body: JSON.stringify({
        model,
        messages,
        temperature,
        max_tokens,
        stream: false,
      }),
    });

    const data = (await res.json().catch(() => ({}))) as {
      error?: { message?: string } | string;
      choices?: Array<{ message?: { content?: string } }>;
      model?: string;
      usage?: GrokChatResult["usage"];
    };

    if (!res.ok) {
      // Retry once with grok-4 if grok-4.3 is unavailable on this account
      if (
        res.status === 404 ||
        (typeof data.error === "object" &&
          /model|not found|invalid/i.test(data.error?.message || ""))
      ) {
        if (model === "grok-4.3") {
          return callXaiChat({ ...req, model: "grok-4" });
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChat({ ...req, model: "grok-3-mini-fast" });
        }
      }
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `xAI error ${res.status}`;
      return { ok: false, status: res.status, error: msg, model };
    }

    const content = data.choices?.[0]?.message?.content?.trim();
    if (!content) {
      return { ok: false, status: res.status, error: "Empty response from Grok", model };
    }

    return {
      ok: true,
      content,
      model: data.model || model,
      usage: data.usage,
      status: res.status,
    };
  } catch (e) {
    return {
      ok: false,
      error: e instanceof Error ? e.message : "Network error calling xAI",
    };
  }
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
  model?: string;
  source?: "xai" | "local";
  error?: string;
};

/** Live Grok / xAI image generation (falls through models if one id is unavailable). */
export async function callXaiImagine(req: {
  prompt: string;
  accessToken?: string;
  apiKey?: string;
  model?: string;
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

  const models = [
    req.model,
    "grok-2-image",
    "grok-2-image-1212",
    "grok-imagine-image",
  ].filter(Boolean) as string[];

  let lastErr = "image generation failed";
  for (const model of models) {
    try {
      const res = await fetch(`${XAI_BASE}/images/generations`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: `Bearer ${auth.bearer}`,
        },
        body: JSON.stringify({
          model,
          prompt,
          n: 1,
          response_format: "b64_json",
        }),
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
        };
      }
      if (row?.url) {
        return { ok: true, imageDataUrl: row.url, model: data.model || model, source: "xai" };
      }
      lastErr = "empty image response";
    } catch (e) {
      lastErr = e instanceof Error ? e.message : "network error";
    }
  }
  return { ok: false, error: lastErr };
}
