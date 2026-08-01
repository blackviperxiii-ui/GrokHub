import { defineEventHandler, readBody, createError } from "h3";

export default defineEventHandler(async (event) => {
  const body = ((await readBody(event)) || {}) as Record<string, unknown>;
  const action = String(body.action || "");
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
