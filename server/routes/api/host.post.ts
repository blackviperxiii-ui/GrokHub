import { defineEventHandler, readBody, createError, getHeader, getRequestURL } from "h3";

function isLoopback(event: Parameters<Parameters<typeof defineEventHandler>[0]>[0]): boolean {
  try {
    const url = getRequestURL(event);
    const host = (url.hostname || "").toLowerCase();
    if (host === "127.0.0.1" || host === "localhost" || host === "::1") return true;
  } catch {
    /* ignore */
  }
  const h = (getHeader(event, "host") || "").toLowerCase();
  return h.startsWith("127.0.0.1") || h.startsWith("localhost") || h.startsWith("[::1]");
}

export default defineEventHandler(async (event) => {
  const body = ((await readBody(event)) || {}) as Record<string, unknown>;
  const action = String(body.action || "");
  if (!isLoopback(event)) {
    throw createError({ statusCode: 403, statusMessage: "Host API is loopback-only" });
  }
  const required = (process.env.GROKHUB_HOST_TOKEN || "").trim();
  if (required) {
    const header = getHeader(event, "x-grokhub-host-token") || "";
    const bodyTok = String(body.hostToken || "");
    if (header !== required && bodyTok !== required) {
      throw createError({ statusCode: 403, statusMessage: "Invalid or missing host token" });
    }
  }
  try {
    const { dispatchHost } = await import("../../../src/lib/host-api-handlers");
    return await dispatchHost(action, body);
  } catch (e) {
    throw createError({
      statusCode: 500,
      statusMessage: e instanceof Error ? e.message : "host api failed",
    });
  }
});
