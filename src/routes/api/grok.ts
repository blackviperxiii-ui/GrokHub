import { createFileRoute } from "@tanstack/react-router";

/** Production Grok / xAI / OAuth RPC */
export const Route = createFileRoute("/api/grok")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        try {
          const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
          const action = String(body.action || "chat");
          const { dispatchApi } = await import("@/lib/api-handlers");
          const result = await dispatchApi("grok", action, body);
          return Response.json(result);
        } catch (e) {
          const message = e instanceof Error ? e.message : "grok api failed";
          return Response.json({ error: message }, { status: 500 });
        }
      },
    },
  },
});
