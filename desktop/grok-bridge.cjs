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
const APP_VERSION = "0.2.16";

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
  const p = String(prompt || "");
  const lower = p.toLowerCase();
  const words = lower.split(/\s+/).filter(Boolean).length;
  if (/\b(imagine|image|picture|draw|render|illustration)\b/i.test(p)) return "fast";
  if (/\b(team of|multi-agent|heavy|red team)\b/i.test(p) || (words > 80 && /debug|architect|debug/i.test(p))) return "heavy";
  if (/\b(code|implement|refactor|typescript|react|scaffold|pkgbuild|full app|rewrite)\b/i.test(p) && words > 20) return "build";
  if (/\b(architect|root cause|trade-?off|research|prove|deep dive|complex)\b/i.test(p) || words > 60 || p.length > 400) return "expert";
  if (words > 28 || /\b(plan|explain|how do i|step by step)\b/i.test(p)) return "expert";
  return "fast";
}

/** Models that often work without SuperGrok / paid API tiers (ordered preference). */
const FREE_FALLBACK_MODELS = [
  "grok-3-mini-fast",
  "grok-3-mini",
  "grok-3",
  "grok-2-latest",
  "grok-2",
  "grok-beta",
  "grok-4-1-fast-non-reasoning",
  "grok-4-fast",
];

function isSubscriptionError(status, msg) {
  const m = String(msg || "").toLowerCase();
  if (status === 402 || status === 403) return true;
  return /subscription|super\s*grok|premium|upgrade|not (entitled|authorized)|permission|quota|billing|payment|insufficient|plan required|access denied|does not have access/i.test(
    m,
  );
}

function modelForMode(mode, prompt = "", opts = {}) {
  const free = Boolean(opts.freeTier);
  const id = free ? (resolveMode(mode, prompt) === "build" ? "fast" : resolveMode(mode, prompt) === "heavy" ? "expert" : resolveMode(mode, prompt)) : resolveMode(mode, prompt);
  // Free tier: never route to 4.5 / heavy / build-only models first
  if (free) {
    if (id === "build") return "grok-3-mini-fast";
    if (id === "expert" || id === "heavy") return "grok-3-mini";
    return "grok-3-mini-fast";
  }
  const p = String(prompt || "");
  switch (id) {
    case "fast":
      return "grok-4-1-fast-non-reasoning";
    case "expert":
      return (p.length > 400 || /\b(architect|research|prove|complex|deep dive)\b/i.test(p))
        ? "grok-4.5"
        : "grok-4.3";
    case "heavy":
      return "grok-4.5";
    case "build":
      return "grok-code-fast-1";
    default:
      return "grok-4-1-fast-non-reasoning";
  }
}

function systemPrompt(mode, prompt = "") {
  const base = `You are Grok, running inside GrokHub (a desktop agent control plane on the user's Linux machine).
Help with coding, ops, research, and local machine tasks.
Be direct and practical. Prefer short structured answers with bullets when listing steps.
Do not prefix replies with mode labels like [Fast] or [Auto → …]. Just answer.

When you need real filesystem / shell data, output:
HOST_CMD: <shell command>
On Linux the host runs bash; on Windows it runs PowerShell. Prefer portable commands when possible (or platform-appropriate paths).
Do not invent file listings — wait for HOST_RESULT.`;
  const id = resolveMode(mode, prompt);
  if (id === "fast") return `${base}\nMode: Fast — concise answers, minimal preamble.`;
  if (id === "expert") return `${base}\nMode: Expert — reason carefully, surface tradeoffs.`;
  if (id === "heavy") return `${base}\nMode: Heavy (team of experts) — multi-angle synthesis.`;
  if (id === "build") return `${base}\nMode: Build — prioritize working code and file paths.`;
  return base;
}

async function callXaiChatOnce(apiKey, model, messages, temperature, max_tokens) {
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
  const msg =
    typeof data.error === "string"
      ? data.error
      : data.error?.message || (res.ok ? "" : `xAI error ${res.status}`);
  if (!res.ok) {
    return { ok: false, status: res.status, error: msg, model };
  }
  const content = data.choices?.[0]?.message?.content?.trim();
  if (!content) return { ok: false, error: "Empty response from Grok", model, status: res.status };
  return {
    ok: true,
    content,
    model: data.model || model,
    usage: data.usage,
    status: res.status,
  };
}

async function callXaiChat(req = {}) {
  const apiKey =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.apiKey && String(req.apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  const freeTier = Boolean(req.freeTier);
  const mode = req.mode || "auto";
  const lastUser = [...(req.messages || [])]
    .reverse()
    .find((m) => m.role === "user")?.content;
  const routed = freeTier
    ? (() => {
        const r = resolveMode(mode, lastUser || "");
        if (r === "heavy" || r === "build") return r === "build" ? "fast" : "expert";
        return r;
      })()
    : resolveMode(mode, lastUser || "");
  const primaryModel =
    req.model || modelForMode(mode, lastUser || "", { freeTier });
  const sys =
    systemPrompt(mode, lastUser || "") +
    (freeTier
      ? "\n\nNote: user is on Free Grok fallback — keep answers concise; heavy/build features may be limited."
      : "") +
    (req.workspaceContext && String(req.workspaceContext).trim()
      ? `\n\n## Imported OpenClaw workspace context\n${String(req.workspaceContext).trim().slice(0, 24000)}`
      : "");
  const messages = [
    { role: "system", content: sys },
    ...(req.messages || []).filter((m) => m.role !== "system"),
  ];
  const temperature =
    routed === "fast" ? 0.5 : routed === "build" ? 0.4 : routed === "heavy" ? 0.8 : 0.7;
  const max_tokens = freeTier
    ? 1536
    : routed === "heavy"
      ? 4096
      : routed === "build"
        ? 8192
        : routed === "expert"
          ? 3072
          : 2048;

  // No API/OAuth token → try website free session if cookie provided
  if (!apiKey) {
    if (req.ssoCookie || req.allowWebsiteFallback !== false) {
      try {
        const websiteSession = require("./website-session.cjs");
        if (typeof websiteSession.chatWithWebsiteSession === "function") {
          const wr = await websiteSession.chatWithWebsiteSession({
            ssoCookie: req.ssoCookie,
            messages: req.messages || [],
            prompt: lastUser || "",
          });
          if (wr?.ok) {
            return {
              ...wr,
              freeTier: true,
              accessPath: "website_free",
              detail: wr.detail || "Free Grok via website session",
            };
          }
          if (!apiKey) {
            return {
              ok: false,
              status: 401,
              error:
                wr?.error ||
                "Not connected. Sign in with free Grok on the website (Link Grok website), use Grok OAuth, or an xAI API key.",
              accessPath: "none",
            };
          }
        }
      } catch (e) {
        if (!apiKey) {
          return {
            ok: false,
            status: 401,
            error:
              e instanceof Error
                ? e.message
                : "Not connected to Grok. Link free website session, OAuth, or API key.",
          };
        }
      }
    }
    return {
      ok: false,
      status: 401,
      error:
        "Not connected to Grok. Link free website session, Grok OAuth, or an xAI API key.",
    };
  }

  const tried = new Set();
  const queue = [primaryModel];
  // Always have free models ready as fallback
  for (const m of FREE_FALLBACK_MODELS) {
    if (!queue.includes(m)) queue.push(m);
  }
  // Alias retries
  if (!queue.includes("grok-4")) queue.push("grok-4");

  let lastErr = null;
  let usedFreeFallback = freeTier;
  try {
    for (const model of queue) {
      if (!model || tried.has(model)) continue;
      tried.add(model);
      const r = await callXaiChatOnce(apiKey, model, messages, temperature, max_tokens);
      if (r.ok) {
        const isFreeModel = FREE_FALLBACK_MODELS.includes(model) || freeTier;
        return {
          ...r,
          freeTier: isFreeModel || usedFreeFallback,
          accessPath: isFreeModel ? "api_free" : "api",
          fallbackFrom: model !== primaryModel ? primaryModel : undefined,
        };
      }
      lastErr = r;
      const msg = r.error || "";
      // Subscription / entitlement → keep trying free models
      if (isSubscriptionError(r.status, msg) || r.status === 404 || /model|not found|invalid/i.test(msg)) {
        usedFreeFallback = true;
        continue;
      }
      // Hard auth failure — don't spin models
      if (r.status === 401) break;
      // Other errors: still try free cascade once
      if (!freeTier && tried.size < 3) {
        usedFreeFallback = true;
        continue;
      }
      break;
    }

    // API exhausted → website free session
    if (req.ssoCookie && req.allowWebsiteFallback !== false) {
      try {
        const websiteSession = require("./website-session.cjs");
        if (typeof websiteSession.chatWithWebsiteSession === "function") {
          const wr = await websiteSession.chatWithWebsiteSession({
            ssoCookie: req.ssoCookie,
            messages: req.messages || [],
            prompt: lastUser || "",
          });
          if (wr?.ok) {
            return {
              ...wr,
              freeTier: true,
              accessPath: "website_free",
              detail: "Fell back to free Grok website session",
            };
          }
        }
      } catch {
        /* ignore */
      }
    }

    return {
      ok: false,
      status: lastErr?.status,
      error:
        lastErr?.error ||
        "Grok request failed. Free-tier models and website fallback unavailable — link website session or upgrade.",
      model: lastErr?.model,
      freeTier: usedFreeFallback,
    };
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

async function isInstallRoot(root) {
  if (!root) return false;
  for (const rel of [
    path.join(".output", "server", "index.mjs"),
    "package.json",
    "VERSION",
    path.join("desktop", "main.mjs"),
  ]) {
    try {
      await fs.stat(path.join(root, rel));
      return true;
    } catch {
      /* try next marker */
    }
  }
  return false;
}

async function findInstallRoot() {
  for (const root of installRoots()) {
    if (await isInstallRoot(root)) return root;
  }
  return null;
}

async function readLocalVersion(root) {
  let version = APP_VERSION;
  let sha = null;
  if (!root) return { version, sha };
  try {
    const av = (await fs.readFile(path.join(root, "APP_VERSION"), "utf8")).trim();
    if (av) version = av;
  } catch {}
  try {
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
    if (pkg.version) version = String(pkg.version);
  } catch {}
  try {
    const v = (await fs.readFile(path.join(root, "VERSION"), "utf8")).trim();
    if (v) sha = v.split(/\s+/)[0];
  } catch {}
  if (!sha) {
    try {
      const { stdout } = await execAsync("git rev-parse HEAD", { cwd: root, timeout: 8000 });
      sha = stdout.trim() || null;
    } catch {}
  }
  return { version, sha };
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
  const local = await readLocalVersion(installRoot);
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
      remoteSha = data.sha || null;
      remoteMessage = (data.commit?.message || "").split("\n")[0] || null;
      if (shaMatch(local.sha, remoteSha)) {
        detail = `Up to date · v${local.version} · ${(local.sha || "").slice(0, 12)}`;
      } else if (!local.sha) {
        detail = "Local VERSION missing — install recommended.";
      } else {
        detail = `Update available · ${local.sha.slice(0, 12)} → ${String(remoteSha).slice(0, 12)}`;
      }
    } else {
      detail = token
        ? "Could not read remote commit (check repo access)."
        : "Could not read remote commit (private repo may need a GitHub token).";
    }
  } catch (e) {
    detail = e instanceof Error ? e.message : "Update check failed";
  }
  const remoteShort = remoteSha ? String(remoteSha).slice(0, 12) : null;
  const localShort = local.sha ? String(local.sha).slice(0, 12) : null;
  return {
    currentVersion: local.version,
    currentSha: localShort,
    remoteSha: remoteShort,
    remoteMessage,
    updateAvailable: Boolean(remoteSha && !shaMatch(local.sha, remoteSha)),
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
  steps.push("User data / memory is outside the install tree and is not modified by updates");

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
    const factoryExtras = opts.factory
      ? ["src", "scripts", "packaging", "public", "package.json", "package-lock.json", "vite.config.ts", "tsconfig.json", "startup.sh", ".grok"]
      : ["package.json", "scripts", "packaging"];
    for (const extra of factoryExtras) {
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
const XAI_UA = "GrokHub/0.2.10 (xAI OAuth; Electron)";

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
    freeTier: req.freeTier,
    ssoCookie: req.ssoCookie,
    allowWebsiteFallback: req.allowWebsiteFallback !== false,
  });
  return {
    ...r,
    ...(tokensOut ? { tokens: tokensOut } : {}),
    refreshed,
  };
}

// patch callXaiChat to accept accessToken


async function callXaiImagine(req = {}) {
  const apiKey =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.apiKey && String(req.apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  if (!apiKey) {
    return {
      ok: false,
      error: "Not connected — Grok OAuth or API key required for live Imagine",
    };
  }
  const prompt = String(req.prompt || "").trim();
  if (!prompt) return { ok: false, error: "empty prompt" };
  const mediaKind = req.mediaKind === "video" ? "video" : "image";
  const quality = req.quality === "quality" ? "quality" : "speed";
  const aspect = req.aspect || "auto";
  const qHint =
    quality === "quality"
      ? mediaKind === "video"
        ? "cinematic motion, high detail, smooth camera"
        : "ultra detailed, sharp focus, professional lighting"
      : mediaKind === "video"
        ? "fast motion sketch"
        : "clean composition, efficient render";
  const fullPrompt = `${prompt}\n\n[${qHint}]`;
  const sizeMap = {
    "16:9": "1792x1024",
    "9:16": "1024x1792",
    "3:2": "1536x1024",
    "2:3": "1024x1536",
    "4:3": "1536x1152",
    "1:1": "1024x1024",
  };
  const size = sizeMap[aspect];

  if (mediaKind === "video") {
    const videoModels = [req.model, "grok-imagine-video", "grok-imagine-video-1.5"].filter(Boolean);
    let lastErr = "video generation unavailable";
    for (const model of videoModels) {
      for (const url of [`${XAI_BASE}/videos/generations`, `${XAI_BASE}/images/generations`]) {
        try {
          const body = { model, prompt: fullPrompt, n: 1 };
          if (size) body.size = size;
          if (aspect && aspect !== "auto") body.aspect_ratio = aspect;
          if (req.referenceDataUrl) body.image = req.referenceDataUrl;
          const res = await fetch(url, {
            method: "POST",
            headers: {
              "content-type": "application/json",
              authorization: `Bearer ${apiKey}`,
            },
            body: JSON.stringify(body),
          });
          const data = await res.json().catch(() => ({}));
          if (!res.ok) {
            lastErr = data.error?.message || data.error || `xAI ${res.status}`;
            continue;
          }
          const row = data.data?.[0] || {};
          const vid = row.video_url || row.video || row.url || "";
          if (vid) {
            return { ok: true, videoDataUrl: vid, model: data.model || model, source: "xai", mediaKind: "video" };
          }
          const b64 = row.b64_json || row.b64 || "";
          if (b64) {
            return {
              ok: true,
              imageDataUrl: `data:image/png;base64,${b64}`,
              model: data.model || model,
              source: "xai",
              mediaKind: "image",
              error: "API returned image for video request",
            };
          }
        } catch (e) {
          lastErr = e instanceof Error ? e.message : "network error";
        }
      }
    }
    return { ok: false, error: lastErr + " — try Image mode or Grok website video", mediaKind: "video" };
  }

  const models = [
    req.model,
    quality === "quality" ? "grok-imagine-image" : "grok-2-image",
    "grok-2-image",
    "grok-2-image-1212",
    "grok-imagine-image",
  ].filter(Boolean);
  let lastErr = "image generation failed";
  for (const model of models) {
    try {
      const body = {
        model,
        prompt: fullPrompt,
        n: Math.min(4, Math.max(1, Number(req.n) || 1)),
        response_format: "b64_json",
      };
      if (size) body.size = size;
      if (aspect && aspect !== "auto") body.aspect_ratio = aspect;
      if (req.referenceDataUrl) body.image = req.referenceDataUrl;
      const res = await fetch(`${XAI_BASE}/images/generations`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          authorization: `Bearer ${apiKey}`,
        },
        body: JSON.stringify(body),
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        lastErr =
          typeof data.error === "string"
            ? data.error
            : data.error?.message || `xAI image ${res.status} (${model})`;
        continue;
      }
      const b64 =
        data.data?.[0]?.b64_json ||
        data.data?.[0]?.b64 ||
        data.data?.[0]?.image ||
        "";
      const url = data.data?.[0]?.url || "";
      if (b64) {
        return {
          ok: true,
          imageDataUrl: `data:image/png;base64,${b64}`,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      if (url) {
        return { ok: true, imageDataUrl: url, model: data.model || model, source: "xai", mediaKind: "image" };
      }
      lastErr = "empty image response";
    } catch (e) {
      lastErr = e instanceof Error ? e.message : "network error";
    }
  }
  return { ok: false, error: lastErr, mediaKind: "image" };
}


async function callXaiChatStream(req = {}, handlers = {}) {
  // Prefer live stream when we have a token; on free-tier / errors fall back to
  // callXaiChat (which already cascades free models + website free session).
  const onDelta = handlers.onDelta || (() => {});
  const onStatus = handlers.onStatus || (() => {});
  let accessToken =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.tokens && req.tokens.accessToken && String(req.tokens.accessToken).trim()) ||
    "";
  let tokensOut = req.tokens || null;
  if (req.tokens && req.tokens.accessToken) {
    try {
      const ensured = await oauthEnsure(req.tokens);
      accessToken = ensured.accessToken || ensured.tokens?.accessToken || accessToken;
      tokensOut = ensured.tokens || tokensOut;
    } catch {}
  }
  const apiKey =
    accessToken ||
    (req.apiKey && String(req.apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  const freeTier = Boolean(req.freeTier);
  const mode = req.mode || "auto";
  const lastUser =
    [...(req.messages || [])].reverse().find((m) => m.role === "user")?.content || "";
  const model = req.model || modelForMode(mode, lastUser, { freeTier });
  const sys =
    systemPrompt(mode, lastUser) +
    (req.workspaceContext && String(req.workspaceContext).trim()
      ? `\n\n## Imported OpenClaw workspace context\n${String(req.workspaceContext).trim().slice(0, 24000)}`
      : "");
  const messages = [
    { role: "system", content: sys },
    ...(req.messages || []).filter((m) => m.role !== "system"),
  ];
  const temperature = 0.6;
  const max_tokens = freeTier ? 1536 : 3072;
  const signal = handlers.signal;

  async function nonStreamFallback(reason) {
    onStatus("fallback");
    const r = await callXaiChatWithOAuth({
      ...req,
      accessToken: accessToken || undefined,
      apiKey: req.apiKey,
      freeTier,
      ssoCookie: req.ssoCookie,
      model: freeTier ? model : req.model,
    });
    if (r.ok && r.content) {
      onDelta(r.content);
    }
    return {
      ...r,
      ...(tokensOut ? { tokens: tokensOut } : {}),
      streamed: false,
      fallbackReason: reason,
    };
  }

  if (!apiKey) {
    return nonStreamFallback("no-api-key");
  }

  try {
    onStatus("connecting");
    const res = await fetch(`${XAI_BASE}/chat/completions`, {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: `Bearer ${apiKey}`,
        accept: "text/event-stream",
      },
      body: JSON.stringify({
        model,
        messages,
        temperature,
        max_tokens,
        stream: true,
      }),
      signal,
    });
    if (!res.ok) {
      const text = await res.text().catch(() => "");
      if (
        isSubscriptionError(res.status, text) ||
        res.status === 404 ||
        freeTier ||
        res.status === 401 ||
        res.status === 403
      ) {
        return nonStreamFallback(`stream ${res.status}`);
      }
      return {
        ok: false,
        status: res.status,
        error: text.slice(0, 240) || `xAI stream ${res.status}`,
        ...(tokensOut ? { tokens: tokensOut } : {}),
      };
    }
    onStatus("streaming");
    const reader = res.body?.getReader?.();
    if (!reader) {
      return nonStreamFallback("no-reader");
    }
    const decoder = new TextDecoder();
    let buffer = "";
    let content = "";
    let usage = undefined;
    while (true) {
      if (signal?.aborted) {
        try {
          await reader.cancel();
        } catch {}
        return {
          ok: false,
          aborted: true,
          error: "Stopped",
          content,
          model,
          ...(tokensOut ? { tokens: tokensOut } : {}),
        };
      }
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split("\n");
      buffer = parts.pop() || "";
      for (const raw of parts) {
        const line = raw.trim();
        if (!line || line.startsWith(":")) continue;
        let payload = line.startsWith("data:") ? line.slice(5).trim() : line;
        if (payload === "[DONE]") continue;
        try {
          const evt = JSON.parse(payload);
          const delta = evt.choices?.[0]?.delta?.content || evt.choices?.[0]?.message?.content || "";
          if (delta) {
            content += delta;
            onDelta(delta);
          }
          if (evt.usage) usage = evt.usage;
        } catch {
          /* ignore */
        }
      }
    }
    if (!content.trim()) {
      return nonStreamFallback("empty-stream");
    }
    return {
      ok: true,
      content,
      model,
      usage,
      freeTier,
      streamed: true,
      ...(tokensOut ? { tokens: tokensOut } : {}),
    };
  } catch (e) {
    if (signal?.aborted) {
      return { ok: false, aborted: true, error: "Stopped", ...(tokensOut ? { tokens: tokensOut } : {}) };
    }
    return nonStreamFallback(e instanceof Error ? e.message : "stream error");
  }
}


module.exports = {
  callXaiChat: callXaiChatWithOAuth,
  callXaiChatStream,
  callXaiImagine,
  probeXaiKey,
  checkForUpdate,
  applyUpdate,
  factoryReinstall,
  scheduleAppRestart,
  oauthStart,
  oauthPoll,
  oauthEnsure,
};
