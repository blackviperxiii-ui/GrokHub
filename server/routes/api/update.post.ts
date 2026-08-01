import { defineEventHandler, readBody, createError } from "h3";

export default defineEventHandler(async (event) => {
  const body = ((await readBody(event)) || {}) as Record<string, unknown>;
  const action = String(body.action || "check");
  try {
    const { dispatchApi } = await import("../../../src/lib/api-handlers");
    return await dispatchApi("update", action, body);
  } catch (e) {
    throw createError({
      statusCode: 500,
      statusMessage: e instanceof Error ? e.message : "update api failed",
    });
  }
});
