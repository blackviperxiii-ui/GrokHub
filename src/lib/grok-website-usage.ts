/**
 * Grok website subscription usage (Settings → Usage).
 *
 * Source: grok.com gRPC-Web
 *   POST /grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig
 *
 * Auth is the consumer SSO cookie from grok.com (same session as the website),
 * not the xAI API management key. Device-code OAuth alone is not enough.
 */

export type GrokProductId =
  | "chat"
  | "build"
  | "app_builder"
  | "imagine"
  | "voice"
  | "plugins"
  | "api"
  | "other";

export type GrokProductUsage = {
  product: GrokProductId;
  label: string;
  usagePercent: number;
};

export type GrokWebsiteUsage = {
  ok: boolean;
  error?: string;
  /** e.g. SuperGrok Heavy */
  planLabel: string;
  planId: "free" | "super" | "pro" | "plus" | "heavy" | "lite";
  /** 0–100+ overall weekly pool used */
  creditUsagePercent: number;
  periodType: "weekly" | "monthly" | "unknown";
  periodStart: number | null;
  periodEnd: number | null;
  productUsage: GrokProductUsage[];
  /** Extra usage credits balance in USD cents */
  prepaidBalanceCents: number;
  onDemandCapCents: number;
  onDemandUsedCents: number;
  raw?: unknown;
};

const PRODUCT_LABELS: Record<number, { id: GrokProductId; label: string }> = {
  0: { id: "other", label: "Other" },
  1: { id: "api", label: "API" },
  2: { id: "build", label: "Grok Build" },
  3: { id: "plugins", label: "Plugins" },
  4: { id: "chat", label: "Chat" },
  5: { id: "imagine", label: "Imagine" },
  6: { id: "voice", label: "Voice" },
  7: { id: "app_builder", label: "App Builder" },
};

const GROK_CREDITS_URL =
  "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const SUBSCRIPTIONS_URL = "https://grok.com/rest/subscriptions";

function grpcWebFrame(payload: Uint8Array): Uint8Array {
  const out = new Uint8Array(5 + payload.length);
  out[0] = 0; // no compression
  const len = payload.length;
  out[1] = (len >>> 24) & 0xff;
  out[2] = (len >>> 16) & 0xff;
  out[3] = (len >>> 8) & 0xff;
  out[4] = len & 0xff;
  out.set(payload, 5);
  return out;
}

/** GetGrokCreditsConfigRequest { exclude_legacy_monthly_usage = true } */
function encodeCreditsRequest(): Uint8Array {
  // field 1, varint, true
  return new Uint8Array([0x08, 0x01]);
}

function readVarint(buf: Uint8Array, offset: number): { value: number; next: number } {
  let result = 0;
  let shift = 0;
  let pos = offset;
  while (pos < buf.length) {
    const b = buf[pos++]!;
    result |= (b & 0x7f) << shift;
    if ((b & 0x80) === 0) break;
    shift += 7;
    if (shift > 35) break;
  }
  return { value: result >>> 0, next: pos };
}

function readVarintBig(buf: Uint8Array, offset: number): { value: bigint; next: number } {
  let result = 0n;
  let shift = 0n;
  let pos = offset;
  while (pos < buf.length) {
    const b = BigInt(buf[pos++]!);
    result |= (b & 0x7fn) << shift;
    if ((b & 0x80n) === 0n) break;
    shift += 7n;
  }
  return { value: result, next: pos };
}

/** Minimal protobuf wire decoder → nested plain objects keyed by field number. */
function decodeFields(buf: Uint8Array): Record<number, unknown[]> {
  const out: Record<number, unknown[]> = {};
  let i = 0;
  while (i < buf.length) {
    const tag = readVarint(buf, i);
    i = tag.next;
    const field = tag.value >>> 3;
    const wire = tag.value & 7;
    if (field === 0) break;
    if (wire === 0) {
      const v = readVarintBig(buf, i);
      i = v.next;
      (out[field] ||= []).push(v.value);
    } else if (wire === 1) {
      // 64-bit
      if (i + 8 > buf.length) break;
      const slice = buf.slice(i, i + 8);
      i += 8;
      (out[field] ||= []).push(slice);
    } else if (wire === 2) {
      const len = readVarint(buf, i);
      i = len.next;
      const slice = buf.slice(i, i + len.value);
      i += len.value;
      (out[field] ||= []).push(slice);
    } else if (wire === 5) {
      if (i + 4 > buf.length) break;
      const slice = buf.slice(i, i + 4);
      i += 4;
      (out[field] ||= []).push(slice);
    } else {
      break;
    }
  }
  return out;
}

function asBytes(v: unknown): Uint8Array | null {
  return v instanceof Uint8Array ? v : null;
}

function asBig(v: unknown): bigint | null {
  return typeof v === "bigint" ? v : null;
}

function decodeDouble(bytes: Uint8Array): number {
  if (bytes.length < 8) return 0;
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  return view.getFloat64(0, true);
}

function decodeTimestamp(bytes: Uint8Array): number | null {
  const f = decodeFields(bytes);
  const sec = asBig(f[1]?.[0]);
  const nanos = asBig(f[2]?.[0]);
  if (sec == null) return null;
  const ms = Number(sec) * 1000 + (nanos != null ? Number(nanos) / 1e6 : 0);
  return Number.isFinite(ms) ? ms : null;
}

function decodeCent(bytes: Uint8Array): number {
  const f = decodeFields(bytes);
  const val = asBig(f[1]?.[0]);
  return val != null ? Number(val) : 0;
}

function parseGrpcWebResponse(buf: Uint8Array): {
  status: number;
  message: string;
  messages: Uint8Array[];
} {
  let i = 0;
  const messages: Uint8Array[] = [];
  let status = 0;
  let message = "";
  while (i + 5 <= buf.length) {
    const flag = buf[i]!;
    const len =
      ((buf[i + 1]! << 24) | (buf[i + 2]! << 16) | (buf[i + 3]! << 8) | buf[i + 4]!) >>> 0;
    i += 5;
    if (i + len > buf.length) break;
    const chunk = buf.slice(i, i + len);
    i += len;
    if (flag === 0) {
      messages.push(chunk);
    } else if (flag === 0x80 || flag === 128) {
      // trailers (text)
      const text = new TextDecoder().decode(chunk);
      const statusMatch = /grpc-status:\s*(\d+)/i.exec(text);
      const msgMatch = /grpc-message:\s*([^\r\n]+)/i.exec(text);
      if (statusMatch) status = Number(statusMatch[1]);
      if (msgMatch) message = decodeURIComponent(msgMatch[1]!.replace(/\+/g, " "));
    }
  }
  // Some servers only send grpc-status header (handled by caller)
  return { status, message, messages };
}

function parseCreditsConfig(msg: Uint8Array): Omit<GrokWebsiteUsage, "ok" | "planLabel" | "planId"> {
  // GetGrokCreditsConfigResponse { config = 1 }
  const root = decodeFields(msg);
  const configBytes = asBytes(root[1]?.[0]);
  if (!configBytes) {
    return {
      creditUsagePercent: 0,
      periodType: "unknown",
      periodStart: null,
      periodEnd: null,
      productUsage: [],
      prepaidBalanceCents: 0,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
    };
  }
  const c = decodeFields(configBytes);
  // current_period = 1
  let periodType: GrokWebsiteUsage["periodType"] = "unknown";
  let periodStart: number | null = null;
  let periodEnd: number | null = null;
  const periodBytes = asBytes(c[1]?.[0]);
  if (periodBytes) {
    const p = decodeFields(periodBytes);
    const t = asBig(p[1]?.[0]);
    if (t === 2n) periodType = "weekly";
    else if (t === 1n) periodType = "monthly";
    const s = asBytes(p[2]?.[0]);
    const e = asBytes(p[3]?.[0]);
    if (s) periodStart = decodeTimestamp(s);
    if (e) periodEnd = decodeTimestamp(e);
  }
  // credit_usage_percent = 2 (double, fixed64)
  let creditUsagePercent = 0;
  const pctBytes = asBytes(c[2]?.[0]);
  if (pctBytes && pctBytes.length === 8) {
    creditUsagePercent = decodeDouble(pctBytes);
  } else {
    // sometimes encoded as float field 5
    const f5 = asBytes(c[2]?.[0]);
    if (f5 && f5.length === 4) {
      const view = new DataView(f5.buffer, f5.byteOffset, 4);
      creditUsagePercent = view.getFloat32(0, true);
    }
  }
  const onDemandCapCents = asBytes(c[3]?.[0]) ? decodeCent(asBytes(c[3]![0]!)!) : 0;
  const onDemandUsedCents = asBytes(c[4]?.[0]) ? decodeCent(asBytes(c[4]![0]!)!) : 0;
  const productUsage: GrokProductUsage[] = [];
  for (const raw of c[5] || []) {
    const b = asBytes(raw);
    if (!b) continue;
    const pu = decodeFields(b);
    const prodNum = Number(asBig(pu[1]?.[0]) ?? 0);
    const meta = PRODUCT_LABELS[prodNum] || PRODUCT_LABELS[0]!;
    let usagePercent = 0;
    const up = asBytes(pu[2]?.[0]);
    if (up && up.length === 8) usagePercent = decodeDouble(up);
    else if (up && up.length === 4) {
      const view = new DataView(up.buffer, up.byteOffset, 4);
      usagePercent = view.getFloat32(0, true);
    }
    if (usagePercent > 0) {
      productUsage.push({
        product: meta.id,
        label: meta.label,
        usagePercent,
      });
    }
  }
  productUsage.sort((a, b) => b.usagePercent - a.usagePercent);
  const prepaidBalanceCents = asBytes(c[8]?.[0]) ? decodeCent(asBytes(c[8]![0]!)!) : 0;
  // billing_period_* may override
  const bStart = asBytes(c[10]?.[0]);
  const bEnd = asBytes(c[11]?.[0]);
  if (bStart) periodStart = decodeTimestamp(bStart) ?? periodStart;
  if (bEnd) periodEnd = decodeTimestamp(bEnd) ?? periodEnd;

  return {
    creditUsagePercent: Math.max(0, creditUsagePercent),
    periodType,
    periodStart,
    periodEnd,
    productUsage,
    prepaidBalanceCents,
    onDemandCapCents,
    onDemandUsedCents,
  };
}

function planFromSubscriptions(json: unknown): {
  planLabel: string;
  planId: GrokWebsiteUsage["planId"];
} {
  try {
    const subs = (json as { subscriptions?: Array<Record<string, unknown>> })?.subscriptions || [];
    const active = subs.find((s) => {
      const st = String(s.status || s.state || "").toLowerCase();
      return !st || st.includes("active") || st.includes("trial");
    }) || subs[0];
    if (!active) return { planLabel: "SuperGrok", planId: "super" };
    const tier = String(
      active.tier ||
        active.plan ||
        active.product ||
        active.name ||
        active.subscriptionTier ||
        "",
    ).toLowerCase();
    if (tier.includes("heavy") || tier.includes("pro")) {
      return { planLabel: "SuperGrok Heavy", planId: "heavy" };
    }
    if (tier.includes("plus")) return { planLabel: "SuperGrok Plus", planId: "plus" };
    if (tier.includes("lite")) return { planLabel: "SuperGrok Lite", planId: "lite" };
    if (tier.includes("free")) return { planLabel: "Free", planId: "free" };
    return { planLabel: "SuperGrok", planId: "super" };
  } catch {
    return { planLabel: "SuperGrok", planId: "super" };
  }
}

export type WebsiteAuth = {
  /** grok.com SSO cookie value (or full cookie header containing sso=) */
  ssoCookie?: string | null;
  /** Optional bearer — website usually wants SSO; accepted if management key */
  bearer?: string | null;
};

function buildCookieHeader(sso: string): string {
  const t = sso.trim();
  if (!t) return "";
  if (t.toLowerCase().includes("sso=")) return t;
  return `sso=${t}`;
}

/**
 * Fetch weekly SuperGrok usage from the same endpoint the Grok website uses.
 * In the browser, routes through our server proxy to avoid CORS.
 */
export async function fetchGrokWebsiteUsage(auth: WebsiteAuth): Promise<GrokWebsiteUsage> {
  const cookie = buildCookieHeader(auth.ssoCookie || "");
  const bearer = (auth.bearer || "").trim();
  if (!cookie && !bearer) {
    return {
      ok: false,
      error:
        "Connect your Grok website session (SSO) to load weekly SuperGrok usage. Device OAuth alone does not expose website quotas.",
      planLabel: "—",
      planId: "free",
      creditUsagePercent: 0,
      periodType: "unknown",
      periodStart: null,
      periodEnd: null,
      productUsage: [],
      prepaidBalanceCents: 0,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
    };
  }

  // Prefer local desktop bridge (no CORS), else same-origin proxy
  if (typeof window !== "undefined") {
    const desktop = window.grokhubDesktop?.grok;
    if (desktop?.websiteUsage) {
      try {
        return await desktop.websiteUsage({ ssoCookie: cookie, bearer });
      } catch {
        /* fall through */
      }
    }
    try {
      const res = await fetch("/api/grok", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          action: "websiteUsage",
          ssoCookie: cookie,
          bearer,
        }),
      });
      if (res.ok) {
        return (await res.json()) as GrokWebsiteUsage;
      }
    } catch {
      /* fall through to direct */
    }
  }

  return fetchGrokWebsiteUsageDirect(auth);
}

/** Direct call to grok.com (server / Electron main). */
export async function fetchGrokWebsiteUsageDirect(
  auth: WebsiteAuth,
): Promise<GrokWebsiteUsage> {
  const cookie = buildCookieHeader(auth.ssoCookie || "");
  const bearer = (auth.bearer || "").trim();
  if (!cookie && !bearer) {
    return {
      ok: false,
      error: "No Grok website session",
      planLabel: "—",
      planId: "free",
      creditUsagePercent: 0,
      periodType: "unknown",
      periodStart: null,
      periodEnd: null,
      productUsage: [],
      prepaidBalanceCents: 0,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
    };
  }

  const headers: Record<string, string> = {
    "content-type": "application/grpc-web+proto",
    "x-grpc-web": "1",
    accept: "application/grpc-web+proto",
    origin: "https://grok.com",
    referer: "https://grok.com/settings",
    "user-agent":
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36 GrokHub/0.8.4",
  };
  if (cookie) headers.cookie = cookie;
  if (bearer) headers.authorization = `Bearer ${bearer}`;

  try {
    const payload = grpcWebFrame(encodeCreditsRequest());
    const res = await fetch(GROK_CREDITS_URL, {
      method: "POST",
      headers,
      body: payload.buffer.slice(
        payload.byteOffset,
        payload.byteOffset + payload.byteLength,
      ) as ArrayBuffer,
    });

    const headerStatus = res.headers.get("grpc-status");
    const headerMsg = res.headers.get("grpc-message");
    const ab = new Uint8Array(await res.arrayBuffer());
    const parsed = parseGrpcWebResponse(ab);
    const status = parsed.status || (headerStatus != null ? Number(headerStatus) : 0);
    const message =
      parsed.message ||
      (headerMsg ? decodeURIComponent(headerMsg.replace(/\+/g, " ")) : "") ||
      (!res.ok ? `HTTP ${res.status}` : "");

    if (status !== 0 || !parsed.messages[0]) {
      return {
        ok: false,
        error:
          message ||
          (status === 16
            ? "Website session expired — re-link Grok SSO in Settings."
            : `Grok usage error (grpc ${status})`),
        planLabel: "—",
        planId: "free",
        creditUsagePercent: 0,
        periodType: "unknown",
        periodStart: null,
        periodEnd: null,
        productUsage: [],
        prepaidBalanceCents: 0,
        onDemandCapCents: 0,
        onDemandUsedCents: 0,
      };
    }

    const usage = parseCreditsConfig(parsed.messages[0]!);

    let planLabel = "SuperGrok";
    let planId: GrokWebsiteUsage["planId"] = "super";
    try {
      const subHeaders: Record<string, string> = {
        accept: "application/json",
        origin: "https://grok.com",
        referer: "https://grok.com/settings",
      };
      if (cookie) subHeaders.cookie = cookie;
      if (bearer) subHeaders.authorization = `Bearer ${bearer}`;
      const subRes = await fetch(SUBSCRIPTIONS_URL, { headers: subHeaders });
      if (subRes.ok) {
        const mapped = planFromSubscriptions(await subRes.json());
        planLabel = mapped.planLabel;
        planId = mapped.planId;
      }
    } catch {
      /* keep defaults */
    }

    // Title like the website when weekly + heavy heuristics
    if (usage.periodType === "weekly" && planId === "heavy") {
      planLabel = "SuperGrok Heavy";
    } else if (usage.periodType === "weekly" && planId === "pro") {
      planLabel = "SuperGrok Heavy";
      planId = "heavy";
    }

    return {
      ok: true,
      planLabel,
      planId,
      ...usage,
    };
  } catch (e) {
    return {
      ok: false,
      error: e instanceof Error ? e.message : "Failed to reach grok.com usage API",
      planLabel: "—",
      planId: "free",
      creditUsagePercent: 0,
      periodType: "unknown",
      periodStart: null,
      periodEnd: null,
      productUsage: [],
      prepaidBalanceCents: 0,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
    };
  }
}

export function formatResetAt(ts: number | null): string {
  if (!ts) return "—";
  try {
    return new Date(ts).toLocaleString(undefined, {
      month: "long",
      day: "numeric",
      year: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return "—";
  }
}

export function formatUsdFromCents(cents: number): string {
  const n = (cents || 0) / 100;
  return n.toLocaleString(undefined, { style: "currency", currency: "USD" });
}
