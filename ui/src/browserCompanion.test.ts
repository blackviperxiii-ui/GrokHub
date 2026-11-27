import { describe, expect, it, vi } from "vitest";
import {
  BrowserCompanionSession,
  announceBrowserCompanion,
  dispatchBrowserCompanionTool,
  pairBrowserCompanion,
  toolsForBrowserOffers,
} from "./browserCompanion";

describe("browserCompanion", () => {
  it("pairs with platform=browser over same-origin /node/v1/pair", async () => {
    const fetchMock = vi.fn(async () =>
      new Response(
        JSON.stringify({ ok: true, token: "tok-123", host: { name: "Desk" } }),
        { status: 200 },
      ),
    );
    const creds = await pairBrowserCompanion({
      code: "123456",
      id: "iphone-safari",
      name: "iPhone Safari",
      fetchImpl: fetchMock as unknown as typeof fetch,
    });
    expect(creds).toEqual({ token: "tok-123", hostName: "Desk" });
    expect(fetchMock).toHaveBeenCalledWith(
      "/node/v1/pair",
      expect.objectContaining({
        method: "POST",
        body: JSON.stringify({
          code: "123456",
          id: "iphone-safari",
          name: "iPhone Safari",
          platform: "browser",
        }),
      }),
    );
  });

  it("advertises nothing until the user enables specific offers", async () => {
    expect(toolsForBrowserOffers([])).toEqual([]);
    expect(toolsForBrowserOffers(["camera", "location"]).map((t) => t.name)).toEqual([
      "camera_snap",
      "location_get",
    ]);

    const fetchMock = vi.fn(async () =>
      new Response(
        JSON.stringify({
          ok: true,
          approved: false,
          offers: ["camera"],
          allowed: [],
        }),
        { status: 200 },
      ),
    );
    const res = await announceBrowserCompanion({
      token: "tok-123",
      offers: ["camera"],
      fetchImpl: fetchMock as unknown as typeof fetch,
    });
    expect(res.approved).toBe(false);
    expect(res.offers).toEqual(["camera"]);
  });

  it("dispatches browser capabilities and handles denied APIs gracefully", async () => {
    const shown: Record<string, unknown>[] = [];
    const env = {
      captureCamera: async () => "base64jpeg",
      getLocation: async () => ({ latitude: 30.26, longitude: -97.74, accuracy: 8 }),
      speak: async () => {},
      showCanvas: (panel: Record<string, unknown>) => {
        shown.push(panel);
      },
      clearCanvas: () => {
        shown.length = 0;
      },
    };
    const snap = await dispatchBrowserCompanionTool("camera_snap", {}, env);
    expect(snap.isError).toBeUndefined();
    expect(snap.content[0]?.data).toBe("base64jpeg");

    const loc = await dispatchBrowserCompanionTool("location_get", {}, env);
    expect(loc.content[0]?.text).toContain("30.26");

    await dispatchBrowserCompanionTool("canvas_show", { title: "Map" }, env);
    expect(shown).toHaveLength(1);
    await dispatchBrowserCompanionTool("canvas_clear", {}, env);
    expect(shown).toHaveLength(0);

    const denied = await dispatchBrowserCompanionTool("location_get", {}, {
      getLocation: async () => {
        throw new Error("Permission denied");
      },
    });
    expect(denied.isError).toBe(true);
    expect(denied.content[0]?.text).toBe("Permission denied");
  });

  it("answers relayed tools/list and tools/call requests via /next and /reply", async () => {
    const replies: unknown[] = [];
    const fetchMock = vi.fn(async (url: string, init?: RequestInit) => {
      if (url.endsWith("/next")) {
        return new Response(
          JSON.stringify({
            ok: true,
            rpc: {
              jsonrpc: "2.0",
              id: "r1",
              method: "tools/call",
              params: { name: "location_get", arguments: {} },
            },
          }),
          { status: 200 },
        );
      }
      replies.push(JSON.parse(String(init?.body ?? "{}")));
      return new Response(JSON.stringify({ ok: true, taken: true }), { status: 200 });
    });

    const session = new BrowserCompanionSession({
      token: "tok-123",
      offers: ["location"],
      fetchImpl: fetchMock as unknown as typeof fetch,
      env: {
        getLocation: async () => ({ latitude: 12.34, longitude: 56.78 }),
      },
    });
    const outcome = await session.pollOnce();
    expect(outcome).toBe("handled");
    expect(replies).toEqual([
      {
        rpc: {
          jsonrpc: "2.0",
          id: "r1",
          result: {
            content: [
              { type: "text", text: JSON.stringify({ latitude: 12.34, longitude: 56.78 }) },
            ],
          },
        },
      },
    ]);
    session.stop();
  });
});
