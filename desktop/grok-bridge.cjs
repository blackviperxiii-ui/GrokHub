/**
 * CommonJS Grok + update bridges for Electron main (no TS loader needed).
 */
const { exec: execCb } = require("node:child_process");
const { promisify } = require("node:util");
const fs = require("node:fs/promises");
const path = require("node:path");
const os = require("node:os");

const execAsync = promisify(execCb);
const XAI_BASE = "https://api.x.ai/v1";
const DEFAULT_REPO = "blackviperxiii-ui/Grok-Hub";
const DEFAULT_BRANCH = "main";
const APP_VERSION = "0.2.3";

function shaMatch(a, b) {
  if (!a || !b) return false;
  const x = String(a).trim().toLowerCase();
  const y = String(b).trim().toLowerCase();
  if (!x || !y) return false;
  const n = Math.min(x.length, y.length);
  if (n < 7) return x === y;
  return x.slice(0, n) === y.slice(0, n);
}

function scheduleAppRestart(appRoot) {
  const { spawn } = require("node:child_process");
  const port = process.env.GROKHUB_PORT || "18765";
  const root = appRoot || process.env.GROKHUB_HOME || process.cwd();
  const runtime = process.env.XDG_RUNTIME_DIR || "/tmp";
  const pidfile = `${runtime}/grokhub/ui.pid`;
  const script = `
set +e
sleep 1.2
if [ -f "${pidfile}" ]; then
  kill "$(cat "${pidfile}")" 2>/dev/null || true
  rm -f "${pidfile}"
fi
fuser -k ${port}/tcp >/dev/null 2>&1 || true
if command -v grokhub >/dev/null 2>&1; then
  nohup grokhub >/dev/null 2>&1 &
  exit 0
fi
export GROKHUB_HOME="${root}"
export GROKHUB_PORT="${port}"
if [ -x "${root}/packaging/aur/grokhub.sh" ]; then
  nohup bash "${root}/packaging/aur/grokhub.sh" >/dev/null 2>&1 &
elif [ -f "${root}/desktop/main.mjs" ] && command -v electron >/dev/null 2>&1; then
  if [ -f "${root}/.output/server/index.mjs" ]; then
    ( cd "${root}" && export PORT="${port}" NITRO_PORT="${port}" HOST=127.0.0.1 NITRO_HOST=127.0.0.1 && nohup node .output/server/index.mjs >/tmp/grokhub-ui-restart.log 2>&1 & echo $! > "${pidfile}" )
    sleep 0.8
  fi
  nohup electron --class=GrokHub --name=GrokHub "${root}/desktop/main.mjs" >/dev/null 2>&1 &
fi
`.trim();
  const child = spawn("bash", ["-c", script], { detached: true, stdio: "ignore", env: process.env });
  child.unref();
}


function resolveMode(mode, prompt = "") {
  let id = mode || "auto";
  if (id !== "auto") return id;
  const p = String(prompt).toLowerCase();
  const heavy =
    p.includes("architect") ||
    p.includes("debug") ||
    p.includes("why") ||
    p.includes("compare") ||
    p.includes("research") ||
    p.includes("plan") ||
    p.includes("implement") ||
    p.includes("refactor") ||
    p.includes("design") ||
    p.length > 160 ||
    p.split(/\s+/).length > 28;
  return heavy ? "expert" : "fast";
}

function modelForMode(mode, prompt = "") {
  const id = resolveMode(mode, prompt);
  switch (id) {
    case "fast":
      return "grok-4-1-fast-non-reasoning";
    case "expert":
    case "heavy":
      return "grok-4.3";
    case "build":
      return "grok-code-fast-1";
    default:
      return "grok-4-1-fast-non-reasoning";
  }
}

function systemPrompt(mode, prompt = "") {
  const base = `You are Grok, running inside GrokHub (a desktop agent control plane).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.`;
  const id = resolveMode(mode, prompt);
  if (id === "fast") return `${base}\nMode: Fast — concise answers, minimal preamble.`;
  if (id === "expert") return `${base}\nMode: Expert — reason carefully, surface tradeoffs.`;
  if (id === "heavy") return `${base}\nMode: Heavy (team of experts) — multi-angle synthesis.`;
  if (id === "build") return `${base}\nMode: Build — prioritize working code and file paths.`;
  return base;
}

async function callXaiChat(req = {}) {
  const apiKey =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.apiKey && String(req.apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  if (!apiKey) {
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Use Grok OAuth (SuperGrok / X Premium) or an xAI API key.",
    };
  }
  const mode = req.mode || "auto";
  const lastUser = [...(req.messages || [])]
    .reverse()
    .find((m) => m.role === "user")?.content;
  const routed = resolveMode(mode, lastUser || "");
  const model = req.model || modelForMode(mode, lastUser || "");
  const messages = [
    { role: "system", content: systemPrompt(mode, lastUser || "") },
    ...(req.messages || []).filter((m) => m.role !== "system"),
  ];
  const temperature =
    routed === "fast" ? 0.5 : routed === "build" ? 0.4 : routed === "heavy" ? 0.8 : 0.7;
  const max_tokens =
    routed === "heavy" ? 4096 : routed === "build" ? 8192 : routed === "expert" ? 3072 : 2048;
  try {
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify({
        model,
        messages,
        temperature,
        max_tokens,
        stream: false,
      }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
      // fallback aliases if account lacks a model id
      if (res.status === 404 || /model|not found|invalid/i.test(String(data.error?.message || data.error || ""))) {
        if (model === "grok-4.3") {
          return callXaiChat({ ...req, model: "grok-4" });
        }
        if (model === "grok-4-1-fast-non-reasoning") {
          return callXaiChat({ ...req, model: "grok-3-mini-fast" });
        }
      }
      const msg =
        typeof data.error === "string"
          ? data.error
          : data.error?.message || `xAI error ${res.status}`;
      return { ok: false, status: res.status, error: msg, model };
    }
    const content = data.choices?.[0]?.message?.content?.trim();
    if (!content) return { ok: false, error: "Empty response from Grok", model };
    return { ok: true, content, model: data.model || model, usage: data.usage, status: res.status };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "Network error" };
  }
}

async function probeXaiKey(apiKey) {
  const key =
    (apiKey && String(apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  if (!key) return { ok: false, detail: "API key is empty" };
  try {
    const res = await fetch(`${XAI_BASE}/models`, {
      headers: { authorization: `Bearer ${key}` },
    });
    if (res.ok) return { ok: true, detail: "Connected to xAI · models reachable" };
    const text = await res.text();
    return { ok: false, detail: `xAI ${res.status}: ${text.slice(0, 160)}` };
  } catch (e) {
    return { ok: false, detail: e instanceof Error ? e.message : "probe failed" };
  }
}

function installRoots() {
  return [
    process.env.GROKHUB_HOME || "",
    "/usr/lib/grokhub",
    path.join(os.homedir(), ".local/share/grokhub"),
    path.resolve(process.cwd()),
  ].filter(Boolean);
}

async function findInstallRoot() {
  for (const root of installRoots()) {
    try {
      const st = await fs.stat(path.join(root, "package.json"));
      if (st.isFile()) return root;
    } catch {
      /* next */
    }
  }
  return null;
}

async function checkForUpdate(opts = {}) {
  const repo = opts.repo || process.env.GROKHUB_REPO || DEFAULT_REPO;
  const branch = opts.branch || process.env.GROKHUB_BRANCH || DEFAULT_BRANCH;
  const token =
    opts.token ||
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN ||
    process.env.GROKHUB_GITHUB_TOKEN ||
    "";
  const installRoot = await findInstallRoot();
  let version = APP_VERSION;
  let sha = null;
  if (installRoot) {
    try {
      const pkg = JSON.parse(await fs.readFile(path.join(installRoot, "package.json"), "utf8"));
      if (pkg.version) version = String(pkg.version);
    } catch {
      /* ignore */
    }
    try {
      const { stdout } = await execAsync("git rev-parse HEAD", { cwd: installRoot, timeout: 8000 });
      sha = stdout.trim().slice(0, 12);
    } catch {
      /* ignore */
    }
  }
  const headers = {
    accept: "application/vnd.github+json",
    "user-agent": "GrokHub-Updater",
  };
  if (token) headers.authorization = `Bearer ${token}`;
  let remoteSha = null;
  let remoteMessage = null;
  let detail = "";
  try {
    const res = await fetch(
      `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(branch)}`,
      { headers },
    );
    if (res.ok) {
      const data = await res.json();
      remoteSha = (data.sha || "").slice(0, 12);
      remoteMessage = (data.commit?.message || "").split("\n")[0] || null;
      detail =
        sha && remoteSha === sha
          ? "Already on latest commit."
          : "Update available from GitHub.";
    } else {
      detail = token
        ? "Could not read remote commit (check repo access)."
        : "Could not read remote commit (private repo may need a GitHub token).";
    }
  } catch (e) {
    detail = e instanceof Error ? e.message : "Update check failed";
  }
  return {
    currentVersion: version,
    currentSha: sha,
    remoteSha,
    remoteMessage,
    updateAvailable: Boolean(remoteSha && !shaMatch(sha, remoteSha)),
    repo,
    branch,
    installRoot,
    detail,
  };
}

async function applyUpdate(opts = {}) {
  const steps = [];
  const repo = opts.repo || process.env.GROKHUB_REPO || DEFAULT_REPO;
  const branch = opts.branch || process.env.GROKHUB_BRANCH || DEFAULT_BRANCH;
  const token =
    opts.token ||
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN ||
    process.env.GROKHUB_GITHUB_TOKEN ||
    "";

  async function isAppRoot(root) {
    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      return true;
    } catch {}
    try {
      const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
      return pkg.name === "grokhub" || pkg.name === "GrokHub";
    } catch {
      return false;
    }
  }

  let root = process.env.GROKHUB_HOME || path.join(os.homedir(), ".local/share/grokhub");
  for (const c of [
    process.env.GROKHUB_HOME,
    process.cwd(),
    "/usr/lib/grokhub",
    path.join(os.homedir(), ".local/share/grokhub"),
  ].filter(Boolean)) {
    if (await isAppRoot(c)) {
      root = c;
      break;
    }
  }
  if (!(await isAppRoot(root))) {
    await fs.mkdir(root, { recursive: true });
    steps.push(`Created ${root}`);
  }
  steps.push(`Install root: ${root}`);

  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
  const tarball = path.join(tmp, "update.tar.gz");
  const extractDir = path.join(tmp, "extract");

  try {
    steps.push("Downloading GitHub archive…");
    const headers = {
      accept: "application/vnd.github+json",
      "user-agent": "GrokHub-Updater",
    };
    if (token) headers.authorization = `Bearer ${token}`;
    const urls = [
      `https://api.github.com/repos/${repo}/tarball/${branch}`,
      `https://codeload.github.com/${repo}/tar.gz/refs/heads/${branch}`,
    ];
    let ok = false;
    let last = "";
    for (const url of urls) {
      try {
        const res = await fetch(url, {
          headers: url.includes("api.github.com")
            ? headers
            : { "user-agent": "GrokHub-Updater", ...(token ? { authorization: `Bearer ${token}` } : {}) },
          redirect: "follow",
        });
        if (!res.ok) {
          last = `HTTP ${res.status}`;
          continue;
        }
        const buf = Buffer.from(await res.arrayBuffer());
        if (buf.length < 1000) {
          last = "archive too small";
          continue;
        }
        await fs.writeFile(tarball, buf);
        ok = true;
        steps.push(`Downloaded ${(buf.length / 1024 / 1024).toFixed(1)} MB`);
        break;
      } catch (e) {
        last = e instanceof Error ? e.message : String(e);
      }
    }
    if (!ok) throw new Error(`Download failed: ${last}`);

    steps.push("Extracting…");
    await fs.mkdir(extractDir, { recursive: true });
    await execAsync(`tar -xzf "${tarball}" -C "${extractDir}"`, {
      timeout: 120000,
      maxBuffer: 20 * 1024 * 1024,
    });
    const entries = await fs.readdir(extractDir);
    let extracted = extractDir;
    if (entries.length === 1) {
      const only = path.join(extractDir, entries[0]);
      if ((await fs.stat(only)).isDirectory()) extracted = only;
    }

    async function swap(srcName) {
      const src = path.join(extracted, srcName);
      const dest = path.join(root, srcName);
      try {
        await fs.stat(src);
      } catch {
        steps.push(`Skip ${srcName}`);
        return;
      }
      const bak = `${dest}.bak-${Date.now()}`;
      let had = false;
      try {
        await fs.stat(dest);
        had = true;
        await fs.rename(dest, bak);
      } catch {}
      try {
        await fs.mkdir(path.dirname(dest), { recursive: true });
        await execAsync(`cp -a "${src}" "${dest}"`, { timeout: 120000 });
        if (had) await fs.rm(bak, { recursive: true, force: true }).catch(() => {});
        steps.push(`Updated ${srcName}`);
      } catch (e) {
        if (had) {
          await fs.rm(dest, { recursive: true, force: true }).catch(() => {});
          await fs.rename(bak, dest).catch(() => {});
        }
        throw e;
      }
    }

    await swap(".output");
    await swap("desktop");
    for (const extra of ["package.json", "scripts", "packaging"]) {
      try {
        await fs.stat(path.join(extracted, extra));
        await swap(extra);
      } catch {}
    }

    let newSha;
    let newVersion = APP_VERSION;
    try {
      const head = await checkForUpdate({ repo, branch, token });
      newSha = head.remoteSha || undefined;
    } catch {}
    try {
      const headers = { accept: "application/vnd.github+json", "user-agent": "GrokHub-Updater" };
      if (token) headers.authorization = `Bearer ${token}`;
      const res = await fetch(`https://api.github.com/repos/${repo}/commits/${branch}`, { headers });
      if (res.ok) {
        const data = await res.json();
        if (data.sha) newSha = data.sha;
      }
    } catch {}
    if (!newSha) {
      const m = path.basename(extracted).match(/-([0-9a-f]{7,40})$/i);
      if (m) newSha = m[1];
    }
    try {
      const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
      if (pkg.version) newVersion = String(pkg.version);
    } catch {}
    if (newSha) {
      await fs.writeFile(path.join(root, "VERSION"), newSha + "\n");
      steps.push(`VERSION → ${newSha.slice(0, 12)}`);
    }
    await fs.writeFile(path.join(root, "APP_VERSION"), newVersion + "\n");
    steps.push(`APP_VERSION → ${newVersion}`);

    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      steps.push("Verified .output/server/index.mjs");
    } catch {
      throw new Error("Update missing .output/server/index.mjs");
    }

    try {
      if (typeof process.getuid === "function" && process.getuid() === 0) {
        const script = path.join(root, "scripts", "install-arch.sh");
        await fs.stat(script);
        steps.push("Running install-arch.sh");
        await execAsync(`bash "${script}"`, { cwd: root, timeout: 180000 });
      }
    } catch {
      steps.push("System reinstall skipped");
    }

    let status;
    try {
      status = await checkForUpdate({ repo, branch, token });
      if (status.updateAvailable && newSha && shaMatch(newSha, status.remoteSha)) {
        status.updateAvailable = false;
        status.currentSha = newSha.slice(0, 12);
        status.currentVersion = newVersion;
        status.detail = `Up to date · v${newVersion} · ${newSha.slice(0, 12)}`;
      }
    } catch {}

    const doRestart = opts.restart !== false;
    if (doRestart) {
      steps.push("Restarting GrokHub…");
      scheduleAppRestart(root);
    } else {
      steps.push("Done — relaunch GrokHub to load the new build");
    }

    return {
      ok: true,
      detail: `Updated to v${newVersion} (${(newSha || "latest").slice(0, 12)})`,
      steps,
      newSha: newSha ? newSha.slice(0, 12) : undefined,
      newVersion,
      restarting: doRestart,
      status,
    };

  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg.slice(0, 2000), steps };
  } finally {
    await fs.rm(tmp, { recursive: true, force: true }).catch(() => {});
  }
}


// ── xAI Grok OAuth device-code (OpenClaw / Grok CLI public client) ──
const XAI_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_OAUTH_SCOPE = "openid profile email offline_access grok-cli:access api:access";
const XAI_OAUTH_DISCOVERY = "https://auth.x.ai/.well-known/openid-configuration";
const XAI_DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";
const XAI_UA = "GrokHub/0.2.3 (xAI OAuth; Electron)";

async function xaiDiscovery() {
  const res = await fetch(XAI_OAUTH_DISCOVERY, {
    headers: { accept: "application/json", "user-agent": XAI_UA },
  });
  if (!res.ok) throw new Error("xAI discovery failed");
  return res.json();
}

async function oauthStart() {
  const d = await xaiDiscovery();
  const res = await fetch(d.device_authorization_endpoint, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
      "user-agent": XAI_UA,
    },
    body: new URLSearchParams({
      client_id: XAI_OAUTH_CLIENT_ID,
      scope: XAI_OAUTH_SCOPE,
    }).toString(),
  });
  const j = await res.json();
  if (!res.ok) throw new Error(j.error_description || j.error || "device code failed");
  return {
    ok: true,
    deviceCode: j.device_code,
    userCode: j.user_code,
    verificationUri: j.verification_uri,
    verificationUriComplete: j.verification_uri_complete,
    expiresIn: j.expires_in || 1800,
    interval: j.interval || 5,
  };
}

async function oauthPoll(deviceCode) {
  const d = await xaiDiscovery();
  const res = await fetch(d.token_endpoint, {
    method: "POST",
    headers: {
      "content-type": "application/x-www-form-urlencoded",
      accept: "application/json",
      "user-agent": XAI_UA,
    },
    body: new URLSearchParams({
      grant_type: XAI_DEVICE_GRANT,
      client_id: XAI_OAUTH_CLIENT_ID,
      device_code: deviceCode,
    }).toString(),
  });
  const j = await res.json().catch(() => ({}));
  if (res.ok && j.access_token) {
    let email, name, picture;
    // Prefer id_token claims (often include Google/X avatar URL)
    if (j.id_token) {
      try {
        const payload = JSON.parse(
          Buffer.from(String(j.id_token).split(".")[1], "base64url").toString("utf8"),
        );
        email = payload.email || email;
        name = payload.name || payload.preferred_username || payload.given_name || name;
        picture =
          payload.picture ||
          payload.avatar_url ||
          payload.profile_image_url ||
          picture;
      } catch {}
    }
    try {
      const ui = await fetch(d.userinfo_endpoint || "https://auth.x.ai/oauth2/userinfo", {
        headers: { authorization: `Bearer ${j.access_token}`, "user-agent": XAI_UA },
      });
      if (ui.ok) {
        const u = await ui.json();
        email = u.email || email;
        name = u.name || u.preferred_username || name;
        picture = u.picture || u.avatar_url || u.profile_image_url || u.image || picture;
      }
    } catch {}
    return {
      status: "ready",
      tokens: {
        accessToken: j.access_token,
        refreshToken: j.refresh_token,
        expiresAt: j.expires_in ? Date.now() + j.expires_in * 1000 : undefined,
        idToken: j.id_token,
        email,
        name,
        picture,
        connectedAt: Date.now(),
      },
    };
  }
  const err = j.error || "unknown";
  if (err === "authorization_pending") return { status: "pending", error: err };
  if (err === "slow_down") return { status: "slow_down" };
  if (err === "expired_token") return { status: "expired", error: j.error_description || err };
  if (err === "access_denied") return { status: "denied", error: j.error_description || err };
  return { status: "pending", error: j.error_description || err };
}

async function oauthEnsure(tokens) {
  if (!tokens || !tokens.accessToken) {
    throw new Error("No OAuth access token");
  }
  let access = String(tokens.accessToken);
  let next = { ...tokens };
  let refreshed = false;
  const skew = 60_000;
  const expired =
    typeof tokens.expiresAt === "number" && tokens.expiresAt - skew < Date.now();

  if (expired && tokens.refreshToken) {
    const d = await xaiDiscovery();
    const res = await fetch(d.token_endpoint, {
      method: "POST",
      headers: {
        "content-type": "application/x-www-form-urlencoded",
        accept: "application/json",
        "user-agent": XAI_UA,
      },
      body: new URLSearchParams({
        grant_type: "refresh_token",
        client_id: XAI_OAUTH_CLIENT_ID,
        refresh_token: tokens.refreshToken,
      }).toString(),
    });
    const text = await res.text();
    let j = {};
    try {
      j = JSON.parse(text);
    } catch {
      /* ignore */
    }
    if (!res.ok) {
      if (/cloudflare|<!doctype html/i.test(text)) {
        throw new Error("xAI blocked token refresh — reconnect Grok OAuth in Settings");
      }
      throw new Error(j.error_description || j.error || `refresh failed (${res.status})`);
    }
    next = {
      ...tokens,
      accessToken: j.access_token,
      refreshToken: j.refresh_token || tokens.refreshToken,
      expiresAt: j.expires_in ? Date.now() + j.expires_in * 1000 : tokens.expiresAt,
      idToken: j.id_token || tokens.idToken,
    };
    access = next.accessToken;
    refreshed = true;
  }

  const probe = await probeXaiKey(access);
  return {
    ok: probe.ok,
    detail: probe.detail,
    refreshed,
    tokens: next,
    accessToken: access,
  };
}

async function callXaiChatWithOAuth(req = {}) {
  let accessToken =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.tokens && req.tokens.accessToken && String(req.tokens.accessToken).trim()) ||
    "";
  let tokensOut = req.tokens || null;
  let refreshed = false;

  if (req.tokens && req.tokens.accessToken) {
    try {
      const ensured = await oauthEnsure(req.tokens);
      // Critical: use ensured.tokens.accessToken (and alias accessToken)
      accessToken = ensured.accessToken || ensured.tokens?.accessToken || accessToken;
      tokensOut = ensured.tokens || tokensOut;
      refreshed = Boolean(ensured.refreshed);
    } catch (e) {
      // If refresh failed, still try existing access token once
      if (!accessToken) {
        return {
          ok: false,
          error: e instanceof Error ? e.message : "OAuth refresh failed",
        };
      }
    }
  }

  const r = await callXaiChat({
    ...req,
    accessToken: accessToken || undefined,
    apiKey: req.apiKey,
  });
  return {
    ...r,
    ...(tokensOut ? { tokens: tokensOut } : {}),
    refreshed,
  };
}

// patch callXaiChat to accept accessToken

module.exports = {
  callXaiChat: callXaiChatWithOAuth,
  probeXaiKey,
  checkForUpdate,
  applyUpdate,
  scheduleAppRestart,
  oauthStart,
  oauthPoll,
  oauthEnsure,
};
