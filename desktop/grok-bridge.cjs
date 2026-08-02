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
const APP_VERSION = "0.8.30";
let updateInProgress = false;

function shaMatch(a, b) {
  if (!a || !b) return false;
  const x = String(a).trim().toLowerCase();
  const y = String(b).trim().toLowerCase();
  if (!x || !y) return false;
  const n = Math.min(x.length, y.length);
  if (n < 7) return x === y;
  return x.slice(0, n) === y.slice(0, n);
}

function sleepMs(ms) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * True only if pid looks like our Nitro UI (node … .output/server), not Electron or random apps.
 */
async function isGrokHubUiPid(pid) {
  if (!pid || !Number.isFinite(pid) || pid <= 1) return false;
  try {
    process.kill(pid, 0);
  } catch {
    return false;
  }
  try {
    const raw = await fs.readFile(`/proc/${pid}/cmdline`, "utf8");
    const cmd = raw.replace(/\0/g, " ");
    const isNode = /\bnode\b|ELECTRON_RUN_AS_NODE/i.test(cmd) || /\/node\s/.test(cmd);
    const isUi =
      /\.output\/server|index\.mjs|nitro|grokhub/i.test(cmd) &&
      !/desktop\/main\.mjs/i.test(cmd);
    return isNode && isUi;
  } catch {
    // Non-Linux: trust pidfile only when process is alive
    return process.platform !== "linux";
  }
}

/**
 * Stop the Nitro UI server so we can swap install files without crashing.
 * NEVER uses `fuser -k` (that kills anything on the port — collateral damage).
 */
async function stopUiServer(steps) {
  const port = process.env.GROKHUB_PORT || process.env.PORT || "18765";
  const runtime = process.env.XDG_RUNTIME_DIR || "/tmp";
  const dir = path.join(runtime, "grokhub");
  const pidfile = path.join(dir, "ui.pid");
  const lockfile = path.join(dir, "ui.lock");
  let stopped = false;
  const seen = new Set();
  for (const file of [pidfile, lockfile]) {
    try {
      const raw = await fs.readFile(file, "utf8");
      const pid = Number(String(raw).trim());
      if (!pid || seen.has(pid)) continue;
      seen.add(pid);
      if (!(await isGrokHubUiPid(pid))) {
        steps.push(`Skip pid ${pid} (not GrokHub UI)`);
        continue;
      }
      try {
        process.kill(pid, "SIGTERM");
        steps.push(`Stopped UI pid ${pid}`);
        stopped = true;
        await sleepMs(400);
        try {
          process.kill(pid, 0);
          process.kill(pid, "SIGKILL");
          steps.push(`Force-killed UI pid ${pid}`);
        } catch {
          /* exited */
        }
      } catch {
        /* already dead */
      }
    } catch {
      /* no pid file */
    }
  }
  await fs.unlink(pidfile).catch(() => {});
  await fs.unlink(lockfile).catch(() => {});
  // Wait until port is free (max ~4s) — do not mass-kill the port
  for (let i = 0; i < 20; i++) {
    try {
      await new Promise((resolve, reject) => {
        const net = require("node:net");
        const s = net.createConnection({ host: "127.0.0.1", port: Number(port) }, () => {
          s.destroy();
          reject(new Error("busy"));
        });
        s.on("error", () => resolve());
        s.setTimeout(200, () => {
          s.destroy();
          resolve();
        });
      });
      if (stopped) steps.push(`UI port ${port} free`);
      return;
    } catch {
      await sleepMs(200);
    }
  }
  steps.push(`UI port ${port} may still be busy (left intact — no fuser kill)`);
}

/**
 * Relaunch after a clean exit. Never mutates files here — only starts a fresh process
 * once the old Electron has quit.
 */
function scheduleAppRestart(appRoot) {
  const { spawn } = require("node:child_process");
  const port = process.env.GROKHUB_PORT || "18765";
  const root = appRoot || process.env.GROKHUB_HOME || process.cwd();
  let home = process.env.HOME || process.env.USERPROFILE || "";
  try {
    if (!home) home = os.homedir() || "";
  } catch {
    home = "";
  }
  // Derive home from user install path if env is stripped (pkexec / sanitized spawn)
  if (!home && String(root).includes("/.local/lib/grokhub")) {
    home = String(root).replace(/\/\.local\/lib\/grokhub\/?$/, "");
  }
  if (!home && String(root).includes("/.local/share/grokhub")) {
    home = String(root).replace(/\/\.local\/share\/grokhub\/?$/, "");
  }
  const runtime = process.env.XDG_RUNTIME_DIR || (home ? path.join(home, ".cache") : "/tmp");
  const pidfile = path.join(runtime, "grokhub", "ui.pid");
  const log = path.join(runtime, "grokhub", "restart.log");
  const userBin = home ? path.join(home, ".local", "bin", "grokhub") : "";
  // IMPORTANT: this is a JS template literal — use \${...} for shell vars, ${jsVar} for Node.
  const script = `
set +e
export HOME="${home || "/tmp"}"
export USER="\${USER:-$(id -un 2>/dev/null || echo user)}"
mkdir -p "${runtime}/grokhub"
exec >>"${log}" 2>&1
echo "[restart] $(date -Iseconds) root=${root} HOME=\$HOME"
# Wait for previous Electron to exit and release files
sleep 2.8
# Free incomplete .new only; KEEP .prev for one-shot rollback from Settings
rm -rf "${root}.new" 2>/dev/null || true
# Write stamp so post-update self-test can confirm
echo "ok $(date -Iseconds)" > "${runtime}/grokhub/last-restart.ok" 2>/dev/null || true
if [ -f "${pidfile}" ]; then
  kill "$(cat "${pidfile}" 2>/dev/null)" 2>/dev/null || true
  rm -f "${pidfile}"
fi
# Port free is handled by pidfile kill only (never fuser -k)
sleep 0.4
export GROKHUB_HOME="${root}"
export GROKHUB_PORT="${port}"
export GROKHUB_URL="http://127.0.0.1:${port}"
# Always relaunch the tree we just installed (never bare PATH grokhub without HOME —
# dual /usr + ~/.local installs would jump back to a broken system package).
if [ -x "${root}/packaging/aur/grokhub.sh" ]; then
  echo "[restart] exec ${root}/packaging/aur/grokhub.sh"
  nohup env HOME="\$HOME" GROKHUB_HOME="${root}" bash "${root}/packaging/aur/grokhub.sh" >/dev/null 2>&1 &
  exit 0
fi
if [ -n "${userBin}" ] && [ -x "${userBin}" ]; then
  echo "[restart] exec user bin ${userBin}"
  nohup env HOME="\$HOME" GROKHUB_HOME="${root}" "${userBin}" >/dev/null 2>&1 &
  exit 0
fi
if [ "${root}" = "/usr/lib/grokhub" ] && [ -x /usr/bin/grokhub ]; then
  echo "[restart] exec system /usr/bin/grokhub"
  nohup env HOME="\$HOME" GROKHUB_HOME="${root}" /usr/bin/grokhub >/dev/null 2>&1 &
  exit 0
fi
if command -v grokhub >/dev/null 2>&1; then
  echo "[restart] exec grokhub with GROKHUB_HOME=${root}"
  nohup env HOME="\$HOME" GROKHUB_HOME="${root}" grokhub >/dev/null 2>&1 &
  exit 0
fi
if [ -f "${root}/desktop/main.mjs" ] && command -v electron >/dev/null 2>&1; then
  if [ -f "${root}/.output/server/index.mjs" ]; then
    (
      cd "${root}"
      export PORT="${port}" NITRO_PORT="${port}" HOST=127.0.0.1 NITRO_HOST=127.0.0.1
      nohup node .output/server/index.mjs >>"${runtime}/grokhub/ui.log" 2>&1 &
      echo $! > "${pidfile}"
    )
    # Wait for UI health before Electron (avoids blank window)
    for i in $(seq 1 40); do
      if curl -sf -o /dev/null --max-time 1 "http://127.0.0.1:${port}/"; then
        break
      fi
      sleep 0.2
    done
  fi
  echo "[restart] exec electron"
  nohup env HOME="\$HOME" GROKHUB_HOME="${root}" electron --class=grokhub --name=grokhub "${root}/desktop/main.mjs" >/dev/null 2>&1 &
  exit 0
fi
echo "[restart] no launcher found"
exit 1
`.trim();
  const child = spawn("bash", ["-c", script], {
    detached: true,
    stdio: "ignore",
    env: {
      ...process.env,
      HOME: home || process.env.HOME || "/tmp",
      GROKHUB_HOME: root,
      GROKHUB_PORT: String(port),
      XDG_RUNTIME_DIR: process.env.XDG_RUNTIME_DIR || runtime,
    },
  });
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

## Desktop host (unsandboxed) — tool-first
When Desktop Host is LIVE you can act on the real machine (files, shell, apps).
Do NOT invent filesystem listings, command output, process state, or install paths.
Put host commands on their OWN line, alone:
HOST_CMD: ls -la "$HOME/Downloads"
Never glue HOST_CMD onto a prose sentence. Prefer one simple command (ls, head, cat, find, ps, stat).
For broad scans always bound the work (find -maxdepth 3, head -n 100). Never unbounded find/grep on / or $HOME.

CRITICAL — no fake progress:
- NEVER say "I'll check", "I'll probe", "let me investigate", "continuing the deep dive", or "would you like me to start" WITHOUT also emitting HOST_CMD lines in the SAME reply.
- If the user asks about their system, install, processes, logs, files, or a bug that needs local data: emit HOST_CMD immediately. Short preface is fine; commands must follow in the same message.
- Do not ask permission to run safe read-only diagnostics (ps, ls, find, journalctl --user -n, uname, which). Just run them.
- After HOST_RESULT arrives, summarize. Use more HOST_CMD rounds only when needed.
- Skip host tools only for pure chat, writing, or code that does not need live machine data.

## Connectors
Only use LIVE connector tools. Own line:
CONNECTOR_CMD: github user
Wait for CONNECTOR_RESULT; do not invent data.

## Safety
Refuse criminal activity, malware, exploits. Confirm destructive commands. Prefer non-interactive shell.`;
  const id = resolveMode(mode, prompt);
  if (id === "fast") return `${base}\nMode: Fast — concise answers, minimal preamble. Still emit HOST_CMD when machine data is required.`;
  if (id === "expert") return `${base}\nMode: Expert — reason carefully, surface tradeoffs. Prefer real HOST_CMD evidence over speculation.`;
  if (id === "heavy") return `${base}\nMode: Heavy (team of experts) — multi-angle synthesis backed by HOST_CMD when local facts matter.`;
  if (id === "build") return `${base}\nMode: Build — prioritize working code and file paths; use HOST_CMD to inspect the real tree.`;
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
  const home = os.homedir();
  // Prefer env, then writable user trees, then system. Dual-install safe.
  return [
    process.env.GROKHUB_HOME || "",
    path.join(home, ".local/lib/grokhub"),
    path.join(home, ".local/share/grokhub"),
    "/usr/lib/grokhub",
    path.resolve(process.cwd()),
  ].filter(Boolean);
}

/** Prefer a writable install when multiple exist (user over root /usr). */
async function findInstallRoot() {
  const roots = installRoots();
  const found = [];
  for (const root of roots) {
    if (await isInstallRoot(root)) found.push(root);
  }
  if (!found.length) return null;
  // Prefer first writable
  for (const root of found) {
    if (await pathWritable(root)) return root;
  }
  // Prefer user paths over /usr even if not writable test failed oddly
  for (const root of found) {
    if (!isSystemInstall(root)) return root;
  }
  return found[0];
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


async function readLocalVersion(root) {
  let version = APP_VERSION;
  let sha = null;
  let uiVersion = null;
  let uiStale = false;
  if (!root) return { version, sha, uiVersion, uiStale };
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
  try {
    const stamp = JSON.parse(
      await fs.readFile(path.join(root, ".output", "GROKHUB_BUILD.json"), "utf8"),
    );
    uiVersion = stamp.version ? String(stamp.version) : null;
    if (uiVersion && version && uiVersion !== version) uiStale = true;
    try {
      await fs.stat(path.join(root, ".output", "STALE_UI"));
      uiStale = true;
    } catch {}
  } catch {
    // Missing build stamp usually means old install / source-only update
    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      uiStale = true; // built UI present but unstamped → force refresh path
      uiVersion = "unknown";
    } catch {
      uiStale = true;
      uiVersion = null;
    }
  }
  return { version, sha, uiVersion, uiStale };
}

/**
 * Helpers for system vs user install paths.
 */
async function pathWritable(dir) {
  try {
    await fs.mkdir(dir, { recursive: true });
    const probe = path.join(dir, `.grokhub-write-test-${process.pid}`);
    await fs.writeFile(probe, "ok\n");
    await fs.unlink(probe);
    return true;
  } catch {
    return false;
  }
}

function isSystemInstall(root) {
  const r = path.resolve(String(root || ""));
  return r === "/usr/lib/grokhub" || r.startsWith("/usr/lib/grokhub" + path.sep);
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
        if (local.uiStale) {
          detail = `UI build stale (app v${local.version}, UI v${local.uiVersion || "?"}) — reinstall recommended`;
        } else {
          detail = `Up to date · v${local.version} · ${(local.sha || "").slice(0, 12)}`;
        }
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
  let writable = null;
  if (installRoot) {
    try {
      const probe = path.join(installRoot, `.grokhub-write-test-${process.pid}`);
      await fs.writeFile(probe, "ok\n");
      await fs.unlink(probe);
      writable = true;
    } catch {
      writable = false;
      if (isSystemInstall(installRoot) && !/admin|permission|writable/i.test(detail)) {
        detail = (detail ? detail + " · " : "") + "System install needs admin for updates (pkexec)";
      }
    }
  }
  return {
    currentVersion: local.version,
    uiVersion: local.uiVersion,
    uiStale: Boolean(local.uiStale),
    currentSha: localShort,
    remoteSha: remoteShort,
    remoteMessage,
    // Treat stale UI as needing update even when git SHA matches
    updateAvailable: Boolean(
      (remoteSha && !shaMatch(local.sha, remoteSha)) || local.uiStale,
    ),
    repo,
    branch,
    installRoot,
    writable,
    detail,
  };
}


/**
 * Atomic-ish install: build DEST.new from current + stage overlay, then swap.
 * Avoids half-written trees and mid-run file nukes that crash Electron/Nitro.
 */
async function installStagedTree(stageRoot, destRoot, steps) {
  const shBody = `#!/bin/bash
set -euo pipefail
STAGE=${JSON.stringify(stageRoot)}
DEST=${JSON.stringify(destRoot)}
NEW="$DEST.new"
PREV="$DEST.prev"
rm -rf "$NEW"
mkdir -p "$NEW"
# 1) Seed from live install (preserves .output and anything not in the stage)
if [ -d "$DEST" ]; then
  cp -a "$DEST"/. "$NEW"/ 2>/dev/null || true
fi
# 2) Overlay staged files (complete packages only — never partial renames of live dirs)
cp -a "$STAGE"/. "$NEW"/
# 3) Guarantee a bootable UI
if [ ! -f "$NEW/.output/server/index.mjs" ]; then
  if [ -f "$DEST/.output/server/index.mjs" ]; then
    rm -rf "$NEW/.output"
    cp -a "$DEST/.output" "$NEW/.output"
    echo "Preserved .output from live install"
  elif [ -f "/usr/lib/grokhub/.output/server/index.mjs" ]; then
    rm -rf "$NEW/.output"
    cp -a /usr/lib/grokhub/.output "$NEW/.output"
    echo "Preserved .output from system install"
  else
    echo "WARNING: no .output — UI may fail until repair-install" >&2
  fi
fi
# 4) Require desktop shell
if [ ! -f "$NEW/desktop/main.mjs" ]; then
  echo "ERROR: stage missing desktop/main.mjs" >&2
  exit 2
fi
# 5) Swap trees (running processes keep old inodes; new launch uses DEST)
rm -rf "$PREV"
if [ -d "$DEST" ]; then
  mv "$DEST" "$PREV"
fi
mv "$NEW" "$DEST"
# 6) Permissions for system tree
if [[ "$DEST" == /usr/lib/grokhub || "$DEST" == /usr/lib/grokhub/* ]]; then
  chown -R root:root "$DEST" 2>/dev/null || true
  find "$DEST" -type d -exec chmod 755 {} + 2>/dev/null || true
  find "$DEST" -type f -exec chmod 644 {} + 2>/dev/null || true
  [ -f "$DEST/desktop/main.mjs" ] && chmod 755 "$DEST/desktop/main.mjs" || true
  [ -f "$DEST/packaging/aur/grokhub.sh" ] && chmod 755 "$DEST/packaging/aur/grokhub.sh" || true
fi
# 7) Keep PREV for Settings → Undo last update (do not delete here)
if [ -d "$PREV" ]; then
  echo "$DEST" > "$PREV/.grokhub-rollback-target" 2>/dev/null || true
  echo "Previous tree kept for rollback at $PREV"
fi
echo OK
`

  async function runScript(elevated) {
    const shPath = path.join(os.tmpdir(), `grokhub-install-${process.pid}-${Date.now()}.sh`);
    await fs.writeFile(shPath, shBody, { mode: 0o755 });
    try {
      if (!elevated) {
        const { stdout, stderr } = await execAsync(`bash ${JSON.stringify(shPath)}`, {
          timeout: 300000,
          maxBuffer: 8 * 1024 * 1024,
          shell: "/bin/bash",
        });
        if (stdout) steps.push(...String(stdout).trim().split("\n").filter(Boolean).slice(0, 30));
        if (stderr && /error|WARNING|fail/i.test(stderr)) steps.push(stderr.trim().slice(0, 300));
        return { ok: true, elevated: false };
      }
      const elevators = [
        ["pkexec", ["bash", shPath]],
        ["sudo", ["-n", "bash", shPath]],
        ["sudo", ["bash", shPath]],
      ];
      let lastErr = "elevation failed";
      for (const [bin, args] of elevators) {
        try {
          await execAsync(`command -v ${bin}`, { timeout: 3000, shell: "/bin/bash" });
        } catch {
          continue;
        }
        try {
          steps.push(`Elevating via ${bin}…`);
          const { stdout, stderr } = await execAsync(
            `${bin} ${args.map((a) => JSON.stringify(a)).join(" ")}`,
            { timeout: 300000, maxBuffer: 8 * 1024 * 1024, shell: "/bin/bash" },
          );
          if (stdout) steps.push(...String(stdout).trim().split("\n").filter(Boolean).slice(0, 30));
          if (stderr && /error|denied|fail|WARNING/i.test(stderr)) steps.push(stderr.slice(0, 300));
          return { ok: true, elevated: true, via: bin };
        } catch (e) {
          lastErr = e instanceof Error ? e.message : String(e);
          steps.push(`${bin}: ${lastErr.slice(0, 160)}`);
        }
      }
      throw new Error(
        `Permission denied writing ${destRoot}. Approve pkexec/sudo, or run: sudo bash ${shPath}. Last: ${lastErr.slice(0, 200)}`,
      );
    } finally {
      await fs.unlink(shPath).catch(() => {});
    }
  }

  if (await pathWritable(destRoot)) {
    steps.push("Atomic install (user-writable)");
    return runScript(false);
  }
  if (process.platform !== "linux") {
    throw new Error(
      `Cannot write to ${destRoot} (permission denied). Reinstall to a user path or run the installer as admin.`,
    );
  }
  steps.push(`Need admin to write ${destRoot}`);
  return runScript(true);
}

async function applyUpdate(opts = {}) {
  if (updateInProgress) {
    return {
      ok: false,
      detail: "An update is already in progress",
      steps: ["Rejected: concurrent update"],
    };
  }
  updateInProgress = true;
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
    if (!root) return false;
    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      return true;
    } catch {}
    try {
      await fs.stat(path.join(root, "desktop", "main.mjs"));
      return true;
    } catch {}
    try {
      const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8"));
      return pkg.name === "grokhub" || pkg.name === "GrokHub";
    } catch {
      return false;
    }
  }

  // Prefer currently running install (GROKHUB_HOME), then user trees, then system
  const candidates = [
    process.env.GROKHUB_HOME,
    path.join(os.homedir(), ".local/lib/grokhub"),
    path.join(os.homedir(), ".local/share/grokhub"),
    "/usr/lib/grokhub",
    process.cwd(),
  ].filter(Boolean);

  let root = null;
  // Prefer writable install when several match (avoids pkexec/update crash on dual install)
  const matches = [];
  for (const c of candidates) {
    if (await isAppRoot(c)) matches.push(c);
  }
  for (const c of matches) {
    if (await pathWritable(c)) {
      root = c;
      break;
    }
  }
  if (!root && matches.length) root = matches[0];
  if (!root) {
    root = path.join(os.homedir(), ".local/lib/grokhub");
    await fs.mkdir(root, { recursive: true });
    steps.push(`Created ${root}`);
  }

  // Prefer ~/.local/lib for user fallback (matches install-arch --user / OpenClaw layout)
  const userRoot = path.join(os.homedir(), ".local/lib/grokhub");
  let targetRoot = root;
  const forceUser = Boolean(opts.userLocal);
  if (forceUser) {
    targetRoot = userRoot;
    await fs.mkdir(targetRoot, { recursive: true });
    steps.push(`User-local update target: ${targetRoot}`);
  }

  steps.push(`Install root: ${targetRoot}`);
  steps.push("User data / memory is outside the install tree and is not modified by updates");

  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
  const tarball = path.join(tmp, "update.tar.gz");
  const extractDir = path.join(tmp, "extract");
  const stageRoot = path.join(tmp, "stage");

  try {
    steps.push("Downloading update…");
    const headers = {
      accept: "application/vnd.github+json",
      "user-agent": "GrokHub-Updater",
    };
    if (token) headers.authorization = `Bearer ${token}`;

    // Prefer published release bundles (include prebuilt .output) over source tarball
    const urls = [];
    try {
      const relRes = await fetch(`https://api.github.com/repos/${repo}/releases/latest`, {
        headers,
      });
      if (relRes.ok) {
        const rel = await relRes.json();
        const assets = Array.isArray(rel.assets) ? rel.assets : [];
        for (const a of assets) {
          const name = String(a.name || "");
          if (/grokhub.*\.(tar\.gz|tgz)$/i.test(name) || /desktop.*bundle/i.test(name) || /with-output/i.test(name)) {
            if (a.browser_download_url) {
              urls.push(a.browser_download_url);
              steps.push(`Release asset: ${name}`);
            }
          }
        }
        // Also try a conventional asset name
        if (rel.tag_name) {
          urls.push(
            `https://github.com/${repo}/releases/download/${rel.tag_name}/grokhub-desktop-${rel.tag_name}.tar.gz`,
          );
        }
      }
    } catch {
      /* fall through to branch tarball */
    }
    urls.push(
      `https://api.github.com/repos/${repo}/tarball/${branch}`,
      `https://codeload.github.com/${repo}/tar.gz/refs/heads/${branch}`,
    );
    let ok = false;
    let last = "";
    for (const url of urls) {
      try {
        const res = await fetch(url, {
          headers: url.includes("api.github.com")
            ? headers
            : {
                "user-agent": "GrokHub-Updater",
                ...(token ? { authorization: `Bearer ${token}` } : {}),
              },
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
    await execAsync(`tar -xzf ${JSON.stringify(tarball)} -C ${JSON.stringify(extractDir)}`, {
      timeout: 120000,
      maxBuffer: 20 * 1024 * 1024,
      shell: "/bin/bash",
    });
    const entries = await fs.readdir(extractDir);
    let extracted = extractDir;
    if (entries.length === 1) {
      const only = path.join(extractDir, entries[0]);
      if ((await fs.stat(only)).isDirectory()) extracted = only;
    }

    // Build a clean stage directory (only files we install)
    await fs.mkdir(stageRoot, { recursive: true });
    // Always pull source + desktop so we can rebuild UI when GitHub has no .output
    const want = opts.factory
      ? [
          ".output",
          "desktop",
          "src",
          "scripts",
          "packaging",
          "public",
          "package.json",
          "package-lock.json",
          "vite.config.ts",
          "tsconfig.json",
          "startup.sh",
          ".grok",
          "APP_VERSION",
          "LICENSE",
          "README.md",
          "electron-builder.yml",
        ]
      : [
          ".output",
          "desktop",
          "src",
          "scripts",
          "packaging",
          "public",
          "package.json",
          "package-lock.json",
          "vite.config.ts",
          "tsconfig.json",
          "APP_VERSION",
          "LICENSE",
          "README.md",
        ];

    for (const name of want) {
      const src = path.join(extracted, name);
      try {
        await fs.stat(src);
      } catch {
        if (name === ".output") {
          steps.push("Skip .output (not in this archive)");
        } else {
          steps.push(`Skip ${name}`);
        }
        continue;
      }
      await execAsync(`cp -a ${JSON.stringify(src)} ${JSON.stringify(path.join(stageRoot, name))}`, {
        timeout: 180000,
        shell: "/bin/bash",
      });
      steps.push(`Staged ${name}`);
    }

    // Ensure stage has a FRESH .output matching this version (never silently keep stale UI)
    async function stageHasOutput() {
      try {
        await fs.stat(path.join(stageRoot, ".output", "server", "index.mjs"));
        return true;
      } catch {
        return false;
      }
    }

    async function tryRebuildUi(workRoot, label) {
      try {
        await fs.stat(path.join(workRoot, "package.json"));
        await fs.stat(path.join(workRoot, "src"));
      } catch {
        return false;
      }
      // Reuse node_modules from running install when possible (speed + offline)
      const nmCandidates = [
        path.join(workRoot, "node_modules"),
        path.join(targetRoot, "node_modules"),
        path.join(process.cwd(), "node_modules"),
        path.join(__dirname, "..", "node_modules"),
      ];
      let hasNm = false;
      for (const nm of nmCandidates) {
        try {
          await fs.stat(path.join(nm, "vite"));
          if (path.resolve(nm) !== path.resolve(workRoot, "node_modules")) {
            await execAsync(
              `rm -rf ${JSON.stringify(path.join(workRoot, "node_modules"))}; ln -s ${JSON.stringify(nm)} ${JSON.stringify(path.join(workRoot, "node_modules"))}`,
              { timeout: 60000, shell: "/bin/bash" },
            );
            steps.push(`Linked node_modules from ${nm}`);
          }
          hasNm = true;
          break;
        } catch {
          /* next */
        }
      }
      if (!hasNm) {
        steps.push("No node_modules available for UI rebuild");
        return false;
      }
      steps.push(`Rebuilding UI (${label})…`);
      try {
        await execAsync(
          `cd ${JSON.stringify(workRoot)} && GROKHUB_DESKTOP=1 NODE_ENV=production npm run desktop:build`,
          {
            timeout: 600000,
            maxBuffer: 40 * 1024 * 1024,
            shell: "/bin/bash",
            env: { ...process.env, GROKHUB_DESKTOP: "1", NODE_ENV: "production" },
          },
        );
        await fs.stat(path.join(workRoot, ".output", "server", "index.mjs"));
        steps.push("UI rebuild OK");
        return true;
      } catch (e) {
        steps.push(`UI rebuild failed: ${e instanceof Error ? e.message : e}`);
        return false;
      }
    }

    if (!(await stageHasOutput())) {
      // 1) Rebuild inside stage from staged source
      const rebuilt = await tryRebuildUi(stageRoot, "stage");
      if (!rebuilt) {
        // 2) Rebuild in extract tree then copy
        const rebuilt2 = await tryRebuildUi(extracted, "extract");
        if (rebuilt2) {
          await execAsync(
            `cp -a ${JSON.stringify(path.join(extracted, ".output"))} ${JSON.stringify(path.join(stageRoot, ".output"))}`,
            { timeout: 180000, shell: "/bin/bash" },
          );
          steps.push("Staged rebuilt .output from extract");
        }
      }
    }

    if (!(await stageHasOutput())) {
      // Last resort only: keep old UI but mark it so we don't lie about being fully updated
      const existingOut = path.join(targetRoot, ".output");
      try {
        await fs.stat(path.join(existingOut, "server", "index.mjs"));
        await execAsync(
          `cp -a ${JSON.stringify(existingOut)} ${JSON.stringify(path.join(stageRoot, ".output"))}`,
          { timeout: 180000, shell: "/bin/bash" },
        );
        steps.push(
          "WARNING: Using previous .output — UI may be stale. Install release asset grokhub-desktop-v*.tar.gz or rebuild with npm run desktop:build",
        );
        await fs.writeFile(
          path.join(stageRoot, ".output", "STALE_UI"),
          "archive lacked .output and rebuild failed\n",
        );
      } catch {
        const sysOut = path.join("/usr/lib/grokhub", ".output");
        try {
          await fs.stat(path.join(sysOut, "server", "index.mjs"));
          await execAsync(
            `cp -a ${JSON.stringify(sysOut)} ${JSON.stringify(path.join(stageRoot, ".output"))}`,
            { timeout: 180000, shell: "/bin/bash" },
          );
          steps.push("WARNING: Seeded stale .output from /usr/lib/grokhub");
        } catch {
          steps.push("ERROR: no UI build available — app window may not load until desktop:build");
        }
      }
    }

    // Stamp UI build so we can detect mismatch later
    try {
      if (await stageHasOutput()) {
        const stamp = {
          version: newVersion,
          builtAt: new Date().toISOString(),
          source: "update",
        };
        // newVersion not set yet — write after version resolve below
        await fs.writeFile(
          path.join(stageRoot, ".output", "GROKHUB_BUILD.json"),
          JSON.stringify({ pending: true }, null, 2),
        );
      }
    } catch {
      /* ignore */
    }

    // Version stamps in stage (so elevated copy installs them atomically)
    let newSha;
    let newVersion = APP_VERSION;
    try {
      const res = await fetch(
        `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(branch)}`,
        { headers },
      );
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
      const pkg = JSON.parse(
        await fs.readFile(path.join(stageRoot, "package.json"), "utf8").catch(async () =>
          fs.readFile(path.join(extracted, "package.json"), "utf8"),
        ),
      );
      if (pkg.version) newVersion = String(pkg.version);
    } catch {}
    if (newSha) {
      await fs.writeFile(path.join(stageRoot, "VERSION"), newSha + "\n");
      steps.push(`VERSION → ${String(newSha).slice(0, 12)}`);
    }
    await fs.writeFile(path.join(stageRoot, "APP_VERSION"), newVersion + "\n");
    steps.push(`APP_VERSION → ${newVersion}`);

    // Stamp UI build for mismatch detection (Settings / checkForUpdate)
    try {
      await fs.stat(path.join(stageRoot, ".output", "server", "index.mjs"));
      await fs.writeFile(
        path.join(stageRoot, ".output", "GROKHUB_BUILD.json"),
        JSON.stringify(
          {
            version: newVersion,
            sha: newSha || null,
            builtAt: new Date().toISOString(),
            source: "update",
          },
          null,
          2,
        ) + "\n",
      );
      steps.push(`UI build stamp v${newVersion}`);
    } catch {
      steps.push("No UI build stamp (missing .output)");
    }

    // Stop Nitro UI before swapping files (prevents crashes / partial reads)
    steps.push("Stopping UI server for safe install…");
    await stopUiServer(steps);

    // Install stage → target (atomic swap; elevates for /usr/lib/grokhub)
    let installResult;
    try {
      installResult = await installStagedTree(stageRoot, targetRoot, steps);
    } catch (e) {
      // Auto-fallback: system install without elevation → user local
      if (isSystemInstall(targetRoot) && !forceUser) {
        steps.push(
          `System update failed (${e instanceof Error ? e.message : e}). Falling back to user install…`,
        );
        targetRoot = userRoot;
        await fs.mkdir(targetRoot, { recursive: true });
        // Seed .output from system if stage still lacks it
        try {
          await fs.stat(path.join(stageRoot, ".output", "server", "index.mjs"));
        } catch {
          try {
            await fs.stat(path.join("/usr/lib/grokhub", ".output", "server", "index.mjs"));
            await execAsync(
              `cp -a ${JSON.stringify("/usr/lib/grokhub/.output")} ${JSON.stringify(path.join(stageRoot, ".output"))}`,
              { timeout: 180000, shell: "/bin/bash" },
            );
            steps.push("Seeded .output from system install into user stage");
          } catch {}
        }
        installResult = await installStagedTree(stageRoot, targetRoot, steps);
        steps.push(`User install ready: ${targetRoot}`);
        steps.push("Launch with: GROKHUB_HOME=" + targetRoot + " grokhub");
        // Best-effort user launcher
        try {
          const binDir = path.join(os.homedir(), ".local", "bin");
          await fs.mkdir(binDir, { recursive: true });
          const launcher = path.join(binDir, "grokhub-user");
          await fs.writeFile(
            launcher,
            `#!/bin/bash\nexport GROKHUB_HOME=${JSON.stringify(targetRoot)}\nexec ${JSON.stringify(path.join(targetRoot, "packaging/aur/grokhub.sh"))} "$@" 2>/dev/null || exec electron ${JSON.stringify(path.join(targetRoot, "desktop/main.mjs"))} "$@"\n`,
            { mode: 0o755 },
          );
          steps.push(`User launcher: ${launcher}`);
        } catch {}
      } else {
        throw e;
      }
    }

    // Verify UI exists on target
    try {
      await fs.stat(path.join(targetRoot, ".output", "server", "index.mjs"));
      steps.push("Verified .output/server/index.mjs");
    } catch {
      steps.push("Warning: .output/server/index.mjs missing after update");
    }

    // Never run install-arch.sh mid-session — it reinstalls packages while UI is half-down.
    steps.push("Skipped install-arch.sh (use repair-install offline if needed)");

    let status;
    try {
      status = await checkForUpdate({ repo, branch, token });
      if (status.updateAvailable && newSha && shaMatch(newSha, status.remoteSha)) {
        status.updateAvailable = false;
        status.currentSha = String(newSha).slice(0, 12);
        status.currentVersion = newVersion;
        status.detail = `Up to date · v${newVersion} · ${String(newSha).slice(0, 12)}`;
      }
      // If we fell back to user local, report that path
      status.installRoot = targetRoot;
    } catch {}

    const doRestart = opts.restart !== false;
    if (doRestart) {
      steps.push("Restarting GrokHub…");
      process.env.GROKHUB_HOME = targetRoot;
      try {
        scheduleAppRestart(targetRoot);
        steps.push("Restart scheduled");
      } catch (re) {
        // Install already succeeded — never fail the whole update on relaunch glitches
        steps.push(
          `Restart helper error (app is updated; relaunch manually): ${
            re instanceof Error ? re.message : re
          }`,
        );
        steps.push(`Manual: GROKHUB_HOME=${targetRoot} ${targetRoot}/packaging/aur/grokhub.sh`);
      }
    } else {
      steps.push("Done — relaunch GrokHub to load the new build");
    }

    return {
      ok: true,
      detail: `Updated to v${newVersion} (${(newSha || "latest").toString().slice(0, 12)}) @ ${targetRoot}`,
      steps,
      newSha: newSha ? String(newSha).slice(0, 12) : undefined,
      newVersion,
      installRoot: targetRoot,
      elevated: Boolean(installResult && installResult.elevated),
      restarting: doRestart,
      status,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg.slice(0, 2000), steps, installRoot: targetRoot };
  } finally {
    updateInProgress = false;
    try {
      if (typeof tmp !== "undefined" && tmp) {
        await fs.rm(tmp, { recursive: true, force: true }).catch(() => {});
      }
    } catch {
      /* ignore */
    }
  }
}

/** List / apply rollback from DEST.prev if present */
async function checkRollback(opts = {}) {
  const root =
    opts.root ||
    process.env.GROKHUB_HOME ||
    (await findInstallRoot()) ||
    path.join(os.homedir(), ".local/lib/grokhub");
  const prev = root + ".prev";
  try {
    await fs.stat(path.join(prev, "desktop", "main.mjs"));
    let version = "?";
    try {
      version = (await fs.readFile(path.join(prev, "APP_VERSION"), "utf8")).trim() || version;
    } catch {}
    return {
      ok: true,
      available: true,
      prevRoot: prev,
      installRoot: root,
      prevVersion: version,
      detail: `Rollback available → v${version} (${prev})`,
    };
  } catch {
    return {
      ok: true,
      available: false,
      installRoot: root,
      detail: "No previous install kept for rollback",
    };
  }
}

async function applyRollback(opts = {}) {
  if (updateInProgress) {
    return { ok: false, detail: "Update already in progress", steps: [] };
  }
  updateInProgress = true;
  const steps = [];
  try {
    const root =
      opts.root ||
      process.env.GROKHUB_HOME ||
      (await findInstallRoot()) ||
      path.join(os.homedir(), ".local/lib/grokhub");
    const prev = root + ".prev";
    try {
      await fs.stat(path.join(prev, "desktop", "main.mjs"));
    } catch {
      return { ok: false, detail: "No rollback snapshot (.prev)", steps: ["Missing " + prev] };
    }
    steps.push(`Rollback ${root} ← ${prev}`);
    await stopUiServer(steps);

    const sh = `#!/bin/bash
set -euo pipefail
DEST=${JSON.stringify(root)}
PREV=${JSON.stringify(prev)}
BROKEN="$DEST.broken-$(date +%s)"
if [ ! -d "$PREV" ]; then echo "no prev"; exit 2; fi
if [ -d "$DEST" ]; then mv "$DEST" "$BROKEN"; fi
mv "$PREV" "$DEST"
# Drop the broken tree after swap (old process should be exiting)
rm -rf "$BROKEN" 2>/dev/null || true
echo OK
`
    const shPath = path.join(os.tmpdir(), `grokhub-rollback-${process.pid}.sh`);
    await fs.writeFile(shPath, sh, { mode: 0o755 });
    try {
      const elevated = !(await pathWritable(root));
      if (elevated) {
        steps.push("Elevating for rollback…");
        try {
          await execAsync(`pkexec bash ${JSON.stringify(shPath)}`, {
            timeout: 180000,
            shell: "/bin/bash",
          });
        } catch {
          await execAsync(`sudo bash ${JSON.stringify(shPath)}`, {
            timeout: 180000,
            shell: "/bin/bash",
          });
        }
      } else {
        await execAsync(`bash ${JSON.stringify(shPath)}`, { timeout: 180000, shell: "/bin/bash" });
      }
      steps.push("Rollback swap complete");
    } finally {
      await fs.unlink(shPath).catch(() => {});
    }

    const doRestart = opts.restart !== false;
    if (doRestart) {
      process.env.GROKHUB_HOME = root;
      steps.push("Restarting after rollback…");
      scheduleAppRestart(root);
    }
    return {
      ok: true,
      detail: "Rolled back to previous install",
      steps,
      restarting: doRestart,
      installRoot: root,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(msg);
    return { ok: false, detail: msg.slice(0, 2000), steps };
  } finally {
    updateInProgress = false;
  }
}

/**
 * Keep at most one rollback tree; drop .prev older than maxAgeMs after a healthy boot.
 */
async function pruneStalePrevInstalls(root, steps, maxAgeMs = 7 * 86400_000) {
  try {
    const prev = root.endsWith(path.sep) ? root.slice(0, -1) + ".prev" : root + ".prev";
    const st = await fs.stat(prev).catch(() => null);
    if (!st || !st.isDirectory()) return;
    const age = Date.now() - (st.mtimeMs || 0);
    if (age < maxAgeMs) {
      steps.push(`Kept rollback tree (${Math.round(age / 3600000)}h old)`);
      return;
    }
    const entry = path.join(root, ".output", "server", "index.mjs");
    const bridgeFile = path.join(root, "desktop", "grok-bridge.cjs");
    await fs.access(entry);
    const src = await fs.readFile(bridgeFile, "utf8");
    if (!src.includes("factoryReinstall")) {
      steps.push("Skip .prev prune — current bridge incomplete");
      return;
    }
    await fs.rm(prev, { recursive: true, force: true });
    steps.push(`Pruned stale rollback tree (${Math.round(age / 86400000)}d old)`);
  } catch (e) {
    steps.push(`Prev prune skipped: ${e instanceof Error ? e.message : "err"}`);
  }
}

async function postUpdateSelfTest(opts = {}) {
  const root =
    opts.root || process.env.GROKHUB_HOME || (await findInstallRoot()) || process.cwd();
  const checks = [];
  let ok = true;
  try {
    await fs.stat(path.join(root, "desktop", "main.mjs"));
    checks.push("desktop/main.mjs OK");
  } catch {
    ok = false;
    checks.push("desktop/main.mjs MISSING");
  }
  try {
    await fs.stat(path.join(root, ".output", "server", "index.mjs"));
    checks.push(".output/server OK");
  } catch {
    ok = false;
    checks.push(".output/server MISSING — run repair-install");
  }
  let version = APP_VERSION;
  try {
    version = (await fs.readFile(path.join(root, "APP_VERSION"), "utf8")).trim() || version;
  } catch {}
  const local = await readLocalVersion(root);
  if (ok) {
    await pruneStalePrevInstalls(root, checks, 7 * 86400_000);
  }
  return {
    ok,
    version,
    sha: local.sha,
    installRoot: root,
    detail: ok ? `Self-test OK · v${version}` : `Self-test FAILED · v${version}`,
    checks,
  };
}

/** Factory reinstall = full applyUpdate with factory file set */
async function factoryReinstall(opts = {}) {
  const steps = ["Factory reinstall requested"];
  if (opts.wipeMemory) {
    steps.push("Note: wipeMemory should be handled by the renderer before restart");
  }
  if (opts.clearSelfMod) {
    try {
      const selfMod = require("./self-mod.cjs");
      if (typeof selfMod.clearJournal === "function") {
        await selfMod.clearJournal();
        steps.push("Cleared self-mod journal");
      }
    } catch {
      steps.push("Self-mod clear skipped");
    }
  }
  const r = await applyUpdate({
    ...opts,
    factory: true,
    restart: opts.restart !== false,
  });
  return {
    ...r,
    steps: [...steps, ...(r.steps || [])],
    detail: r.detail || "Factory reinstall finished",
  };
}


// ── xAI Grok OAuth device-code (OpenClaw / Grok CLI public client) ──
const XAI_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_OAUTH_SCOPE = "openid profile email offline_access grok-cli:access api:access";
const XAI_OAUTH_DISCOVERY = "https://auth.x.ai/.well-known/openid-configuration";
const XAI_DEVICE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";
const XAI_UA = "GrokHub/0.8.10 (xAI OAuth; Electron)";

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

function jwtExpMs(accessToken) {
  try {
    const part = String(accessToken || "").split(".")[1];
    if (!part) return null;
    const json = Buffer.from(part, "base64url").toString("utf8");
    const payload = JSON.parse(json);
    if (typeof payload.exp === "number" && payload.exp > 0) return payload.exp * 1000;
  } catch {
    /* ignore */
  }
  return null;
}

async function oauthEnsure(tokens) {
  if (!tokens || !tokens.accessToken) {
    throw new Error("No OAuth access token");
  }
  let access = String(tokens.accessToken);
  let next = { ...tokens };
  let refreshed = false;
  // Proactive refresh ~30 min before hard expiry (access tokens ~6h)
  const skew = 30 * 60 * 1000;
  let exp =
    typeof tokens.expiresAt === "number" ? tokens.expiresAt : jwtExpMs(tokens.accessToken);
  if (exp != null && typeof next.expiresAt !== "number") {
    next = { ...next, expiresAt: exp };
  }
  const needsRefresh =
    (typeof exp === "number" && exp - skew < Date.now()) ||
    (exp == null &&
      tokens.refreshToken &&
      tokens.connectedAt &&
      Date.now() - Number(tokens.connectedAt) >= 5 * 60 * 60 * 1000);

  if (needsRefresh && tokens.refreshToken) {
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
    let expiresAt = j.expires_in ? Date.now() + Number(j.expires_in) * 1000 : tokens.expiresAt;
    const jwtE = jwtExpMs(j.access_token);
    if (jwtE && (!expiresAt || jwtE < expiresAt)) expiresAt = jwtE;
    next = {
      ...tokens,
      accessToken: j.access_token,
      refreshToken: j.refresh_token || tokens.refreshToken,
      expiresAt,
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
  const aspectIn = req.aspect || "auto";
  const qHint =
    quality === "quality"
      ? mediaKind === "video"
        ? "cinematic motion, high detail, smooth camera"
        : "ultra detailed, sharp focus, professional lighting"
      : mediaKind === "video"
        ? "clear motion, simple scene"
        : "clean composition, efficient render";
  const fullPrompt = `${prompt}\n\n[${qHint}]`;
  const headers = {
    "content-type": "application/json",
    authorization: `Bearer ${apiKey}`,
    accept: "application/json",
  };

  function videoAspect(a) {
    if (a === "9:16" || a === "2:3" || a === "3:4" || a === "1:2") return "9:16";
    if (a === "1:1") return "1:1";
    return "16:9";
  }
  function imageAspect(a) {
    if (!a || a === "auto") return "auto";
    const allowed = new Set([
      "1:1","16:9","9:16","4:3","3:4","3:2","2:3","2:1","1:2",
      "19.5:9","9:19.5","20:9","9:20","auto",
    ]);
    return allowed.has(a) ? a : "auto";
  }
  async function sleep(ms) {
    return new Promise((r) => setTimeout(r, ms));
  }
  async function pollVideo(requestId, maxMs = 180000) {
    const started = Date.now();
    let delay = 1500;
    while (Date.now() - started < maxMs) {
      const res = await fetch(`${XAI_BASE}/videos/${encodeURIComponent(requestId)}`, {
        headers: { authorization: `Bearer ${apiKey}`, accept: "application/json" },
      });
      const data = await res.json().catch(() => ({}));
      if (!res.ok) {
        if (res.status === 404 && Date.now() - started < 15000) {
          await sleep(delay);
          continue;
        }
        return {
          error:
            (typeof data.error === "string" && data.error) ||
            data.error?.message ||
            `poll ${res.status}`,
        };
      }
      const status = String(data.status || "").toLowerCase();
      if (["done", "completed", "succeeded", "success"].includes(status)) {
        const url = data.video?.url || data.video_url || data.url;
        if (url) return { url };
        return { error: "video done but no URL" };
      }
      if (["failed", "expired", "error", "cancelled"].includes(status)) {
        return {
          error:
            (typeof data.error === "string" && data.error) ||
            data.error?.message ||
            `video ${status}`,
        };
      }
      await sleep(delay);
      delay = Math.min(5000, Math.floor(delay * 1.25));
    }
    return { error: "video generation timed out" };
  }

  if (mediaKind === "video") {
    // xAI video API: NO `size` — use aspect_ratio + resolution + duration, poll request_id
    const videoModels = [req.model, "grok-imagine-video-1.5", "grok-imagine-video"].filter(Boolean);
    const aspect = videoAspect(aspectIn);
    const resolution = quality === "quality" ? "1080p" : "720p";
    const duration = Math.min(15, Math.max(5, Number(req.duration) || (quality === "quality" ? 10 : 6)));
    let lastErr = "video generation unavailable";
    for (const model of videoModels) {
      try {
        const body = {
          model,
          prompt: fullPrompt,
          duration,
          aspect_ratio: aspect,
          resolution,
        };
        if (req.referenceDataUrl) body.image = { url: req.referenceDataUrl };
        const res = await fetch(`${XAI_BASE}/videos/generations`, {
          method: "POST",
          headers,
          body: JSON.stringify(body),
        });
        const data = await res.json().catch(() => ({}));
        if (!res.ok) {
          lastErr =
            (typeof data.error === "string" && data.error) ||
            data.error?.message ||
            `xAI ${res.status}`;
          continue;
        }
        const immediate =
          data.video?.url || data.url || data.data?.[0]?.video_url || data.data?.[0]?.url;
        if (immediate) {
          return {
            ok: true,
            videoDataUrl: immediate,
            model: data.model || model,
            source: "xai",
            mediaKind: "video",
          };
        }
        const requestId = data.request_id || data.id;
        if (requestId) {
          const polled = await pollVideo(requestId);
          if (polled.url) {
            return {
              ok: true,
              videoDataUrl: polled.url,
              model: data.model || model,
              source: "xai",
              mediaKind: "video",
            };
          }
          lastErr = polled.error || lastErr;
          continue;
        }
        lastErr = "video response missing request_id";
      } catch (e) {
        lastErr = e instanceof Error ? e.message : "network error";
      }
    }
    return {
      ok: false,
      error: lastErr + " — try Image mode or check SuperGrok video access",
      mediaKind: "video",
    };
  }

  // Images: aspect_ratio + resolution (1k/2k). Never send OpenAI-style `size`.
  const models = [
    req.model,
    quality === "quality" ? "grok-imagine-image-quality" : "grok-imagine-image",
    "grok-imagine-image-quality",
    "grok-imagine-image",
    "grok-2-image",
    "grok-2-image-1212",
  ].filter(Boolean);
  const aspect = imageAspect(aspectIn);
  const resolution = quality === "quality" ? "2k" : "1k";
  let lastErr = "image generation failed";
  for (const model of models) {
    try {
      const body = {
        model,
        prompt: fullPrompt,
        n: Math.min(4, Math.max(1, Number(req.n) || 1)),
        response_format: "b64_json",
        resolution,
      };
      if (aspect) body.aspect_ratio = aspect;
      if (req.referenceDataUrl) body.image = { url: req.referenceDataUrl };
      const res = await fetch(`${XAI_BASE}/images/generations`, {
        method: "POST",
        headers,
        body: JSON.stringify(body),
      });
      let data = await res.json().catch(() => ({}));
      if (!res.ok) {
        lastErr =
          (typeof data.error === "string" && data.error) ||
          data.error?.message ||
          `xAI image ${res.status} (${model})`;
        // strip resolution if rejected
        if (/resolution|argument not supported|size/i.test(String(lastErr))) {
          const body2 = { ...body };
          delete body2.resolution;
          const res2 = await fetch(`${XAI_BASE}/images/generations`, {
            method: "POST",
            headers,
            body: JSON.stringify(body2),
          });
          data = await res2.json().catch(() => ({}));
          if (!res2.ok) {
            lastErr =
              (typeof data.error === "string" && data.error) ||
              data.error?.message ||
              lastErr;
            continue;
          }
        } else {
          continue;
        }
      }
      const b64 = data.data?.[0]?.b64_json || data.data?.[0]?.b64 || data.data?.[0]?.image || "";
      const url = data.data?.[0]?.url || "";
      if (b64) {
        return {
          ok: true,
          imageDataUrl: String(b64).startsWith("data:") ? b64 : `data:image/png;base64,${b64}`,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      if (url) {
        return {
          ok: true,
          imageDataUrl: url,
          model: data.model || model,
          source: "xai",
          mediaKind: "image",
        };
      }
      lastErr = "empty image response";
    } catch (e) {
      lastErr = e instanceof Error ? e.message : "network error";
    }
  }
  return { ok: false, error: lastErr, mediaKind: "image" };
}


async function callXaiStt(req = {}) {
  const apiKey =
    (req.accessToken && String(req.accessToken).trim()) ||
    (req.apiKey && String(req.apiKey).trim()) ||
    process.env.XAI_API_KEY ||
    process.env.GROK_API_KEY ||
    "";
  if (!apiKey) {
    return { ok: false, error: "Not connected — sign in to Grok for voice transcription" };
  }
  const b64 = String(req.audioBase64 || req.audio || "").replace(/^data:[^;]+;base64,/, "");
  if (!b64) return { ok: false, error: "empty audio" };
  const buf = Buffer.from(b64, "base64");
  if (!buf.length) return { ok: false, error: "empty audio buffer" };
  const mime = req.mimeType || "audio/webm";
  const name =
    req.fileName ||
    (mime.includes("wav")
      ? "speech.wav"
      : mime.includes("ogg")
        ? "speech.ogg"
        : mime.includes("mp4") || mime.includes("m4a")
          ? "speech.m4a"
          : "speech.webm");
  try {
    const form = new FormData();
    form.append("format", "true");
    form.append("language", req.language || "en");
    form.append("file", new Blob([new Uint8Array(buf)], { type: mime }), name);
    const res = await fetch(`${XAI_BASE}/stt`, {
      method: "POST",
      headers: { authorization: `Bearer ${apiKey}` },
      body: form,
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
      return {
        ok: false,
        error:
          (typeof data.error === "string" && data.error) ||
          data.error?.message ||
          `STT ${res.status}`,
      };
    }
    const text = String(data.text || data.transcript || data.result || "").trim();
    if (!text) return { ok: false, error: "empty transcript" };
    return { ok: true, text };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : "STT network error" };
  }
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
  const temperature = typeof req.temperature === "number" ? req.temperature : 0.6;
  // Higher caps so long tool-using turns don't get cut mid-stream
  const max_tokens = freeTier ? 2048 : 8192;
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
  callXaiStt,
  probeXaiKey,
  checkForUpdate,
  applyUpdate,
  checkRollback,
  applyRollback,
  postUpdateSelfTest,
  factoryReinstall,
  scheduleAppRestart,
  oauthStart,
  oauthPoll,
  oauthEnsure,
};
