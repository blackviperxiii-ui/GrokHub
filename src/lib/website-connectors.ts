/**
 * Discover Grok website "Installed" connectors using the linked SSO session.
 * Best-effort: REST candidates + HTML parse of skills/connectors page.
 */
import { mapWebsiteConnectorName } from "./connector-tools";

export type WebsiteConnectorHit = {
  id: string;
  name: string;
  accountLabel?: string | null;
  status: "connected" | "disconnected";
  raw?: string;
};

export type WebsiteConnectorsResult = {
  ok: boolean;
  source: "rest" | "html" | "electron" | "none";
  connectors: WebsiteConnectorHit[];
  detail: string;
};

const REST_CANDIDATES = [
  "https://grok.com/rest/connectors",
  "https://grok.com/rest/apps",
  "https://grok.com/rest/integrations",
  "https://grok.com/rest/user/connectors",
  "https://grok.com/rest/skills/connectors",
  "https://grok.com/api/connectors",
];

function cookieHeader(ssoCookie: string): string {
  const c = ssoCookie.trim();
  if (!c) return "";
  if (c.includes("=")) return c;
  return `sso=${c}`;
}

function normalizeList(payload: unknown): WebsiteConnectorHit[] {
  const hits: WebsiteConnectorHit[] = [];
  const push = (name: string, account?: string | null, connected = true) => {
    const id = mapWebsiteConnectorName(name);
    if (!id) return;
    if (hits.some((h) => h.id === id)) return;
    hits.push({
      id,
      name,
      accountLabel: account || null,
      status: connected ? "connected" : "disconnected",
    });
  };

  const walk = (node: unknown, depth = 0) => {
    if (depth > 8 || node == null) return;
    if (Array.isArray(node)) {
      for (const item of node) walk(item, depth + 1);
      return;
    }
    if (typeof node !== "object") return;
    const o = node as Record<string, unknown>;
    const name = String(
      o.name || o.displayName || o.title || o.provider || o.appName || o.connectorName || "",
    ).trim();
    const status = String(o.status || o.state || o.connectionStatus || "").toLowerCase();
    const account = String(
      o.email || o.account || o.accountEmail || o.userEmail || o.username || o.login || "",
    ).trim();
    const connected =
      status.includes("connect") ||
      o.connected === true ||
      o.isConnected === true ||
      o.installed === true ||
      (!status && Boolean(name));
    if (name && (connected || account)) {
      push(name, account || null, connected || Boolean(account));
    }
    for (const v of Object.values(o)) {
      if (v && typeof v === "object") walk(v, depth + 1);
    }
  };
  walk(payload);
  return hits;
}

function parseConnectorsHtml(html: string): WebsiteConnectorHit[] {
  const hits: WebsiteConnectorHit[] = [];
  // Look for known product names near "Connected"
  const names = [
    "GitHub",
    "Notion",
    "Microsoft Teams",
    "Outlook Calendar",
    "Outlook",
    "Google Calendar",
    "Google Drive",
    "Gmail",
    "Box",
    "Canva",
    "Stripe",
    "Vercel",
    "Linear",
  ];
  const lower = html;
  for (const name of names) {
    const re = new RegExp(
      `${name}[\\s\\S]{0,240}?(Connected|connected|[\\w.+-]+@[\\w.-]+)`,
      "i",
    );
    const m = lower.match(re);
    if (!m) continue;
    const id = mapWebsiteConnectorName(name);
    if (!id || hits.some((h) => h.id === id)) continue;
    const tail = m[1] || "";
    const email = /@/.test(tail) ? tail : null;
    // Only count if Connected or email nearby
    if (!/connected/i.test(m[0]) && !email) continue;
    hits.push({
      id,
      name,
      accountLabel: email,
      status: "connected",
      raw: m[0].slice(0, 120),
    });
  }
  return hits;
}

export async function fetchWebsiteConnectors(opts: {
  ssoCookie?: string;
  bearer?: string;
}): Promise<WebsiteConnectorsResult> {
  // Electron main path
  if (typeof window !== "undefined" && window.grokhubDesktop?.grok?.websiteConnectors) {
    try {
      const r = await window.grokhubDesktop.grok.websiteConnectors({
        ssoCookie: opts.ssoCookie,
        bearer: opts.bearer,
      });
      if (r?.connectors?.length) {
        return {
          ok: true,
          source: "electron",
          connectors: r.connectors.map((c) => ({
            id: c.id,
            name: c.name,
            accountLabel: c.accountLabel,
            status: (c.status === "disconnected" ? "disconnected" : "connected") as
              | "connected"
              | "disconnected",
          })),
          detail: r.detail || `Synced ${r.connectors.length} from website session`,
        };
      }
      if (r && r.ok === false) {
        return {
          ok: false,
          source: "electron",
          connectors: [],
          detail: r.detail || "Website connector sync failed",
        };
      }
    } catch (e) {
      /* fall through */
    }
  }

  const cookie = cookieHeader(opts.ssoCookie || "");
  if (!cookie && !opts.bearer) {
    return {
      ok: false,
      source: "none",
      connectors: [],
      detail: "Link Grok website session in Settings to sync Installed connectors",
    };
  }

  const headers: Record<string, string> = {
    accept: "application/json, text/html",
    "user-agent":
      "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  };
  if (cookie) headers.cookie = cookie;
  if (opts.bearer) headers.authorization = `Bearer ${opts.bearer}`;

  // REST probes
  for (const url of REST_CANDIDATES) {
    try {
      const res = await fetch(url, { headers, credentials: "include" });
      if (!res.ok) continue;
      const ct = res.headers.get("content-type") || "";
      if (ct.includes("json")) {
        const json = await res.json();
        const list = normalizeList(json);
        if (list.length) {
          return {
            ok: true,
            source: "rest",
            connectors: list,
            detail: `REST ${url} · ${list.length} connectors`,
          };
        }
      }
    } catch {
      /* next */
    }
  }

  // HTML pages that may list skills/connectors
  for (const url of [
    "https://grok.com/skills",
    "https://grok.com/connectors",
    "https://grok.com/app",
    "https://grok.com/",
  ]) {
    try {
      const res = await fetch(url, { headers, credentials: "include" });
      if (!res.ok) continue;
      const html = await res.text();
      if (html.length < 200) continue;
      const list = parseConnectorsHtml(html);
      if (list.length) {
        return {
          ok: true,
          source: "html",
          connectors: list,
          detail: `Parsed ${url} · ${list.length} connected`,
        };
      }
    } catch {
      /* next */
    }
  }

  return {
    ok: false,
    source: "none",
    connectors: [],
    detail: "Could not list website connectors — re-link Grok website session",
  };
}
