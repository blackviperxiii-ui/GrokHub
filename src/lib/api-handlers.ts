/**
 * Unified JSON API handlers for /api/grok and /api/update (Node only).
 */
import {
  callXaiChat,
  callXaiChatStream,
  callXaiImagine,
  probeXaiBearer,
  XAI_BASE,
  type GrokChatMessage,
} from "./grok";
import type { GrokModeId } from "./types";
import { applyUpdate, checkForUpdate } from "./update";
import { parseRateLimitHeaders } from "./usage";
import {
  ensureAccessToken,
  pollXaiDeviceCode,
  startXaiDeviceCode,
  type XaiOAuthTokens,
} from "./xai-oauth";

async function resolveChatAuth(body: Record<string, unknown>) {
  const apiKey = body.apiKey ? String(body.apiKey) : undefined;
  let accessToken = body.accessToken ? String(body.accessToken) : undefined;
  let tokensOut: XaiOAuthTokens | undefined;
  let refreshed = false;

  if (body.tokens && typeof body.tokens === "object") {
    try {
      const ensured = await ensureAccessToken(body.tokens as XaiOAuthTokens);
      accessToken = ensured.accessToken;
      tokensOut = ensured.tokens;
      refreshed = ensured.refreshed;
    } catch (e) {
      if (!accessToken && !(body.tokens as XaiOAuthTokens).accessToken) {
        throw e;
      }
      accessToken = accessToken || (body.tokens as XaiOAuthTokens).accessToken;
    }
  }
  if (!accessToken && body.tokens && typeof body.tokens === "object") {
    const t = body.tokens as XaiOAuthTokens;
    if (t.accessToken) accessToken = t.accessToken;
  }
  return { apiKey, accessToken, tokensOut, refreshed };
}

/** SSE ReadableStream for chat streaming over HTTP */
export function createGrokChatSseStream(body: Record<string, unknown>): ReadableStream<Uint8Array> {
  const encoder = new TextEncoder();
  const messages = (body.messages as GrokChatMessage[]) || [];
  const mode = (body.mode as GrokModeId) || "auto";
  const model = body.model ? String(body.model) : undefined;

  return new ReadableStream({
    async start(controller) {
      const send = (obj: unknown) => {
        controller.enqueue(encoder.encode(`data: ${JSON.stringify(obj)}\n\n`));
      };
      try {
        const auth = await resolveChatAuth(body);
        send({ type: "status", content: "streaming" });
        const result = await callXaiChatStream(
          {
            messages,
            mode,
            model,
            apiKey: auth.apiKey,
            accessToken: auth.accessToken,
            workspaceContext: body.workspaceContext
              ? String(body.workspaceContext)
              : undefined,
          },
          {
            onDelta: (delta) => send({ type: "delta", delta }),
            onStatus: (status) => send({ type: "status", content: status }),
          },
        );
        if (result.aborted) {
          send({ type: "error", error: "Stopped", ok: false });
        } else if (!result.ok) {
          send({ type: "error", error: result.error || "stream failed", ok: false });
        } else {
          send({
            type: "done",
            ok: true,
            content: result.content,
            model: result.model,
            usage: result.usage,
            rateLimit: result.rateLimit,
            tokens: auth.tokensOut,
            refreshed: auth.refreshed,
          });
        }
      } catch (e) {
        send({
          type: "error",
          error: e instanceof Error ? e.message : "stream failed",
          ok: false,
        });
      } finally {
        controller.enqueue(encoder.encode("data: [DONE]\n\n"));
        controller.close();
      }
    },
  });
}

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

    if (action === "websiteUsage") {
      const { fetchGrokWebsiteUsageDirect } = await import("./grok-website-usage");
      return fetchGrokWebsiteUsageDirect({
        ssoCookie: String(body.ssoCookie || ""),
        bearer: String(body.bearer || body.accessToken || "") || null,
      });
    }

    if (action === "usageProbe") {
      const bearer =
        String(body.accessToken || "") ||
        String(body.apiKey || "") ||
        process.env.XAI_API_KEY ||
        process.env.GROK_API_KEY ||
        "";
      if (!bearer) {
        return { ok: false, detail: "not connected" };
      }
      try {
        // Models list is the lightest authenticated call; capture rate-limit headers.
        const res = await fetch(`${XAI_BASE}/models`, {
          headers: { authorization: `Bearer ${bearer}` },
        });
        const rateLimit = parseRateLimitHeaders(res.headers);
        if (!res.ok) {
          return {
            ok: false,
            detail: `xAI ${res.status}`,
            rateLimit,
          };
        }
        return {
          ok: true,
          detail: "usage probe ok",
          rateLimit,
        };
      } catch (e) {
        return {
          ok: false,
          detail: e instanceof Error ? e.message : "probe failed",
        };
      }
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

    if (action === "classifyModels") {
      const models = Array.isArray(body.models)
        ? (body.models as unknown[]).map(String).filter(Boolean)
        : [];
      if (!models.length) {
        return { ok: false, error: "models required" };
      }
      const {
        buildClassifyPrompt,
        parseGrokSlotPlan,
        pickSlotModel,
      } = await import("./models-catalog");
      let accessToken = body.accessToken ? String(body.accessToken) : undefined;
      const apiKey = body.apiKey ? String(body.apiKey) : undefined;
      if (body.tokens && typeof body.tokens === "object") {
        try {
          const ensured = await ensureAccessToken(body.tokens as XaiOAuthTokens);
          accessToken = ensured.accessToken;
        } catch {
          /* keep raw */
        }
      }
      // Use a cheap/fast model for classification when possible
      const classifierModel =
        pickSlotModel("fast", models) || "grok-4-1-fast-non-reasoning";
      const prompt = buildClassifyPrompt(models);
      const result = await callXaiChat({
        messages: [{ role: "user", content: prompt }],
        mode: "fast",
        model: classifierModel,
        apiKey,
        accessToken,
        temperature: 0.1,
        maxTokens: 800,
      });
      if (!result.ok || !result.content) {
        return {
          ok: false,
          error: result.error || "classify failed",
          model: result.model,
        };
      }
      const plan = parseGrokSlotPlan(result.content, models);
      if (!plan) {
        return {
          ok: false,
          error: "Could not parse Grok classification JSON",
          raw: result.content.slice(0, 500),
        };
      }
      return {
        ok: true,
        plan,
        classifierModel: result.model || classifierModel,
      };
    }

    if (action === "imagine") {
      const prompt = String(body.prompt || "");
      let accessToken = body.accessToken ? String(body.accessToken) : undefined;
      const apiKey = body.apiKey ? String(body.apiKey) : undefined;
      if (body.tokens && typeof body.tokens === "object") {
        try {
          const ensured = await ensureAccessToken(body.tokens as XaiOAuthTokens);
          accessToken = ensured.accessToken;
        } catch {
          /* use raw */
        }
      }
      return callXaiImagine({
        prompt,
        accessToken,
        apiKey,
        model: body.model ? String(body.model) : undefined,
        aspect: body.aspect ? String(body.aspect) : undefined,
        quality: body.quality === "quality" || body.quality === "speed" ? body.quality : undefined,
        mediaKind: body.mediaKind === "video" ? "video" : "image",
        n: typeof body.n === "number" ? body.n : undefined,
        referenceDataUrl: body.referenceDataUrl ? String(body.referenceDataUrl) : undefined,
      });
    }

    // chatStream handled as SSE at the route layer via createGrokChatSseStream
    if (action === "chatStream") {
      // Non-SSE callers get a full buffered stream result
      const messages = (body.messages as GrokChatMessage[]) || [];
      const mode = (body.mode as GrokModeId) || "auto";
      const model = body.model ? String(body.model) : undefined;
      const auth = await resolveChatAuth(body);
      let content = "";
      const result = await callXaiChatStream(
        {
          messages,
          mode,
          model,
          apiKey: auth.apiKey,
          accessToken: auth.accessToken,
          workspaceContext: body.workspaceContext
            ? String(body.workspaceContext)
            : undefined,
        },
        {
          onDelta: (d) => {
            content += d;
          },
        },
      );
      return {
        ...result,
        content: result.content || content,
        ...(auth.tokensOut ? { tokens: auth.tokensOut } : {}),
        refreshed: auth.refreshed,
      };
    }

    if (action === "chat") {
      const messages = (body.messages as GrokChatMessage[]) || [];
      const mode = (body.mode as GrokModeId) || "auto";
      const model = body.model ? String(body.model) : undefined;
      try {
        const auth = await resolveChatAuth(body);
        const result = await callXaiChat({
          messages,
          mode,
          model,
          apiKey: auth.apiKey,
          accessToken: auth.accessToken,
          workspaceContext: body.workspaceContext
            ? String(body.workspaceContext)
            : undefined,
        });
        return {
          ...result,
          ...(auth.tokensOut ? { tokens: auth.tokensOut } : {}),
          refreshed: auth.refreshed,
        };
      } catch (e) {
        return {
          ok: false,
          error: e instanceof Error ? e.message : "OAuth refresh failed",
        };
      }
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
        force: Boolean(body.force),
        restart: body.restart !== false,
      });
    }
    throw new Error(`Unknown update action: ${action}`);
  }

  throw new Error(`Unknown route: ${route}`);
}
