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

/**
 * Collect every cookie from the Grok partition (and related domains).
 * Grok has used `sso`, `sso-rw`, and other session names over time.
 */
async function collectSessionCookies() {
  const ses = grokSession();
  let all = [];
  try {
    // Full partition dump is most reliable
    all = await ses.cookies.get({});
  } catch {
    all = [];
  }
  if (!all.length) {
    const buckets = await Promise.all([
      ses.cookies.get({ domain: "grok.com" }).catch(() => []),
      ses.cookies.get({ domain: ".grok.com" }).catch(() => []),
      ses.cookies.get({ url: "https://grok.com" }).catch(() => []),
      ses.cookies.get({ domain: "x.ai" }).catch(() => []),
      ses.cookies.get({ domain: ".x.ai" }).catch(() => []),
      ses.cookies.get({ url: "https://accounts.x.ai" }).catch(() => []),
      ses.cookies.get({ url: "https://grok.x.ai" }).catch(() => []),
    ]);
    const byKey = new Map();
    for (const list of buckets) {
      for (const c of list || []) {
        byKey.set(`${c.domain}|${c.name}`, c);
      }
    }
    all = [...byKey.values()];
  }

  const names = all.map((c) => c.name);
  // Priority order for auth cookies
  const ssoPick =
    all.find((c) => c.name === "sso" && c.value && c.value.length > 8) ||
    all.find((c) => c.name === "sso-rw" && c.value && c.value.length > 8) ||
    all.find((c) => /^sso/i.test(c.name) && c.value && c.value.length > 8) ||
    all.find(
      (c) =>
        /session|auth|token|cf_clearance|__Secure-next|next-auth/i.test(c.name) &&
        c.value &&
        c.value.length > 12 &&
        ((c.domain || "").includes("grok") || (c.domain || "").includes("x.ai")),
    );

  const relevant = all.filter(
    (c) =>
      (c.domain || "").includes("grok.com") ||
      (c.domain || "").includes("x.ai") ||
      /sso|session|auth|token|cf_clearance/i.test(c.name),
  );

  const cookieHeader = relevant.map((c) => `${c.name}=${c.value}`).join("; ");
  const sso = ssoPick ? `${ssoPick.name}=${ssoPick.value}` : "";

  // "Signed in enough" heuristic: sso cookie OR (grok.com cookies + any long-lived auth-ish cookie)
  const grokCookies = all.filter((c) => (c.domain || "").includes("grok"));
  const signedIn =
    Boolean(sso) ||
    (grokCookies.length >= 1 &&
      relevant.some((c) => c.value && c.value.length > 20 && /sso|session|token|auth/i.test(c.name)));

  return {
    sso,
    cookieHeader: cookieHeader || sso,
    count: all.length,
    names,
    signedIn,
    domains: [...new Set(all.map((c) => c.domain).filter(Boolean))],
  };
}

/**
 * Inject a pasted cookie string into the persistent partition so session.fetch works.
 */
async function injectCookieHeader(raw) {
  const text = String(raw || "").trim();
  if (!text) return { ok: false, error: "empty cookie" };
  const ses = grokSession();
  // Accept "sso=VALUE" or full "a=1; b=2" or bare token
  let pairs = [];
  if (text.includes("=")) {
    pairs = text.split(/;\s*/).map((p) => {
      const i = p.indexOf("=");
      if (i < 0) return null;
      return { name: p.slice(0, i).trim(), value: p.slice(i + 1).trim() };
    }).filter(Boolean);
  } else {
    pairs = [{ name: "sso", value: text }];
  }
  const expires = Math.floor(Date.now() / 1000) + 60 * 60 * 24 * 60;
  for (const { name, value } of pairs) {
    if (!name || !value) continue;
    const domains = name.toLowerCase().includes("cf")
      ? [".grok.com", "grok.com"]
      : [".grok.com", "grok.com", ".x.ai"];
    for (const domain of domains) {
      try {
        await ses.cookies.set({
          url: domain.includes("x.ai") ? "https://accounts.x.ai" : "https://grok.com",
          name,
          value,
          domain,
          path: "/",
          secure: true,
          httpOnly: !/cf_clearance/i.test(name),
          expirationDate: expires,
        });
      } catch {
        try {
          await ses.cookies.set({
            url: "https://grok.com",
            name,
            value,
            path: "/",
            secure: true,
            expirationDate: expires,
          });
        } catch {
          /* ignore */
        }
      }
    }
  }
  const collected = await collectSessionCookies();
  return {
    ok: Boolean(collected.sso || collected.cookieHeader),
    cookie: collected.sso || collected.cookieHeader || pairs.map((p) => `${p.name}=${p.value}`).join("; "),
    cookieHeader: collected.cookieHeader,
    names: collected.names,
  };
}

/**
 * Open an in-app browser so the user can sign in to grok.com.
 * Does NOT auto-close on the first cookie blip — waits for a real session
 * or the user clicking "I'm signed in" (injected toolbar) / window close after success.
 */
function linkWebsiteSession() {
  return new Promise((resolve) => {
    const ses = grokSession();
    const win = new BrowserWindow({
      width: 1100,
      height: 860,
      minWidth: 720,
      minHeight: 560,
      title: "Sign in to Grok — wait for chat home, then click “Use this session”",
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

    const captureIfReady = async (opts = {}) => {
      if (settled) return false;
      const force = Boolean(opts.force);
      const { sso, cookieHeader, count, names, signedIn, domains } =
        await collectSessionCookies();

      // Prefer explicit sso
      if (sso && sso.length > 12) {
        done({
          cookie: sso,
          cookieHeader: cookieHeader || sso,
          names,
          domains,
        });
        return true;
      }

      // Full cookie header once signed-in heuristic passes
      if (signedIn && cookieHeader && cookieHeader.length > 20) {
        done({
          cookie: cookieHeader,
          cookieHeader,
          names,
          domains,
        });
        return true;
      }

      // User pressed "Use this session"
      if (force) {
        if (cookieHeader && count > 0) {
          done({
            cookie: sso || cookieHeader,
            cookieHeader,
            names,
            domains,
          });
          return true;
        }
        done({
          error:
            count === 0
              ? "No cookies yet. Finish sign-in until the Grok chat UI loads, then click “Use this session” again."
              : `Cookies present (${names.slice(0, 12).join(", ") || "unnamed"}) but session looks incomplete. Stay on grok.com chat home and retry, or paste sso= from browser DevTools.`,
          names,
          domains,
        });
        return true;
      }
      return false;
    };

    // Floating capture bar via executeJavaScript on each navigation
    const injectToolbar = async () => {
      if (settled || win.isDestroyed()) return;
      try {
        await win.webContents.executeJavaScript(`
          (function () {
            if (document.getElementById('grokhub-session-bar')) return;
            var bar = document.createElement('div');
            bar.id = 'grokhub-session-bar';
            bar.style.cssText = 'position:fixed;z-index:2147483647;left:12px;right:12px;bottom:12px;display:flex;gap:8px;align-items:center;justify-content:center;padding:10px 14px;border-radius:14px;background:rgba(12,12,14,0.94);border:1px solid rgba(255,255,255,0.12);box-shadow:0 12px 40px rgba(0,0,0,.45);font:600 13px system-ui,sans-serif;color:#f4f4f5';
            bar.innerHTML = '<span style="opacity:.85;font-weight:500">When Grok chat is visible, capture the session:</span>';
            var btn = document.createElement('button');
            btn.textContent = 'Use this session';
            btn.style.cssText = 'border:0;border-radius:999px;padding:8px 14px;background:#f4f4f5;color:#0a0a0a;font:600 13px system-ui,sans-serif;cursor:pointer';
            btn.onclick = function () {
              document.title = 'GROKHUB_CAPTURE_SESSION';
              btn.textContent = 'Capturing…';
            };
            bar.appendChild(btn);
            document.documentElement.appendChild(bar);
          })();
        `, true);
      } catch {
        /* page may not allow */
      }
    };

    win.once("ready-to-show", () => {
      if (!win.isDestroyed()) win.show();
    });

    win.webContents.on("did-fail-load", (_e, code, desc, url, isMain) => {
      if (!isMain || settled) return;
      if (code === -3) return;
      const html = `<!doctype html><html><body style="font-family:system-ui;background:#111;color:#eee;padding:2rem;max-width:40rem;margin:auto">
        <h1 style="font-size:1.25rem">Could not load Grok</h1>
        <p style="color:#aaa;line-height:1.5">Error ${code}: ${desc || "unknown"}</p>
        <p style="color:#aaa;word-break:break-all">${url || ""}</p>
        <p><a href="https://grok.com/" style="color:#7dd3fc">Retry grok.com</a>
        · <a href="https://accounts.x.ai/sign-in?redirect=grok-com" style="color:#7dd3fc">xAI sign-in</a></p>
        <p style="color:#888;font-size:0.9rem;line-height:1.5">If the embed stays blank, open <b>grok.com</b> in Firefox/Chrome, DevTools → Application → Cookies → copy the <code>sso</code> value, and paste it in GrokHub.</p>
      </body></html>`;
      win.loadURL(`data:text/html;charset=utf-8,${encodeURIComponent(html)}`);
    });

    win.webContents.setWindowOpenHandler(({ url }) => {
      if (/grok\.com|x\.ai|x\.com|twitter\.com|accounts\.|google\.|apple\.|github\./i.test(url)) {
        win.loadURL(url);
        return { action: "deny" };
      }
      shell.openExternal(url);
      return { action: "deny" };
    });

    // Capture toolbar "Use this session" via title change
    win.webContents.on("page-title-updated", (_e, title) => {
      if (title === "GROKHUB_CAPTURE_SESSION") {
        void captureIfReady({ force: true });
      }
    });

    win.webContents.on("did-navigate", () => {
      void injectToolbar();
      void captureIfReady(false);
    });
    win.webContents.on("did-navigate-in-page", () => void captureIfReady(false));
    win.webContents.on("did-finish-load", () => {
      void injectToolbar();
      void captureIfReady(false);
    });
    win.webContents.on("did-redirect-navigation", () => void captureIfReady(false));

    // Cookie store changes (Electron 30+)
    try {
      ses.cookies.on("changed", () => {
        void captureIfReady(false);
      });
    } catch {
      /* older electron */
    }

    const poll = setInterval(() => {
      void injectToolbar();
      void captureIfReady(false);
    }, 1500);

    win.on("closed", async () => {
      clearInterval(poll);
      if (settled) return;
      // Last chance: user closed after signing in
      const { sso, cookieHeader, signedIn, names } = await collectSessionCookies();
      if (sso || (signedIn && cookieHeader)) {
        settled = true;
        resolve({
          cookie: sso || cookieHeader,
          cookieHeader: cookieHeader || sso,
          names,
        });
        return;
      }
      settled = true;
      resolve({
        error:
          "Window closed before a Grok session was captured. Sign in until chat loads, click “Use this session”, or paste the sso cookie in Settings.",
        names,
      });
    });

    // Hard timeout 8 minutes
    setTimeout(() => {
      clearInterval(poll);
      void captureIfReady({ force: true });
    }, 8 * 60 * 1000);

    // Start at accounts sign-in with return to grok — more reliable than bare grok.com in embeds
    const start =
      process.env.GROKHUB_GROK_LOGIN_URL ||
      "https://accounts.x.ai/sign-in?redirect=https%3A%2F%2Fgrok.com%2F";
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
  // If user pasted a raw value, inject it for this process too
  if (t && !fromSes.count) {
    try {
      await injectCookieHeader(t);
      const again = await collectSessionCookies();
      if (again.cookieHeader) return again.cookieHeader;
    } catch {
      /* ignore */
    }
  }
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
      "No Grok website session. Click Link Grok website and sign in until chat loads, then “Use this session”.",
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
      "user-agent": CHROME_UA,
      cookie: cookieHeader,
    };

    // Prefer session.fetch so partition cookies merge with explicit Cookie header
    let res;
    try {
      res = await ses.fetch(CREDITS_URL, {
        method: "POST",
        headers,
        body,
      });
    } catch {
      res = await fetch(CREDITS_URL, { method: "POST", headers, body });
    }

    const ab = Buffer.from(await res.arrayBuffer());
    if (!res.ok) {
      // Try subscriptions JSON fallback
      try {
        const subRes = await ses.fetch(SUBSCRIPTIONS_URL, {
          method: "GET",
          headers: {
            accept: "application/json",
            cookie: cookieHeader,
            "user-agent": CHROME_UA,
            referer: "https://grok.com/",
          },
        });
        if (subRes.ok) {
          const json = await subRes.json();
          return {
            ok: true,
            planLabel: json?.plan || json?.tier || "Grok",
            planId: "super",
            creditUsagePercent: Number(json?.usagePercent || json?.percent || 0) || 0,
            periodType: "unknown",
            periodStart: null,
            periodEnd: null,
            productUsage: [],
            prepaidBalanceCents: 0,
            onDemandCapCents: 0,
            onDemandUsedCents: 0,
            raw: "subscriptions",
            ssoCookie: cookieHeader,
          };
        }
      } catch {
        /* fall through */
      }
      return emptyUsage(`Usage API ${res.status} — re-link website session (cookie may be stale)`);
    }

    const parsed = parseGrpcWeb(ab);
    if (parsed.status && parsed.status !== 0) {
      return emptyUsage(
        parsed.message || `grpc-status ${parsed.status} — re-link Grok website session`,
      );
    }

    let creditUsagePercent = 0;
    let planLabel = "Grok";
    let periodEnd = null;
    let periodStart = null;
    let prepaidBalanceCents = 0;
    let productUsage = [];

    for (const msg of parsed.messages) {
      const top = decodeFields(msg);
      // Walk nested messages for doubles / strings (best-effort)
      for (const [, vals] of Object.entries(top)) {
        for (const v of vals) {
          if (Buffer.isBuffer(v) && v.length >= 8) {
            // try as nested message
            try {
              const nested = decodeFields(v);
              for (const [fk, fvals] of Object.entries(nested)) {
                for (const fv of fvals) {
                  if (Buffer.isBuffer(fv) && fv.length === 8) {
                    const pct = fv.readDoubleLE(0);
                    if (pct >= 0 && pct <= 100 && creditUsagePercent === 0) {
                      creditUsagePercent = pct;
                    }
                  }
                }
              }
            } catch {
              /* ignore */
            }
          }
        }
      }
    }

    // Also try REST rate-limits style endpoints
    if (!creditUsagePercent) {
      for (const url of [
        "https://grok.com/rest/rate-limits",
        "https://grok.com/rest/subscriptions",
      ]) {
        try {
          const r = await ses.fetch(url, {
            headers: {
              accept: "application/json",
              cookie: cookieHeader,
              "user-agent": CHROME_UA,
              referer: "https://grok.com/",
            },
          });
          if (!r.ok) continue;
          const j = await r.json();
          const pct =
            Number(j?.creditUsagePercent ?? j?.usagePercent ?? j?.percentUsed ?? 0) || 0;
          if (pct > 0) creditUsagePercent = pct;
          if (j?.plan || j?.planName) planLabel = String(j.plan || j.planName);
          break;
        } catch {
          /* next */
        }
      }
    }

    return {
      ok: true,
      planLabel,
      planId: "super",
      creditUsagePercent,
      periodType: "weekly",
      periodStart,
      periodEnd,
      productUsage,
      prepaidBalanceCents,
      onDemandCapCents: 0,
      onDemandUsedCents: 0,
      ssoCookie: cookieHeader,
    };
  } catch (e) {
    return emptyUsage(e instanceof Error ? e.message : "usage fetch failed");
  }
}

async function getStoredSso() {
  const { sso, cookieHeader, signedIn } = await collectSessionCookies();
  return {
    cookie: sso || cookieHeader || "",
    signedIn: Boolean(signedIn || sso),
  };
}

// ---- connectors fetch (kept from previous version, simplified import) ----
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
  injectCookieHeader,
};
