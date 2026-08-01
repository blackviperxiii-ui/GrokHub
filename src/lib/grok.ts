import type { GrokModeId } from "./types";
import { resolveMode } from "./modes";

export const XAI_BASE = "https://api.x.ai/v1";

/** Map GrokHub modes → xAI model IDs */
export function modelForMode(mode: GrokModeId, prompt = ""): string {
  const id = resolveMode(mode, prompt);
  switch (id) {
    case "fast":
      return "grok-4-1-fast-non-reasoning";
    case "expert":
      return "grok-4";
    case "heavy":
      return "grok-4";
    case "build":
      return "grok-code-fast-1";
    case "auto":
    default:
      return "grok-4-1-fast-non-reasoning";
  }
}

export function systemPromptForMode(mode: GrokModeId): string {
  const base = `You are GrokHub, a desktop agent control plane powered by Grok (xAI).
You help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
The user may have unsandboxed host access ($ shell, files, apps) on their Linux desktop.`;

  switch (resolveMode(mode, "")) {
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
  const model = req.model || modelForMode(mode, lastUser);
  const system = systemPromptForMode(mode);

  const messages: GrokChatMessage[] = [
    { role: "system", content: system },
    ...req.messages.filter((m) => m.role !== "system"),
  ];

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
        temperature: req.temperature ?? (mode === "fast" ? 0.5 : 0.7),
        max_tokens: req.maxTokens ?? (mode === "heavy" ? 4096 : mode === "build" ? 8192 : 2048),
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
