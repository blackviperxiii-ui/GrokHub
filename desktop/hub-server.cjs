/**
 * Built-in LAN Hub — pair computers, sync chats/memory, send remote tasks.
 * Separate from Grok OAuth. Pairing code required; no secrets in snapshots.
 */
const http = require("node:http");
const os = require("node:os");
const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const HUB_KIND = "grokhub-hub-v1";
const DEFAULT_PORT = 18766;
const PAIR_TTL_MS = 15 * 60 * 1000;
const MAX_BODY = 8 * 1024 * 1024;
const CODE_ALPH = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

function userDataDir() {
  try {
    const { app } = require("electron");
    if (app?.getPath) return app.getPath("userData");
  } catch {
    /* not in electron */
  }
  return path.join(os.homedir(), ".config", "GrokHub");
}

function statePath() {
  return path.join(userDataDir(), "hub-state.json");
}

function uid(prefix) {
  return `${prefix}-${crypto.randomBytes(8).toString("hex")}`;
}

function newToken() {
  return crypto.randomBytes(24).toString("hex");
}

function makePairCode() {
  let s = "";
  const bytes = crypto.randomBytes(6);
  for (let i = 0; i < 6; i++) s += CODE_ALPH[bytes[i] % CODE_ALPH.length];
  return `${s.slice(0, 3)}-${s.slice(3)}`;
}

function normalizeCode(c) {
  return String(c || "")
    .toUpperCase()
    .replace(/[^A-Z0-9]/g, "");
}

function lanAddresses() {
  const out = [];
  const ifs = os.networkInterfaces();
  for (const rows of Object.values(ifs || {})) {
    for (const row of rows || []) {
      if (!row || row.internal || row.family !== "IPv4") continue;
      if (row.address) out.push(row.address);
    }
  }
  return out;
}

function defaultName() {
  try {
    return os.hostname() || "This computer";
  } catch {
    return "This computer";
  }
}

function emptyState() {
  return {
    deviceId: uid("d"),
    deviceName: defaultName(),
    sharing: false,
    port: DEFAULT_PORT,
    pair: null,
    peers: [],
    remotes: [],
    inbox: [],
    snapshot: null,
    lastIncomingAt: 0,
  };
}

function loadState() {
  try {
    const raw = fs.readFileSync(statePath(), "utf8");
    const j = JSON.parse(raw);
    if (!j || typeof j !== "object") return emptyState();
    return {
      ...emptyState(),
      ...j,
      peers: Array.isArray(j.peers) ? j.peers : [],
      remotes: Array.isArray(j.remotes) ? j.remotes : [],
      inbox: Array.isArray(j.inbox) ? j.inbox : [],
    };
  } catch {
    return emptyState();
  }
}

function saveState(st) {
  state = st;
  if (!persistEnabled) return;
  const dir = userDataDir();
  fs.mkdirSync(dir, { recursive: true });
  const tmp = statePath() + ".tmp";
  fs.writeFileSync(tmp, JSON.stringify(st, null, 2));
  fs.renameSync(tmp, statePath());
}

let state = loadState();
let server = null;
let listenPort = 0;
let persistEnabled = true;

function publicStatus() {
  const port = listenPort || state.port || DEFAULT_PORT;
  const addrs = lanAddresses();
  return {
    ok: true,
    kind: HUB_KIND,
    deviceId: state.deviceId,
    deviceName: state.deviceName,
    sharing: Boolean(server && state.sharing),
    port,
    urls: addrs.map((ip) => `http://${ip}:${port}`),
    pairCode: state.pair && state.pair.expiresAt > Date.now() ? state.pair.code : null,
    pairExpiresAt: state.pair && state.pair.expiresAt > Date.now() ? state.pair.expiresAt : null,
    peers: state.peers.map((p) => ({
      id: p.id,
      name: p.name,
      lastSeen: p.lastSeen || 0,
    })),
    remotes: state.remotes.map((r) => ({
      id: r.id,
      name: r.name,
      url: r.url,
      lastSeen: r.lastSeen || 0,
    })),
    inboxCount: state.inbox.filter((t) => t.status === "queued").length,
    lastIncomingAt: state.lastIncomingAt || 0,
  };
}

function rotatePair() {
  state.pair = { code: makePairCode(), expiresAt: Date.now() + PAIR_TTL_MS };
  saveState(state);
  return state.pair;
}

function setDeviceName(name) {
  const n = String(name || "").trim().slice(0, 48);
  if (n) state.deviceName = n;
  saveState(state);
  return publicStatus();
}

function bearer(req) {
  const h = String(req.headers.authorization || "");
  const m = h.match(/^Bearer\s+(\S+)/i);
  return m ? m[1] : "";
}

function peerForToken(token) {
  if (!token) return null;
  return state.peers.find((p) => p.token === token) || null;
}

function cors(res) {
  res.setHeader("access-control-allow-origin", "*");
  res.setHeader("access-control-allow-headers", "authorization, content-type");
  res.setHeader("access-control-allow-methods", "GET,POST,PUT,OPTIONS");
}

function sendJson(res, status, body) {
  cors(res);
  const s = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json; charset=utf-8",
    "content-length": Buffer.byteLength(s),
    "cache-control": "no-store",
  });
  res.end(s);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    let n = 0;
    req.on("data", (c) => {
      n += c.length;
      if (n > MAX_BODY) {
        reject(new Error("payload too large"));
        req.destroy();
        return;
      }
      chunks.push(c);
    });
    req.on("end", () => {
      if (!chunks.length) return resolve({});
      try {
        resolve(JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}"));
      } catch {
        reject(new Error("invalid JSON"));
      }
    });
    req.on("error", reject);
  });
}

function enqueueTask(from, targetDeviceId, title, prompt) {
  const task = {
    id: uid("task"),
    fromId: from.id,
    fromName: from.name,
    targetDeviceId: targetDeviceId || state.deviceId,
    title: String(title || "Remote task").slice(0, 120),
    prompt: String(prompt || "").slice(0, 16_000),
    status: "queued",
    createdAt: Date.now(),
  };
  state.inbox = [task, ...state.inbox].slice(0, 80);
  saveState(state);
  return task;
}

async function handle(req, res) {
  cors(res);
  if (req.method === "OPTIONS") {
    res.writeHead(204);
    res.end();
    return;
  }
  const url = new URL(req.url || "/", "http://127.0.0.1");
  const p = url.pathname.replace(/\/+$/, "") || "/";

  if (req.method === "GET" && (p === "/v1/health" || p === "/health")) {
    sendJson(res, 200, { ok: true, kind: HUB_KIND, name: state.deviceName });
    return;
  }

  if (req.method === "POST" && p === "/v1/pair") {
    let body;
    try {
      body = await readBody(req);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e.message });
      return;
    }
    const code = normalizeCode(body.code);
    const want = state.pair ? normalizeCode(state.pair.code) : "";
    if (!want || !state.pair || state.pair.expiresAt < Date.now()) {
      sendJson(res, 400, { ok: false, error: "No active pairing code — generate one on the host." });
      return;
    }
    if (code !== want) {
      sendJson(res, 403, { ok: false, error: "Pairing code does not match." });
      return;
    }
    const deviceId = String(body.deviceId || "").trim() || uid("d");
    const deviceName = String(body.deviceName || "Computer").trim().slice(0, 48);
    let peer = state.peers.find((x) => x.id === deviceId);
    if (!peer) {
      peer = { id: deviceId, name: deviceName, token: newToken(), lastSeen: Date.now() };
      state.peers.push(peer);
    } else {
      peer.name = deviceName;
      peer.token = newToken();
      peer.lastSeen = Date.now();
    }
    state.pair = null;
    saveState(state);
    sendJson(res, 200, {
      ok: true,
      token: peer.token,
      deviceId: peer.id,
      hub: {
        id: state.deviceId,
        name: state.deviceName,
      },
    });
    return;
  }

  const token = bearer(req);
  const peer = peerForToken(token);
  if (!peer) {
    sendJson(res, 401, { ok: false, error: "Pair this computer first (Settings → Devices)." });
    return;
  }
  peer.lastSeen = Date.now();

  if (req.method === "GET" && p === "/v1/status") {
    saveState(state);
    sendJson(res, 200, {
      ok: true,
      hub: { id: state.deviceId, name: state.deviceName },
      you: { id: peer.id, name: peer.name },
      peers: [
        { id: state.deviceId, name: state.deviceName, role: "hub" },
        ...state.peers.map((x) => ({ id: x.id, name: x.name, role: "peer" })),
      ],
    });
    return;
  }

  if (req.method === "GET" && p === "/v1/snapshot") {
    sendJson(res, 200, { ok: true, snapshot: state.snapshot || null });
    return;
  }

  if (req.method === "PUT" && p === "/v1/snapshot") {
    let body;
    try {
      body = await readBody(req);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e.message });
      return;
    }
    const snap = body.snapshot || body;
    if (!snap || snap.kind !== HUB_KIND) {
      sendJson(res, 400, { ok: false, error: "Not a GrokHub hub snapshot." });
      return;
    }
    state.snapshot = snap;
    state.lastIncomingAt = Date.now();
    saveState(state);
    sendJson(res, 200, { ok: true });
    return;
  }

  if (req.method === "POST" && p === "/v1/task") {
    let body;
    try {
      body = await readBody(req);
    } catch (e) {
      sendJson(res, 400, { ok: false, error: e.message });
      return;
    }
    const prompt = String(body.prompt || "").trim();
    if (!prompt) {
      sendJson(res, 400, { ok: false, error: "Task prompt is empty." });
      return;
    }
    const task = enqueueTask(peer, body.targetDeviceId, body.title, prompt);
    sendJson(res, 200, { ok: true, task: { id: task.id, targetDeviceId: task.targetDeviceId } });
    return;
  }

  if (req.method === "GET" && p === "/v1/inbox") {
    const mine = state.inbox.filter(
      (t) => t.status === "queued" && t.targetDeviceId === peer.id,
    );
    sendJson(res, 200, { ok: true, tasks: mine });
    return;
  }

  const ack = p.match(/^\/v1\/inbox\/([^/]+)\/ack$/);
  if (req.method === "POST" && ack) {
    const id = ack[1];
    const t = state.inbox.find((x) => x.id === id && x.targetDeviceId === peer.id);
    if (t) t.status = "acked";
    saveState(state);
    sendJson(res, 200, { ok: true });
    return;
  }

  sendJson(res, 404, { ok: false, error: "unknown hub route" });
}

function startShare(opts = {}) {
  if (server) return { ...publicStatus(), already: true };
  const port = Number(opts.port || process.env.GROKHUB_HUB_PORT || state.port || DEFAULT_PORT);
  state.port = port;
  state.sharing = true;
  if (!state.pair || state.pair.expiresAt < Date.now()) rotatePair();
  saveState(state);
  return new Promise((resolve, reject) => {
    server = http.createServer((req, res) => {
      handle(req, res).catch((e) => {
        try {
          sendJson(res, 500, { ok: false, error: e instanceof Error ? e.message : String(e) });
        } catch {
          /* ignore */
        }
      });
    });
    server.on("error", (e) => {
      server = null;
      state.sharing = false;
      saveState(state);
      reject(e);
    });
    server.listen(port, "0.0.0.0", () => {
      const addr = server.address();
      listenPort = addr && typeof addr === "object" ? addr.port : port;
      resolve(publicStatus());
    });
  });
}

function stopShare(opts = {}) {
  const persist = opts.persist !== false;
  return new Promise((resolve) => {
    const done = () => {
      server = null;
      listenPort = 0;
      if (persist) state.sharing = false;
      saveState(state);
      resolve(publicStatus());
    };
    if (!server) return done();
    server.close(() => done());
    setTimeout(done, 800);
  });
}

function addRemote(entry) {
  const url = String(entry.url || "").replace(/\/+$/, "");
  if (!url) return publicStatus();
  const existing = state.remotes.find((r) => r.url === url || r.id === entry.id);
  if (existing) {
    existing.id = entry.id || existing.id;
    existing.name = entry.name || existing.name;
    existing.token = entry.token || existing.token;
    existing.lastSeen = Date.now();
  } else {
    state.remotes.push({
      id: entry.id || uid("hub"),
      name: entry.name || "Hub",
      url,
      token: entry.token,
      lastSeen: Date.now(),
    });
  }
  saveState(state);
  return publicStatus();
}

function removeRemote(id) {
  state.remotes = state.remotes.filter((r) => r.id !== id && r.url !== id);
  saveState(state);
  return publicStatus();
}

function forgetPeer(id) {
  state.peers = state.peers.filter((p) => p.id !== id);
  saveState(state);
  return publicStatus();
}

function claimLocalInbox() {
  const mine = state.inbox.filter(
    (t) => t.status === "queued" && t.targetDeviceId === state.deviceId,
  );
  for (const t of mine) t.status = "claimed";
  if (mine.length) saveState(state);
  return mine;
}

function storeSnapshot(snap) {
  if (!snap || snap.kind !== HUB_KIND) return { ok: false, error: "bad snapshot" };
  state.snapshot = snap;
  state.lastIncomingAt = Date.now();
  saveState(state);
  return { ok: true };
}

function getStoredSnapshot() {
  return state.snapshot || null;
}

async function joinHub({ url, code, deviceName }) {
  const base = String(url || "").trim().replace(/\/+$/, "");
  if (!base) return { ok: false, error: "Enter the other computer’s address." };
  const res = await fetch(`${base}/v1/pair`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      code,
      deviceId: state.deviceId,
      deviceName: deviceName || state.deviceName,
    }),
  });
  const data = await res.json().catch(() => ({}));
  if (!res.ok || !data.ok || !data.token) {
    return { ok: false, error: data.error || `Could not pair (${res.status})` };
  }
  addRemote({
    id: data.hub?.id,
    name: data.hub?.name,
    url: base,
    token: data.token,
  });
  return { ok: true, ...publicStatus() };
}

async function hubFetch(remote, method, pathname, body) {
  const headers = {
    authorization: `Bearer ${remote.token}`,
    accept: "application/json",
  };
  const init = { method, headers };
  if (body != null) {
    headers["content-type"] = "application/json";
    init.body = JSON.stringify(body);
  }
  const res = await fetch(`${remote.url}${pathname}`, init);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    const err = new Error(data.error || `hub ${res.status}`);
    err.status = res.status;
    throw err;
  }
  remote.lastSeen = Date.now();
  saveState(state);
  return data;
}

function listRemotes() {
  return state.remotes.slice();
}

function listTargets() {
  const me = { id: state.deviceId, name: `${state.deviceName} (this computer)`, self: true };
  const peers = state.peers.map((p) => ({ id: p.id, name: p.name, self: false }));
  return [me, ...peers];
}

module.exports = {
  HUB_KIND,
  DEFAULT_PORT,
  publicStatus,
  rotatePair,
  setDeviceName,
  startShare,
  stopShare,
  addRemote,
  removeRemote,
  forgetPeer,
  claimLocalInbox,
  storeSnapshot,
  getStoredSnapshot,
  joinHub,
  hubFetch,
  listRemotes,
  listTargets,
  enqueueTask,
  makePairCode,
  normalizeCode,
  lanAddresses,
  /** test helpers */
  _resetForTests: () => {
    persistEnabled = false;
    state = emptyState();
    if (server) {
      try {
        server.close();
      } catch {
        /* ignore */
      }
      server = null;
    }
  },
  _getState: () => state,
};
