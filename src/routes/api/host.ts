import { createFileRoute } from "@tanstack/react-router";

/**
 * Production + desktop host bridge (CLI / files / apps).
 * Dev also has Vite middleware for the same path; both call dispatchHost.
 */
export const Route = createFileRoute("/api/host")({
  server: {
    handlers: {
      POST: async ({ request }) => {
        try {
          const body = (await request.json().catch(() => ({}))) as Record<string, unknown>;
          const action = String(body.action || "");
          const { dispatchHost } = await import("@/lib/host-api-handlers");
          const result = await dispatchHost(action, body);
          return Response.json(result);
        } catch (e) {
          const message = e instanceof Error ? e.message : "host api failed";
          return Response.json({ error: message }, { status: 500 });
        }
      },
    },
  },
});
