/**
 * Unified JSON API handlers for /api/grok and /api/update (Node only).
 */
import { callXaiChat, probeXaiKey, XAI_BASE, type GrokChatMessage } from "./grok";
import type { GrokModeId } from "./types";
import { applyUpdate, checkForUpdate } from "./update";

export async function dispatchApi(
  route: "grok" | "update",
  action: string,
  body: Record<string, unknown>,
): Promise<unknown> {
  if (route === "grok") {
    if (action === "probe" || action === "status") {
      const apiKey = String(body.apiKey || "");
      if (!apiKey && !process.env.XAI_API_KEY && !process.env.GROK_API_KEY) {
        return {
          ok: false,
          detail: "No API key configured",
          envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
        };
      }
      const key = apiKey || process.env.XAI_API_KEY || process.env.GROK_API_KEY || "";
      const result = await probeXaiKey(key);
      return {
        ...result,
        envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
      };
    }
    if (action === "models") {
      const apiKey =
        String(body.apiKey || "") ||
        process.env.XAI_API_KEY ||
        process.env.GROK_API_KEY ||
        "";
      if (!apiKey) return { models: [] };
      try {
        const res = await fetch(`${XAI_BASE}/models`, {
          headers: { authorization: `Bearer ${apiKey}` },
        });
        if (!res.ok) return { models: [] };
        const data = (await res.json()) as { data?: Array<{ id?: string }> };
        const models = (data.data || []).map((m) => m.id || "").filter(Boolean);
        return { models };
      } catch {
        return { models: [] };
      }
    }
    if (action === "chat") {
      const messages = (body.messages as GrokChatMessage[]) || [];
      const mode = (body.mode as GrokModeId) || "auto";
      const apiKey = body.apiKey ? String(body.apiKey) : undefined;
      return callXaiChat({ messages, mode, apiKey });
    }
    throw new Error(`Unknown grok action: ${action}`);
  }

  if (route === "update") {
    if (action === "check") {
      return checkForUpdate({
        repo: body.repo ? String(body.repo) : undefined,
        branch: body.branch ? String(body.branch) : undefined,
        token: body.token ? String(body.token) : undefined,
      });
    }
    if (action === "apply") {
      return applyUpdate({
        repo: body.repo ? String(body.repo) : undefined,
        branch: body.branch ? String(body.branch) : undefined,
        token: body.token ? String(body.token) : undefined,
      });
    }
    throw new Error(`Unknown update action: ${action}`);
  }

  throw new Error(`Unknown route: ${route}`);
}
