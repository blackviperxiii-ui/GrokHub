/**
 * Unified JSON API handlers for /api/grok and /api/update (Node only).
 */
import { callXaiChat, probeXaiBearer, XAI_BASE, type GrokChatMessage } from "./grok";
import type { GrokModeId } from "./types";
import { applyUpdate, checkForUpdate } from "./update";
import {
  ensureAccessToken,
  pollXaiDeviceCode,
  startXaiDeviceCode,
  type XaiOAuthTokens,
} from "./xai-oauth";

export async function dispatchApi(
  route: "grok" | "update",
  action: string,
  body: Record<string, unknown>,
): Promise<unknown> {
  if (route === "grok") {
    if (action === "oauthStart") {
      const start = await startXaiDeviceCode();
      return { ok: true, ...start };
    }

    if (action === "oauthPoll") {
      const deviceCode = String(body.deviceCode || "");
      if (!deviceCode) throw new Error("deviceCode required");
      return pollXaiDeviceCode(deviceCode);
    }

    if (action === "oauthEnsure") {
      const tokens = body.tokens as XaiOAuthTokens | undefined;
      if (!tokens?.accessToken) throw new Error("tokens required");
      const ensured = await ensureAccessToken(tokens);
      // Verify still works
      const probe = await probeXaiBearer(ensured.accessToken);
      return {
        ok: probe.ok,
        detail: probe.detail,
        refreshed: ensured.refreshed,
        tokens: ensured.tokens,
      };
    }

    if (action === "probe" || action === "status") {
      const accessToken = String(body.accessToken || "");
      const apiKey = String(body.apiKey || "");
      const bearer =
        accessToken ||
        apiKey ||
        process.env.XAI_API_KEY ||
        process.env.GROK_API_KEY ||
        "";
      if (!bearer) {
        return {
          ok: false,
          detail: "No Grok OAuth session or API key",
          envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
        };
      }
      const result = await probeXaiBearer(bearer);
      return {
        ...result,
        envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
        authMode: accessToken ? "oauth" : apiKey ? "apiKey" : "env",
      };
    }

    if (action === "models") {
      const bearer =
        String(body.accessToken || "") ||
        String(body.apiKey || "") ||
        process.env.XAI_API_KEY ||
        process.env.GROK_API_KEY ||
        "";
      if (!bearer) return { models: [] };
      try {
        const res = await fetch(`${XAI_BASE}/models`, {
          headers: { authorization: `Bearer ${bearer}` },
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
      let accessToken = body.accessToken ? String(body.accessToken) : undefined;
      // Auto-refresh if client sent full tokens blob
      if (body.tokens && typeof body.tokens === "object") {
        try {
          const ensured = await ensureAccessToken(body.tokens as XaiOAuthTokens);
          accessToken = ensured.accessToken;
          const result = await callXaiChat({ messages, mode, apiKey, accessToken });
          return { ...result, tokens: ensured.tokens, refreshed: ensured.refreshed };
        } catch (e) {
          return {
            ok: false,
            error: e instanceof Error ? e.message : "OAuth refresh failed",
          };
        }
      }
      return callXaiChat({ messages, mode, apiKey, accessToken });
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
        force: body.force === true || body.force === "1" || body.force === 1,
      });
    }
    throw new Error(`Unknown update action: ${action}`);
  }

  throw new Error(`Unknown route: ${route}`);
}
