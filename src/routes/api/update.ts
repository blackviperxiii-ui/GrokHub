import { createFileRoute } from "@tanstack/react-router";

/** Production GitHub update RPC */
export const Route = createFileRoute("/api/update")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        try {
          const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
          const action = String(body.action || "check");
          const { dispatchApi } = await import("@/lib/api-handlers");
          const result = await dispatchApi("update", action, body);
          return Response.json(result);
        } catch (e) {
          const message = e instanceof Error ? e.message : "update api failed";
          return Response.json({ error: message }, { status: 500 });
        }
      },
    },
  },
});
