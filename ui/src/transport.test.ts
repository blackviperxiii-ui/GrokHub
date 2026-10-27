import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  call,
  CODE_REFUSED,
  NOT_GROKHUB,
  NOT_PAIRED,
  onPairChange,
  pair,
  paired,
  subscribe,
  unpair,
  UNREACHABLE,
} from "./transport";

const BASE = "http://192.168.1.20:4620";
const CODE = "pair-abcdefghijkl";

type Route = (init: RequestInit | undefined) => Response | Promise<Response>;

/** A desktop that serves the remote API; `routes` maps a path to its reply. */
function desktop(routes: Record<string, Route>) {
  const seen: { url: string; init: RequestInit | undefined }[] = [];
  let push: (text: string) => void = () => {};
  const stream = new ReadableStream<Uint8Array>({
    start(controller) {
      push = (text) => controller.enqueue(new TextEncoder().encode(text));
    },
  });
  const fetch = vi.fn(async (url: string, init?: RequestInit) => {
    seen.push({ url, init });
    const path = url.slice(BASE.length);
    if (path === "/remote/v1/hello") return Response.json({ kind: "grokhub-remote", version: 1 });
    if (path === "/remote/v1/events") {
      const auth = new Headers(init?.headers).get("Authorization");
      if (auth !== `Bearer ${CODE}`) return new Response("", { status: 401 });
      return new Response(stream, { headers: { "Content-Type": "text/event-stream" } });
    }
    const route = routes[path];
    return route ? route(init) : new Response("", { status: 404 });
  });
  vi.stubGlobal("fetch", fetch);
  return { seen, push: (text: string) => push(text) };
}

describe("transport (web app)", () => {
  beforeEach(() => {
    unpair();
    localStorage.clear();
    sessionStorage.clear();
  });
  afterEach(() => {
    unpair();
    vi.unstubAllGlobals();
  });

  it("refuses a command before pairing", async () => {
    await expect(call("get_settings")).rejects.toBe(NOT_PAIRED);
  });

  it("pairs, sends commands with the code and returns their value", async () => {
    const box = desktop({
      "/remote/v1/call/send_chat": () => Response.json({ ok: true, value: "end_turn" }),
    });
    await pair(` ${CODE} `, `${BASE}/`);
    expect(paired()).toBe(true);

    await expect(call("send_chat", { conversationId: "c1", text: "hi" })).resolves.toBe(
      "end_turn",
    );
    const sent = box.seen.find((s) => s.url.endsWith("/call/send_chat"));
    expect(sent?.init?.method).toBe("POST");
    expect(new Headers(sent?.init?.headers).get("Authorization")).toBe(`Bearer ${CODE}`);
    expect(sent?.init?.body).toBe('{"conversationId":"c1","text":"hi"}');
    expect(sent?.init?.credentials).toBe("omit");
  });

  it("keeps the code out of every browser store", async () => {
    desktop({});
    await pair(CODE, BASE);
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(document.cookie).toBe("");
  });

  it("rejects with the desktop's own message, like invoke", async () => {
    desktop({
      "/remote/v1/call/save_settings": () =>
        Response.json({ ok: false, error: "The web app can't change settings." }),
    });
    await pair(CODE, BASE);
    await expect(call("save_settings")).rejects.toBe("The web app can't change settings.");
  });

  it("drops the pairing when the desktop refuses the code", async () => {
    desktop({ "/remote/v1/call/get_settings": () => new Response("", { status: 401 }) });
    await pair(CODE, BASE);
    const changes: boolean[] = [];
    const stop = onPairChange((now) => changes.push(now));
    await expect(call("get_settings")).rejects.toBe(CODE_REFUSED);
    expect(paired()).toBe(false);
    expect(changes).toEqual([false]);
    stop();
  });

  it("says when the code is wrong, the address isn't Loadout, or it can't be reached", async () => {
    desktop({});
    await expect(pair("wrong-code", BASE)).rejects.toThrow(CODE_REFUSED);
    expect(paired()).toBe(false);

    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ kind: "something-else" })));
    await expect(pair(CODE, BASE)).rejects.toThrow(NOT_GROKHUB);

    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );
    await expect(pair(CODE, BASE)).rejects.toThrow(UNREACHABLE);
    await expect(pair("  ", BASE)).rejects.toThrow("Enter the pairing code Loadout shows.");
  });

  it("hands each event on the stream to its listeners", async () => {
    const box = desktop({});
    await pair(CODE, BASE);
    const got: unknown[] = [];
    const stop = await subscribe("chat://event", (payload) => got.push(payload));
    const other: unknown[] = [];
    await subscribe("other://event", (payload) => other.push(payload));

    const envelope = { conversationId: "c1", event: { kind: "text", text: "Hel" } };
    box.push(": ping\n\n");
    // One frame split across two chunks, with CRLF line ends.
    box.push(`data: ${JSON.stringify({ event: "chat://event", payload: envelope }).slice(0, 20)}`);
    box.push(`${JSON.stringify({ event: "chat://event", payload: envelope }).slice(20)}\r\n\r\n`);
    await vi.waitFor(() => expect(got).toEqual([envelope]));
    expect(other).toEqual([]);

    stop();
    box.push(`data: ${JSON.stringify({ event: "chat://event", payload: envelope })}\n\n`);
    await new Promise((done) => setTimeout(done, 20));
    expect(got).toHaveLength(1);
  });
});
