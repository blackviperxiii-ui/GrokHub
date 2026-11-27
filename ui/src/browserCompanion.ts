export const BROWSER_COMPANION_OFFERS = ["camera", "location", "talk", "canvas"] as const;
export type BrowserCompanionOffer = (typeof BROWSER_COMPANION_OFFERS)[number];

export interface BrowserCompanionTool {
  name: string;
  description: string;
  inputSchema: Record<string, unknown>;
}

export interface BrowserCompanionCredentials {
  token: string;
  hostName: string;
}

export interface BrowserCompanionRpc {
  jsonrpc?: string;
  id?: string | number | null;
  method: string;
  params?: Record<string, unknown>;
}

export interface BrowserCompanionEnv {
  captureCamera?: (args: Record<string, unknown>) => Promise<string>;
  getLocation?: (args: Record<string, unknown>) => Promise<{
    latitude: number;
    longitude: number;
    accuracy?: number;
  }>;
  speak?: (text: string) => Promise<void>;
  listen?: (args: Record<string, unknown>) => Promise<string>;
  showCanvas?: (panel: Record<string, unknown>) => void;
  clearCanvas?: () => void;
}

const TOOL_CATALOG: Record<BrowserCompanionOffer, BrowserCompanionTool[]> = {
  camera: [
    {
      name: "camera_snap",
      description: "Capture one still image from this browser camera.",
      inputSchema: { type: "object", properties: { facingMode: { type: "string" } } },
    },
  ],
  location: [
    {
      name: "location_get",
      description: "Read this browser's current geolocation coordinates.",
      inputSchema: { type: "object", properties: { highAccuracy: { type: "boolean" } } },
    },
  ],
  talk: [
    {
      name: "talk_say",
      description: "Speak a short message aloud using Web Speech synthesis.",
      inputSchema: {
        type: "object",
        properties: { text: { type: "string" } },
        required: ["text"],
      },
    },
    {
      name: "talk_listen",
      description: "Listen briefly via microphone speech recognition.",
      inputSchema: { type: "object", properties: {} },
    },
  ],
  canvas: [
    {
      name: "canvas_show",
      description: "Display a companion card or HTML snippet in this browser.",
      inputSchema: { type: "object", properties: { title: { type: "string" }, text: { type: "string" } } },
    },
    {
      name: "canvas_clear",
      description: "Clear the active companion canvas panel in this browser.",
      inputSchema: { type: "object", properties: {} },
    },
  ],
};

/** Build the MCP tool descriptors advertised for the user-enabled browser offers. */
export function toolsForBrowserOffers(
  offers: readonly BrowserCompanionOffer[],
): BrowserCompanionTool[] {
  const seen = new Set<BrowserCompanionOffer>();
  const tools: BrowserCompanionTool[] = [];
  for (const offer of BROWSER_COMPANION_OFFERS) {
    if (!offers.includes(offer) || seen.has(offer)) continue;
    seen.add(offer);
    tools.push(...TOOL_CATALOG[offer]);
  }
  return tools;
}

export async function pairBrowserCompanion(opts: {
  code: string;
  id: string;
  name: string;
  baseUrl?: string;
  fetchImpl?: typeof fetch;
}): Promise<BrowserCompanionCredentials> {
  const f = opts.fetchImpl ?? fetch;
  const base = opts.baseUrl ?? "";
  const resp = await f(`${base}/node/v1/pair`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      code: opts.code.trim(),
      id: opts.id.trim(),
      name: opts.name.trim(),
      platform: "browser",
    }),
  });
  const payload = (await resp.json()) as {
    ok?: boolean;
    error?: string;
    token?: string;
    host?: { name?: string };
  };
  if (!resp.ok || !payload.ok || !payload.token) {
    throw new Error(payload.error ?? "Companion pairing failed.");
  }
  return {
    token: payload.token,
    hostName: payload.host?.name ?? "Desktop",
  };
}

export async function announceBrowserCompanion(opts: {
  token: string;
  offers: readonly BrowserCompanionOffer[];
  baseUrl?: string;
  fetchImpl?: typeof fetch;
}): Promise<{ approved: boolean; offers: string[]; allowed: string[] }> {
  const f = opts.fetchImpl ?? fetch;
  const base = opts.baseUrl ?? "";
  const resp = await f(`${base}/node/v1/hello`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${opts.token}`,
    },
    body: JSON.stringify({
      offers: opts.offers,
      tools: toolsForBrowserOffers(opts.offers),
    }),
  });
  const payload = (await resp.json()) as {
    ok?: boolean;
    error?: string;
    approved?: boolean;
    offers?: string[];
    allowed?: string[];
  };
  if (!resp.ok || !payload.ok) {
    throw new Error(payload.error ?? "Failed to update companion offers.");
  }
  return {
    approved: Boolean(payload.approved),
    offers: payload.offers ?? [],
    allowed: payload.allowed ?? [],
  };
}

function defaultEnv(): BrowserCompanionEnv {
  return {
    async getLocation(args) {
      if (typeof navigator === "undefined" || !navigator.geolocation) {
        throw new Error("Geolocation is unavailable in this browser.");
      }
      return new Promise((resolve, reject) => {
        navigator.geolocation.getCurrentPosition(
          (pos) =>
            resolve({
              latitude: pos.coords.latitude,
              longitude: pos.coords.longitude,
              accuracy: pos.coords.accuracy,
            }),
          (err) => reject(new Error(err.message || "Location access denied.")),
          { enableHighAccuracy: Boolean(args.highAccuracy), timeout: 15_000 },
        );
      });
    },
    async speak(text) {
      if (typeof window === "undefined" || !("speechSynthesis" in window)) {
        throw new Error("Speech synthesis is unavailable in this browser.");
      }
      const utter = new SpeechSynthesisUtterance(text);
      window.speechSynthesis.speak(utter);
    },
  };
}

export async function dispatchBrowserCompanionTool(
  name: string,
  args: Record<string, unknown>,
  env: BrowserCompanionEnv = defaultEnv(),
): Promise<{ content: Array<{ type: string; text?: string; data?: string; mimeType?: string }>; isError?: boolean }> {
  try {
    switch (name) {
      case "camera_snap": {
        if (!env.captureCamera) {
          throw new Error("Camera capture is unavailable in this session.");
        }
        const data = await env.captureCamera(args);
        return { content: [{ type: "image", data, mimeType: "image/jpeg" }] };
      }
      case "location_get": {
        if (!env.getLocation) {
          throw new Error("Location capture is unavailable in this session.");
        }
        const coords = await env.getLocation(args);
        return { content: [{ type: "text", text: JSON.stringify(coords) }] };
      }
      case "talk_say": {
        const text = typeof args.text === "string" ? args.text : "";
        if (!env.speak) {
          throw new Error("Speech output is unavailable in this session.");
        }
        await env.speak(text);
        return { content: [{ type: "text", text: "Spoken." }] };
      }
      case "talk_listen": {
        if (!env.listen) {
          throw new Error("Speech input is unavailable in this session.");
        }
        const heard = await env.listen(args);
        return { content: [{ type: "text", text: heard }] };
      }
      case "canvas_show": {
        if (!env.showCanvas) {
          throw new Error("Canvas display is unavailable in this session.");
        }
        env.showCanvas(args);
        return { content: [{ type: "text", text: "Shown." }] };
      }
      case "canvas_clear": {
        if (!env.clearCanvas) {
          throw new Error("Canvas display is unavailable in this session.");
        }
        env.clearCanvas();
        return { content: [{ type: "text", text: "Cleared." }] };
      }
      default:
        return {
          isError: true,
          content: [{ type: "text", text: `Unknown browser companion tool: ${name}` }],
        };
    }
  } catch (err) {
    return {
      isError: true,
      content: [{ type: "text", text: err instanceof Error ? err.message : String(err) }],
    };
  }
}

export class BrowserCompanionSession {
  private readonly baseUrl: string;
  private readonly token: string;
  private readonly fetchImpl: typeof fetch;
  private readonly env: BrowserCompanionEnv;
  private offers: BrowserCompanionOffer[];
  private controller: AbortController | null = null;
  private running = false;

  constructor(opts: {
    token: string;
    offers?: readonly BrowserCompanionOffer[];
    baseUrl?: string;
    fetchImpl?: typeof fetch;
    env?: BrowserCompanionEnv;
  }) {
    this.token = opts.token;
    this.offers = [...(opts.offers ?? [])];
    this.baseUrl = opts.baseUrl ?? "";
    this.fetchImpl = opts.fetchImpl ?? fetch;
    this.env = opts.env ?? defaultEnv();
  }

  async setOffers(offers: readonly BrowserCompanionOffer[]) {
    this.offers = [...offers];
    return announceBrowserCompanion({
      token: this.token,
      offers: this.offers,
      baseUrl: this.baseUrl,
      fetchImpl: this.fetchImpl,
    });
  }

  async pollOnce(): Promise<"idle" | "pending" | "handled"> {
    this.controller = new AbortController();
    const resp = await this.fetchImpl(`${this.baseUrl}/node/v1/next`, {
      method: "GET",
      headers: { Authorization: `Bearer ${this.token}` },
      signal: this.controller.signal,
    });
    const payload = (await resp.json()) as {
      ok?: boolean;
      pending?: boolean;
      rpc?: BrowserCompanionRpc;
      error?: string;
    };
    if (!resp.ok || !payload.ok) {
      throw new Error(payload.error ?? "Companion poll failed.");
    }
    if (payload.pending) return "pending";
    if (!payload.rpc) return "idle";

    const rpc = payload.rpc;
    let reply: Record<string, unknown>;
    if (rpc.method === "tools/list") {
      reply = {
        jsonrpc: "2.0",
        id: rpc.id ?? null,
        result: { tools: toolsForBrowserOffers(this.offers) },
      };
    } else if (rpc.method === "tools/call") {
      const name = String(rpc.params?.name ?? "");
      const args = (rpc.params?.arguments ?? {}) as Record<string, unknown>;
      const result = await dispatchBrowserCompanionTool(name, args, this.env);
      reply = { jsonrpc: "2.0", id: rpc.id ?? null, result };
    } else {
      reply = {
        jsonrpc: "2.0",
        id: rpc.id ?? null,
        error: { code: -32601, message: `Unsupported method: ${rpc.method}` },
      };
    }

    if (rpc.id !== undefined && rpc.id !== null) {
      await this.fetchImpl(`${this.baseUrl}/node/v1/reply`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Authorization: `Bearer ${this.token}`,
        },
        body: JSON.stringify({ rpc: reply }),
      });
    }
    return "handled";
  }

  async runLoop(onError?: (err: unknown) => void): Promise<void> {
    this.running = true;
    while (this.running) {
      try {
        await this.pollOnce();
      } catch (err) {
        if (!this.running) return;
        onError?.(err);
        await new Promise((r) => setTimeout(r, 1_000));
      }
    }
  }

  stop(): void {
    this.running = false;
    this.controller?.abort();
    this.controller = null;
  }
}
