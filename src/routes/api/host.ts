import { createFileRoute } from "@tanstack/react-router";

function clientIp(request: Request): string {
  // Prefer proxy headers only when clearly loopback-ish; default to remote
  return (
    request.headers.get("x-forwarded-for")?.split(",")[0]?.trim() ||
    request.headers.get("x-real-ip") ||
    ""
  );
}

function isLocalRequest(request: Request): boolean {
  try {
    const url = new URL(request.url);
    const host = (url.hostname || "").toLowerCase();
    if (host === "127.0.0.1" || host === "localhost" || host === "::1") return true;
  } catch {
    /* ignore */
  }
  const ip = clientIp(request);
  if (ip === "127.0.0.1" || ip === "::1" || ip === "localhost") return true;
  // Same-origin browser calls from the app UI are allowed when Host is loopback
  const h = (request.headers.get("host") || "").toLowerCase();
  if (h.startsWith("127.0.0.1") || h.startsWith("localhost") || h.startsWith("[::1]")) {
    return true;
  }
  return false;
}

function authorizeHost(request: Request, body: Record<string, unknown>): string | null {
  if (!isLocalRequest(request)) {
    return "Host API is loopback-only";
  }
  const required = (process.env.GROKHUB_HOST_TOKEN || "").trim();
  if (!required) return null; // token optional unless set
  const header = request.headers.get("x-grokhub-host-token") || "";
  const bodyTok = String(body.hostToken || "");
  if (header === required || bodyTok === required) return null;
  return "Invalid or missing host token";
}

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
          const denied = authorizeHost(request, body);
          if (denied) {
            return Response.json({ error: denied }, { status: 403 });
          }
          // Block high-risk actions without token when not Electron (defense in depth)
          const action = String(body.action || "");
          const required = (process.env.GROKHUB_HOST_TOKEN || "").trim();
          if (
            required &&
            ["exec", "writeFile", "openApp"].includes(action) &&
            !(
              request.headers.get("x-grokhub-host-token") === required ||
              body.hostToken === required
            )
          ) {
            return Response.json({ error: "Host token required for this action" }, { status: 403 });
          }
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
