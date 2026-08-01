/**
 * Grok website SSO session + usage fetch for Electron main.
 * Uses a persistent partition so login cookies stick and API calls reuse them.
 */
const { BrowserWindow, session, shell } = require("electron");

const PARTITION = "persist:grokhub-grok-web";
const CREDITS_URL =
  "https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
const SUBSCRIPTIONS_URL = "https://grok.com/rest/subscriptions";

const CHROME_UA =
  "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

function grokSession() {
  return session.fromPartition(PARTITION);
}

function grpcWebFrame(payload) {
  const out = Buffer.alloc(5 + payload.length);
  out[0] = 0;
  out.writeUInt32BE(payload.length, 1);
  Buffer.from(payload).copy(out, 5);
  return out;
}

/** Prefer real consumer session cookies from the login partition. */
async function collectSessionCookies() {
  const ses = grokSession();
  const buckets = await Promise.all([
    ses.cookies.get({ domain: "grok.com" }),
    ses.cookies.get({ domain: ".grok.com" }),
    ses.cookies.get({ url: "https://grok.com" }),
    ses.cookies.get({ domain: "x.ai" }),
    ses.cookies.get({ domain: ".x.ai" }),
    ses.cookies.get({ url: "https://accounts.x.ai" }),
  ]);
  const byKey = new Map();
  for (const list of buckets) {
    for (const c of list || []) {
      byKey.set(`${c.domain}|${c.name}`, c);
    }
  }
  const all = [...byKey.values()];
  const sso =
    all.find((c) => c.name === "sso") ||
    all.find((c) => c.name === "sso-rw") ||
    all.find((c) => /sso/i.test(c.name));
  const cookieHeader = all
    .filter(
      (c) =>
        (c.domain || "").includes("grok.com") ||
        (c.domain || "").includes("x.ai") ||
        /sso|session|auth|token/i.test(c.name),
    )
    .map((c) => `${c.name}=${c.value}`)
    .join("; ");
  return {
    sso: sso ? `${sso.name}=${sso.value}` : "",
    cookieHeader: cookieHeader || (sso ? `${sso.name}=${sso.value}` : ""),
    count: all.length,
    names: all.map((c) => c.name),
  };
}

/**
 * Open an in-app browser so the user can sign in to grok.com.
 * Waits until an SSO-like cookie appears (or the window is closed).
 */
function linkWebsiteSession() {
  return new Promise((resolve) => {
    const ses = grokSession();
    const win = new BrowserWindow({
      width: 1040,
      height: 820,
      minWidth: 720,
      minHeight: 560,
      title: "Sign in to Grok — then this window closes",
      autoHideMenuBar: true,
      backgroundColor: "#0a0a0a",
      show: false,
      webPreferences: {
        session: ses,
        nodeIntegration: false,
        contextIsolation: true,
        sandbox: false,
        webSecurity: true,
      },
    });

    win.webContents.setUserAgent(CHROME_UA);

    let settled = false;
    const done = (payload) => {
      if (settled) return;
      settled = true;
      try {
        if (!win.isDestroyed()) win.close();
      } catch {
        /* ignore */
      }
      resolve(payload);
    };

    const tryCapture = async (force = false) => {
      if (settled) return;
      try {
        const { sso, cookieHeader, count, names } = await collectSessionCookies();
        if (sso) {
          // Mirror into default session too (optional helpers)
          try {
            const name = sso.split("=")[0];
            const value = sso.slice(name.length + 1);
            await session.defaultSession.cookies.set({
              url: "https://grok.com",
              name,
              value,
              domain: ".grok.com",
              path: "/",
              secure: true,
              httpOnly: true,
              expirationDate: Math.floor(Date.now() / 1000) + 60 * 60 * 24 * 30,
            });
          } catch {
            /* ignore */
          }
          done({ cookie: sso, cookieHeader, names });
          return;
        }
        // Some sessions only set a long combined auth cookie after home load
        if (cookieHeader && /sso=|session/i.test(cookieHeader) && count >= 2) {
          done({ cookie: cookieHeader, cookieHeader, names });
          return;
        }
        if (force) {
          done({
            error:
              count === 0
                ? "No grok.com cookies yet. Sign in fully (home chat must load), then click Link again."
                : `Signed in cookies found (${names.join(", ") || "none"}) but no sso cookie. Paste sso=… from DevTools → Application → Cookies if needed.`,
            names,
          });
        }
      } catch (e) {
        if (force) {
          done({ error: e instanceof Error ? e.message : "cookie capture failed" });
        }
      }
    };

    win.once("ready-to-show", () => {
      if (!win.isDestroyed()) win.show();
    });

    win.webContents.on("did-fail-load", (_e, code, desc, url, isMain) => {
      if (!isMain || settled) return;
      // -3 = aborted (redirects) — ignore
      if (code === -3) return;
      const html = `<!doctype html><html><body style="font-family:system-ui;background:#111;color:#eee;padding:2rem;max-width:36rem">
        <h1 style="font-size:1.25rem">Could not load Grok</h1>
        <p style="color:#aaa;line-height:1.5">Error ${code}: ${desc || "unknown"}</p>
        <p style="color:#aaa;line-height:1.5;word-break:break-all">${url || ""}</p>
        <p><a href="https://grok.com/" style="color:#7dd3fc">Retry grok.com</a>
        · <a href="https://accounts.x.ai/sign-in" style="color:#7dd3fc">xAI sign-in</a></p>
        <p style="color:#888;font-size:0.85rem">If this stays blank, open grok.com in Firefox, copy the <code>sso</code> cookie, and paste it in GrokHub Settings → Usage.</p>
      </body></html>`;
      win.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`);
    });

    // Keep external targets inside this window when possible
    win.webContents.setWindowOpenHandler(({ url }) => {
      if (/grok\.com|x\.ai|x\.com|twitter\.com|accounts\./i.test(url)) {
        win.loadURL(url);
        return { action: "deny" };
      }
      shell.openExternal(url);
      return { action: "deny" };
    });

    win.webContents.on("did-navigate", () => void tryCapture(false));
    win.webContents.on("did-navigate-in-page", () => void tryCapture(false));
    win.webContents.on("did-finish-load", () => void tryCapture(false));

    const poll = setInterval(() => void tryCapture(false), 1200);
    win.on("closed", () => {
      clearInterval(poll);
      if (!settled) {
        settled = true;
        resolve({ error: "Window closed before Grok website session was linked" });
      }
    });
    setTimeout(() => {
      clearInterval(poll);
      void tryCapture(true);
    }, 5 * 60 * 1000);

    // Prefer login entry that actually renders in Chromium embeds
    const start =
      process.env.GROKHUB_GROK_LOGIN_URL ||
      "https://grok.com/?_gh=1";
    win.loadURL(start, {
      userAgent: CHROME_UA,
      httpReferrer: "https://grok.com/",
    });
  });
}

async function readSessionCookieHeader(fallbackSso) {
  const fromSes = await collectSessionCookies();
  if (fromSes.cookieHeader) return fromSes.cookieHeader;
  if (fromSes.sso) return fromSes.sso;
  const t = String(fallbackSso || "").trim();
  if (!t) return "";
  if (/sso=/i.test(t) || t.includes("=")) return t;
  return `sso=${t}`;
}

function parseGrpcWeb(buf) {
  let i = 0;
  const messages = [];
  let status = 0;
  let message = "";
  while (i + 5 <= buf.length) {
    const flag = buf[i];
    const len = buf.readUInt32BE(i + 1);
    i += 5;
    if (i + len > buf.length) break;
    const chunk = buf.subarray(i, i + len);
    i += len;
    if (flag === 0) messages.push(chunk);
    else if (flag === 0x80) {
      const text = chunk.toString("utf8");
      const sm = /grpc-status:\s*(\d+)/i.exec(text);
      const mm = /grpc-message:\s*([^\r\n]+)/i.exec(text);
      if (sm) status = Number(sm[1]);
      if (mm) message = decodeURIComponent(mm[1].replace(/\+/g, " "));
    }
  }
  return { status, message, messages };
}

// Minimal protobuf helpers for credit_usage_percent double at config.field2
function readVarint(buf, offset) {
  let result = 0;
  let shift = 0;
  let pos = offset;
  while (pos < buf.length) {
    const b = buf[pos++];
    result |= (b & 0x7f) << shift;
    if ((b & 0x80) === 0) break;
    shift += 7;
    if (shift > 35) break;
  }
  return { value: result >>> 0, next: pos };
}

function decodeFields(buf) {
  const out = {};
  let i = 0;
  while (i < buf.length) {
    const tag = readVarint(buf, i);
    i = tag.next;
    const field = tag.value >>> 3;
    const wire = tag.value & 7;
    if (field === 0) break;
    if (wire === 0) {
      const v = readVarint(buf, i);
      i = v.next;
      (out[field] ||= []).push(v.value);
    } else if (wire === 1) {
      if (i + 8 > buf.length) break;
      (out[field] ||= []).push(buf.subarray(i, i + 8));
      i += 8;
    } else if (wire === 2) {
      const len = readVarint(buf, i);
      i = len.next;
      (out[field] ||= []).push(buf.subarray(i, i + len.value));
      i += len.value;
    } else if (wire === 5) {
      if (i + 4 > buf.length) break;
      (out[field] ||= []).push(buf.subarray(i, i + 4));
      i += 4;
    } else break;
  }
  return out;
}

function emptyUsage(error) {
  return {
    ok: false,
    error,
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

/**
 * Fetch website usage using the Electron session (cookies auto-attached).
 * Falls back to Cookie header if partition is empty.
 */
async function fetchWebsiteUsage(opts = {}) {
  const cookieHeader = await readSessionCookieHeader(opts.ssoCookie);
  if (!cookieHeader) {
    return emptyUsage(
      "No Grok website session. Click Link Grok website and sign in until chat loads.",
    );
  }

  try {
    const ses = grokSession();
    const body = grpcWebFrame(Buffer.from([0x08, 0x01]));
    const headers = {
      "content-type": "application/grpc-web+proto",
      accept: "application/grpc-web+proto",
      "x-grpc-web": "1",
      "x-user-agent": "grokhub-desktop",
      origin: "https://grok.com",
      referer: "https://grok.com/",
      cookie: cookieHeader,
      "user-agent": CHROME_UA,
    };
    if (opts.bearer) headers.authorization = `Bearer ${opts.bearer}`;

    // Prefer session.fetch so partition cookies merge with explicit Cookie header
    const res = await ses.fetch(CREDITS_URL, {
      method: "POST",
      headers,
      body,
    });
    const ab = await res.arrayBuffer();
    const buf = Buffer.from(ab);
    if (!res.ok) {
      return emptyUsage(
        `grok.com usage HTTP ${res.status} — re-link website session (sign in fully)`,
      );
    }
    const parsed = parseGrpcWeb(buf);
    if (parsed.status && parsed.status !== 0) {
      return emptyUsage(
        parsed.message || `grpc-status ${parsed.status} — re-link website session`,
      );
    }
    const msg = parsed.messages[0];
    if (!msg) {
      return emptyUsage("Empty usage response — try re-linking after chat loads");
    }

    // Prefer server-side full parser when local UI API is up
    try {
      const local = await fetch("http://127.0.0.1:8080/api/grok", {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          action: "websiteUsage",
          ssoCookie: cookieHeader,
          bearer: opts.bearer || "",
        }),
      });
      if (local.ok) {
        const data = await local.json();
        if (data && (data.ok || data.creditUsagePercent != null)) return data;
      }
    } catch {
      /* offline local API */
    }

    // Minimal inline parse for credit_usage_percent
    const root = decodeFields(msg);
    const config = root[1] && root[1][0] instanceof Buffer ? decodeFields(root[1][0]) : {};
    let creditUsagePercent = 0;
    if (config[2] && config[2][0] instanceof Buffer && config[2][0].length === 8) {
      creditUsagePercent = config[2][0].readDoubleLE(0);
    }

    let planLabel = "SuperGrok";
    try {
      const subRes = await ses.fetch(SUBSCRIPTIONS_URL, {
        headers: {
          accept: "application/json",
          cookie: cookieHeader,
          "user-agent": CHROME_UA,
          origin: "https://grok.com",
          referer: "https://grok.com/",
        },
      });
      if (subRes.ok) {
        const sub = await subRes.json();
        const name =
          sub?.tier?.name ||
          sub?.subscription?.tier?.name ||
          sub?.plan?.name ||
          sub?.displayName;
        if (name) planLabel = String(name);
      }
    } catch {
      /* ignore */
    }

    return {
      ok: true,
      planLabel,
      planId: /heavy/i.test(planLabel) ? "heavy" : /pro/i.test(planLabel) ? "pro" : "super",
      creditUsagePercent: Number.isFinite(creditUsagePercent) ? creditUsagePercent : 0,
      periodType: "weekly",
      periodStart: null,
      periodEnd: null,
      productUsage: [],
      prepaidBalanceCents: 0,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
    };
  } catch (e) {
    return emptyUsage(e instanceof Error ? e.message : "usage fetch failed");
  }
}

async function getStoredSso() {
  const { sso, cookieHeader } = await collectSessionCookies();
  return { cookie: sso || cookieHeader || "" };
}

const CONNECTOR_REST = [
  "https://grok.com/rest/connectors",
  "https://grok.com/rest/apps",
  "https://grok.com/rest/integrations",
  "https://grok.com/rest/user/connectors",
];

const CONNECTOR_PAGES = [
  "https://grok.com/skills",
  "https://grok.com/connectors",
  "https://grok.com/",
];

const KNOWN_NAMES = [
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

function mapConnectorName(name) {
  const k = String(name || "")
    .trim()
    .toLowerCase();
  const aliases = {
    github: "github",
    notion: "notion",
    "microsoft teams": "teams",
    teams: "teams",
    outlook: "outlook",
    "outlook calendar": "outlook-calendar",
    "google calendar": "google-calendar",
    "google drive": "gdrive",
    gmail: "gmail",
    box: "box",
    canva: "canva",
    stripe: "stripe",
    vercel: "vercel",
    linear: "linear",
  };
  if (aliases[k]) return aliases[k];
  for (const [a, id] of Object.entries(aliases)) {
    if (k.includes(a)) return id;
  }
  return null;
}

function parseHtmlConnectors(html) {
  const hits = [];
  for (const name of KNOWN_NAMES) {
    const re = new RegExp(
      name + "[\\s\\S]{0,240}?(Connected|connected|[\\w.+-]+@[\\w.-]+)",
      "i",
    );
    const m = String(html).match(re);
    if (!m) continue;
    const id = mapConnectorName(name);
    if (!id || hits.some((h) => h.id === id)) continue;
    const tail = m[1] || "";
    const email = /@/.test(tail) ? tail : null;
    if (!/connected/i.test(m[0]) && !email) continue;
    hits.push({ id, name, accountLabel: email, status: "connected" });
  }
  return hits;
}

function walkJsonConnectors(node, hits, depth) {
  if (depth > 8 || node == null) return;
  if (Array.isArray(node)) {
    for (const item of node) walkJsonConnectors(item, hits, depth + 1);
    return;
  }
  if (typeof node !== "object") return;
  const o = node;
  const name = String(
    o.name || o.displayName || o.title || o.provider || o.appName || "",
  ).trim();
  const account = String(
    o.email || o.account || o.accountEmail || o.userEmail || o.username || "",
  ).trim();
  const status = String(o.status || o.state || "").toLowerCase();
  const connected =
    status.includes("connect") ||
    o.connected === true ||
    o.isConnected === true ||
    o.installed === true;
  if (name && (connected || account)) {
    const id = mapConnectorName(name);
    if (id && !hits.some((h) => h.id === id)) {
      hits.push({
        id,
        name,
        accountLabel: account || null,
        status: "connected",
      });
    }
  }
  for (const v of Object.values(o)) {
    if (v && typeof v === "object") walkJsonConnectors(v, hits, depth + 1);
  }
}

/**
 * List Grok website Installed connectors using the persistent login partition.
 */
async function fetchWebsiteConnectors(opts = {}) {
  const ses = grokSession();
  const collected = await collectSessionCookies();
  let cookie = String(opts.ssoCookie || collected.cookieHeader || collected.sso || "").trim();
  if (cookie && !cookie.includes("=")) cookie = `sso=${cookie}`;
  if (!cookie) {
    return {
      ok: false,
      connectors: [],
      detail: "No website session — link Grok website first",
    };
  }

  const headers = {
    accept: "application/json, text/html, */*",
    "user-agent": CHROME_UA,
    cookie,
  };

  for (const url of CONNECTOR_REST) {
    try {
      const res = await ses.fetch(url, { method: "GET", headers });
      if (!res.ok) continue;
      const ct = res.headers.get("content-type") || "";
      if (!ct.includes("json")) continue;
      const json = await res.json();
      const hits = [];
      walkJsonConnectors(json, hits, 0);
      if (hits.length) {
        return { ok: true, connectors: hits, detail: `REST ${url} · ${hits.length}` };
      }
    } catch {
      /* next */
    }
  }

  for (const url of CONNECTOR_PAGES) {
    try {
      const res = await ses.fetch(url, { method: "GET", headers });
      if (!res.ok) continue;
      const html = await res.text();
      const hits = parseHtmlConnectors(html);
      if (hits.length) {
        return { ok: true, connectors: hits, detail: `HTML ${url} · ${hits.length}` };
      }
    } catch {
      /* next */
    }
  }

  return {
    ok: false,
    connectors: [],
    detail:
      "No connectors found — open Grok website → Skills and Connectors, ensure they show Connected, then re-link",
  };
}

module.exports = {
  PARTITION,
  linkWebsiteSession,
  fetchWebsiteUsage,
  fetchWebsiteConnectors,
  getStoredSso,
  collectSessionCookies,
};
