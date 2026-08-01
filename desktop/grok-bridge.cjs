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
const APP_VERSION = "0.1";

function modelForMode(mode, prompt = "") {
  let id = mode || "auto";
  if (id === "auto") {
    const p = String(prompt).toLowerCase();
    const heavy =
      p.includes("architect") ||
      p.includes("debug") ||
      p.includes("why") ||
      p.includes("research") ||
      p.includes("plan") ||
      p.length > 160;
    id = heavy ? "expert" : "fast";
  }
  switch (id) {
    case "fast":
      return "grok-4-1-fast-non-reasoning";
    case "expert":
    case "heavy":
      return "grok-4";
    case "build":
      return "grok-code-fast-1";
    default:
      return "grok-4-1-fast-non-reasoning";
  }
}

function systemPrompt(mode) {
  const base = `You are GrokHub, a desktop agent control plane powered by Grok (xAI).
Be direct and practical. Prefer short structured answers.`;
  if (mode === "fast") return `${base}\nMode: Fast — concise.`;
  if (mode === "expert") return `${base}\nMode: Expert — careful reasoning.`;
  if (mode === "heavy") return `${base}\nMode: Heavy — multi-angle synthesis.`;
  if (mode === "build") return `${base}\nMode: Build — prioritize working code.`;
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
  const model = req.model || modelForMode(mode, lastUser || "");
  const messages = [
    { role: "system", content: systemPrompt(mode) },
    ...(req.messages || []).filter((m) => m.role !== "system"),
  ];
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
        temperature: mode === "fast" ? 0.5 : 0.7,
        max_tokens: mode === "heavy" ? 4096 : mode === "build" ? 8192 : 2048,
        stream: false,
      }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
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
      if (pkg.version) version = String(pkg.version).replace(/\.0$/, "") || version;
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
    updateAvailable: Boolean(remoteSha && remoteSha !== sha),
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
  const root = (await findInstallRoot()) || process.cwd();
  steps.push(`Install root: ${root}`);
  try {
    let hasGit = false;
    try {
      await fs.stat(path.join(root, ".git"));
      hasGit = true;
    } catch {
      hasGit = false;
    }
    if (hasGit) {
      steps.push(`git fetch + reset --hard origin/${branch}`);
      await execAsync(`git -C "${root}" fetch origin ${branch}`, {
        timeout: 180000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
      await execAsync(`git -C "${root}" reset --hard origin/${branch}`, {
        timeout: 60000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
    } else {
      steps.push("No .git — cloning fresh");
      const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
      const cloneUrl = token
        ? `https://x-access-token:${token}@github.com/${repo}.git`
        : `https://github.com/${repo}.git`;
      await execAsync(`git clone --depth 1 --branch ${branch} "${cloneUrl}" "${tmp}/src"`, {
        timeout: 300000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
      await execAsync(
        `rsync -a --delete --exclude node_modules --exclude .env --exclude .env.local "${tmp}/src/" "${root}/"`,
        { timeout: 120000 },
      );
      await fs.rm(tmp, { recursive: true, force: true });
    }
    steps.push("npm install + desktop build");
    await execAsync("npm ci --ignore-scripts || npm install --ignore-scripts", {
      cwd: root,
      timeout: 600000,
    });
    await execAsync("GROKHUB_DESKTOP=1 npm run build", {
      cwd: root,
      timeout: 600000,
      env: { ...process.env, GROKHUB_DESKTOP: "1" },
    });
    try {
      await fs.access("/usr/bin", fs.constants.W_OK);
      steps.push("Reinstalling system package");
      await execAsync(`bash "${root}/scripts/install-arch.sh"`, { cwd: root, timeout: 120000 });
    } catch {
      steps.push("Skipped system reinstall (no root) — restart from this tree");
    }
    let newSha;
    try {
      const { stdout } = await execAsync("git rev-parse HEAD", { cwd: root });
      newSha = stdout.trim().slice(0, 12);
      await fs.writeFile(path.join(root, "VERSION"), stdout.trim() + "\n");
    } catch {
      /* ignore */
    }
    steps.push("Done — restart GrokHub");
    return { ok: true, detail: `Updated to ${newSha || "latest"}`, steps, newSha };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg, steps };
  }
}


// ── xAI Grok OAuth device-code (OpenClaw / Grok CLI public client) ──
const XAI_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_OAUTH_SCOPE = "openid profile email offline_access grok-cli:access api:access";
const XAI_OAUTH_DISCOVERY = "https://auth.x.ai/.well-known/openid-configuration";
const XAI_DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";
const XAI_UA = "GrokHub/0.1 (xAI OAuth; Electron)";

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
    try {
      const ui = await fetch(d.userinfo_endpoint || "https://auth.x.ai/oauth2/userinfo", {
        headers: { authorization: `Bearer ${j.access_token}`, "user-agent": XAI_UA },
      });
      if (ui.ok) {
        const u = await ui.json();
        email = u.email;
        name = u.name;
        picture = u.picture;
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
  let access = tokens.accessToken;
  let next = { ...tokens };
  const skew = 60000;
  if (tokens.expiresAt && tokens.expiresAt - skew < Date.now() && tokens.refreshToken) {
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
    const j = await res.json().catch(() => ({}));
    if (!res.ok) throw new Error(j.error_description || j.error || "refresh failed");
    next = {
      ...tokens,
      accessToken: j.access_token,
      refreshToken: j.refresh_token || tokens.refreshToken,
      expiresAt: j.expires_in ? Date.now() + j.expires_in * 1000 : tokens.expiresAt,
    };
    access = next.accessToken;
  }
  const probe = await probeXaiKey(access);
  return { ok: probe.ok, detail: probe.detail, refreshed: next !== tokens, tokens: next };
}

async function callXaiChatWithOAuth(req = {}) {
  if (req.tokens?.accessToken) {
    try {
      const ensured = await oauthEnsure(req.tokens);
      const r = await callXaiChat({
        ...req,
        accessToken: ensured.accessToken,
        apiKey: req.apiKey,
      });
      return { ...r, tokens: ensured.tokens, refreshed: ensured.refreshed };
    } catch (e) {
      return { ok: false, error: e instanceof Error ? e.message : "oauth failed" };
    }
  }
  return callXaiChat(req);
}

// patch callXaiChat to accept accessToken

module.exports = {
  callXaiChat: callXaiChatWithOAuth,
  probeXaiKey,
  checkForUpdate,
  applyUpdate,
  oauthStart,
  oauthPoll,
  oauthEnsure,
};
