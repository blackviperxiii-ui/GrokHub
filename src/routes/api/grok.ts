import { createFileRoute } from "@tanstack/react-router";

/** Production Grok / xAI / OAuth RPC (+ SSE chat stream) */
export const Route = createFileRoute("/api/grok")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        try {
          const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
          const action = String(body.action || "chat");

          if (action === "chatStream") {
            const { createGrokChatSseStream } = await import("@/lib/api-handlers");
            const stream = createGrokChatSseStream(body);
            return new Response(stream, {
              headers: {
                "content-type": "text/event-stream; charset=utf-8",
                "cache-control": "no-cache, no-transform",
                connection: "keep-alive",
              },
            });
          }

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
