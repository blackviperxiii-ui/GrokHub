/**
 * GitHub update helpers — Node only (server / Electron main).
 *
 * Packaged Arch installs live at /usr/lib/grokhub with only `.output` + `desktop`
 * (no .git / package.json). Updates download a GitHub tarball and swap those
 * trees — never `git reset --hard` (that wipes local work).
 */
import { execFile as execFileCb, spawn } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import { createWriteStream, existsSync, readFileSync } from "node:fs";
import path from "node:path";
import os from "node:os";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";
import { APP_VERSION as BUILTIN_VERSION } from "./version";

const execFileAsync = promisify(execFileCb);

export const DEFAULT_REPO = "blackviperxiii-ui/Grok-Hub";
export const DEFAULT_BRANCH = "main";
export const APP_VERSION = BUILTIN_VERSION;

export type UpdateStatus = {
  currentVersion: string;
  currentSha: string | null;
  remoteSha: string | null;
  remoteMessage: string | null;
  updateAvailable: boolean;
  repo: string;
  branch: string;
  installRoot: string | null;
  /** false when install root is root-owned (e.g. /usr/lib/grokhub) */
  writable?: boolean | null;
  detail: string;
};

export type UpdateResult = {
  ok: boolean;
  detail: string;
  steps: string[];
  newSha?: string;
  newVersion?: string;
  restarting?: boolean;
  installRoot?: string;
  elevated?: boolean;
  /** Post-apply check so UI can clear “Update available” immediately */
  status?: UpdateStatus;
};

type RunResult = { stdout: string; stderr: string };

async function run(
  cmd: string,
  args: string[],
  opts: { cwd?: string; timeout?: number; env?: NodeJS.ProcessEnv } = {},
): Promise<RunResult> {
  try {
    const { stdout, stderr } = await execFileAsync(cmd, args, {
      cwd: opts.cwd,
      timeout: opts.timeout ?? 120_000,
      env: { ...process.env, ...opts.env, GIT_TERMINAL_PROMPT: "0" },
      maxBuffer: 20 * 1024 * 1024,
    });
    return { stdout: String(stdout || ""), stderr: String(stderr || "") };
  } catch (e) {
    const err = e as {
      message?: string;
      stdout?: string | Buffer;
      stderr?: string | Buffer;
    };
    const stderr = String(err.stderr || "");
    const stdout = String(err.stdout || "");
    const msg = [err.message, stderr, stdout].filter(Boolean).join("\n").slice(0, 4000);
    throw new Error(msg || `Command failed: ${cmd} ${args.join(" ")}`);
  }
}

/** Compare git SHAs allowing 7–40 char prefixes. */
export function shaMatch(a: string | null | undefined, b: string | null | undefined): boolean {
  if (!a || !b) return false;
  const x = a.trim().toLowerCase();
  const y = b.trim().toLowerCase();
  if (!x || !y) return false;
  const n = Math.min(x.length, y.length);
  if (n < 7) return x === y;
  return x.slice(0, n) === y.slice(0, n);
}

function installRoots(): string[] {
  return [
    process.env.GROKHUB_HOME || "",
    "/usr/lib/grokhub",
    path.join(os.homedir(), ".local/share/grokhub"),
    path.resolve(process.cwd()),
  ].filter(Boolean);
}

async function isAppRoot(root: string): Promise<boolean> {
  try {
    await fs.stat(path.join(root, ".output", "server", "index.mjs"));
    return true;
  } catch {
    /* fall through */
  }
  try {
    await fs.stat(path.join(root, "package.json"));
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8")) as {
      name?: string;
    };
    return pkg.name === "grokhub" || pkg.name === "GrokHub";
  } catch {
    return false;
  }
}

async function findInstallRoot(): Promise<string | null> {
  for (const root of installRoots()) {
    if (await isAppRoot(root)) return root;
  }
  return null;
}

function readBuiltinVersion(): string {
  // Prefer package.json next to this module's project root when available
  const candidates = [
    path.join(process.cwd(), "package.json"),
    path.join(process.env.GROKHUB_HOME || "", "package.json"),
    path.join("/usr/lib/grokhub", "package.json"),
  ];
  for (const f of candidates) {
    try {
      if (!f || !existsSync(f)) continue;
      const pkg = JSON.parse(readFileSync(f, "utf8")) as { version?: string; name?: string };
      if (pkg.version && (pkg.name === "grokhub" || !pkg.name)) {
        return String(pkg.version);
      }
      if (pkg.version) return String(pkg.version);
    } catch {
      /* next */
    }
  }
  return BUILTIN_VERSION;
}

async function readLocalVersion(
  root: string | null,
): Promise<{ version: string; sha: string | null }> {
  let version = readBuiltinVersion();
  let sha: string | null = null;
  if (!root) return { version, sha };

  try {
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8")) as {
      version?: string;
    };
    if (pkg.version) version = String(pkg.version);
  } catch {
    /* packaged installs may lack package.json */
  }
  try {
    const v = (await fs.readFile(path.join(root, "APP_VERSION"), "utf8")).trim();
    if (v) version = v;
  } catch {
    /* ignore */
  }
  try {
    const v = (await fs.readFile(path.join(root, "VERSION"), "utf8")).trim();
    if (v) sha = v.split(/\s+/)[0] || null;
  } catch {
    /* ignore */
  }
  if (!sha) {
    try {
      const { stdout } = await run("git", ["rev-parse", "HEAD"], { cwd: root, timeout: 8000 });
      sha = stdout.trim() || null;
    } catch {
      /* ignore */
    }
  }
  return { version, sha };
}

async function fetchRemoteHead(
  repo: string,
  branch: string,
  token?: string,
): Promise<{ sha: string; message: string } | null> {
  const headers: Record<string, string> = {
    accept: "application/vnd.github+json",
    "user-agent": "GrokHub-Updater",
  };
  if (token) headers.authorization = `Bearer ${token}`;
  const url = `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(branch)}`;
  const res = await fetch(url, { headers });
  if (!res.ok) return null;
  const data = (await res.json()) as {
    sha?: string;
    commit?: { message?: string };
  };
  if (!data.sha) return null;
  return {
    sha: data.sha,
    message: (data.commit?.message || "").split("\n")[0] || "",
  };
}

export async function checkForUpdate(opts?: {
  repo?: string;
  branch?: string;
  token?: string;
}): Promise<UpdateStatus> {
  const repo = opts?.repo || process.env.GROKHUB_REPO || DEFAULT_REPO;
  const branch = opts?.branch || process.env.GROKHUB_BRANCH || DEFAULT_BRANCH;
  const token =
    opts?.token ||
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN ||
    process.env.GROKHUB_GITHUB_TOKEN ||
    "";
  const installRoot = await findInstallRoot();
  const local = await readLocalVersion(installRoot);
  let remote: { sha: string; message: string } | null = null;
  let detail = "";
  try {
    remote = await fetchRemoteHead(repo, branch, token || undefined);
    if (!remote) {
      detail = token
        ? "Could not read remote commit (check repo access / token scopes)."
        : "Could not read remote commit (private repo needs a GitHub token).";
    } else if (shaMatch(local.sha, remote.sha)) {
      detail = `Up to date · v${local.version} · ${local.sha?.slice(0, 12) || "local"}`;
    } else if (!local.sha) {
      detail = "Local VERSION unknown — install recommended.";
    } else {
      detail = `Update available · ${local.sha.slice(0, 12)} → ${remote.sha.slice(0, 12)}`;
    }
  } catch (e) {
    detail = e instanceof Error ? e.message : "Update check failed";
  }

  const remoteShort = remote?.sha ? remote.sha.slice(0, 12) : null;
  const localShort = local.sha ? local.sha.slice(0, 12) : null;
  const updateAvailable = Boolean(remote && !shaMatch(local.sha, remote.sha));

  return {
    currentVersion: local.version,
    currentSha: localShort,
    remoteSha: remoteShort,
    remoteMessage: remote?.message ?? null,
    updateAvailable,
    repo,
    branch,
    installRoot,
    detail,
  };
}

async function downloadGithubTarball(opts: {
  repo: string;
  branch: string;
  token?: string;
  destFile: string;
}): Promise<void> {
  const headers: Record<string, string> = {
    accept: "application/vnd.github+json",
    "user-agent": "GrokHub-Updater",
  };
  if (opts.token) headers.authorization = `Bearer ${opts.token}`;

  const urls = [
    `https://api.github.com/repos/${opts.repo}/tarball/${encodeURIComponent(opts.branch)}`,
    `https://codeload.github.com/${opts.repo}/tar.gz/refs/heads/${encodeURIComponent(opts.branch)}`,
  ];

  let lastErr = "download failed";
  for (const url of urls) {
    try {
      const res = await fetch(url, {
        headers: url.includes("api.github.com")
          ? headers
          : opts.token
            ? { ...headers, authorization: `Bearer ${opts.token}` }
            : { "user-agent": "GrokHub-Updater" },
        redirect: "follow",
      });
      if (!res.ok || !res.body) {
        lastErr = `HTTP ${res.status} from ${url}`;
        continue;
      }
      const nodeStream = Readable.fromWeb(res.body as import("stream/web").ReadableStream);
      await pipeline(nodeStream, createWriteStream(opts.destFile));
      const st = await fs.stat(opts.destFile);
      if (st.size < 1000) {
        lastErr = "Downloaded archive too small";
        continue;
      }
      return;
    } catch (e) {
      lastErr = e instanceof Error ? e.message : String(e);
    }
  }
  throw new Error(`Could not download update archive: ${lastErr}`);
}

async function extractTarball(tarball: string, destDir: string): Promise<string> {
  await fs.mkdir(destDir, { recursive: true });
  await run("tar", ["-xzf", tarball, "-C", destDir], { timeout: 120_000 });
  const entries = await fs.readdir(destDir);
  if (entries.length === 1) {
    const only = path.join(destDir, entries[0]!);
    const st = await fs.stat(only);
    if (st.isDirectory()) return only;
  }
  return destDir;
}

async function copyTree(src: string, dest: string): Promise<void> {
  await fs.mkdir(dest, { recursive: true });
  try {
    await run("cp", ["-a", `${src}/.`, dest], { timeout: 120_000 });
    return;
  } catch {
    /* fall through */
  }
  await fs.cp(src, dest, { recursive: true, force: true });
}

async function replaceDir(src: string, dest: string, steps: string[]): Promise<void> {
  try {
    await fs.stat(src);
  } catch {
    steps.push(`Skip missing ${path.basename(src)}`);
    return;
  }
  const backup = `${dest}.bak-${Date.now()}`;
  let hadDest = false;
  try {
    await fs.stat(dest);
    hadDest = true;
    await fs.rename(dest, backup);
  } catch {
    hadDest = false;
  }
  try {
    await copyTree(src, dest);
    if (hadDest) {
      await fs.rm(backup, { recursive: true, force: true }).catch(() => null);
    }
    steps.push(`Updated ${path.basename(dest)}`);
  } catch (e) {
    if (hadDest) {
      await fs.rm(dest, { recursive: true, force: true }).catch(() => null);
      await fs.rename(backup, dest).catch(() => null);
    }
    throw e;
  }
}

async function deployExtracted(
  extracted: string,
  root: string,
  steps: string[],
): Promise<{ sha?: string; version?: string }> {
  await replaceDir(path.join(extracted, ".output"), path.join(root, ".output"), steps);
  await replaceDir(path.join(extracted, "desktop"), path.join(root, "desktop"), steps);

  let version: string | undefined;
  for (const name of ["package.json", "package-lock.json", "scripts", "packaging"]) {
    const src = path.join(extracted, name);
    try {
      const st = await fs.stat(src);
      if (st.isDirectory()) {
        await replaceDir(src, path.join(root, name), steps);
      } else if (st.isFile()) {
        await fs.copyFile(src, path.join(root, name));
        steps.push(`Updated ${name}`);
        if (name === "package.json") {
          try {
            const pkg = JSON.parse(await fs.readFile(src, "utf8")) as { version?: string };
            if (pkg.version) version = String(pkg.version);
          } catch {
            /* ignore */
          }
        }
      }
    } catch {
      /* optional */
    }
  }

  let sha: string | undefined;
  const base = path.basename(extracted);
  const m = base.match(/-([0-9a-f]{7,40})$/i);
  if (m?.[1]) sha = m[1];
  try {
    const v = (await fs.readFile(path.join(extracted, "VERSION"), "utf8")).trim();
    if (v) sha = v.split(/\s+/)[0] || sha;
  } catch {
    /* ignore */
  }
  return { sha, version };
}

async function stampInstall(
  root: string,
  sha: string | undefined,
  version: string | undefined,
  steps: string[],
): Promise<void> {
  if (sha) {
    await fs.writeFile(path.join(root, "VERSION"), `${sha}\n`);
    steps.push(`VERSION → ${sha.slice(0, 12)}`);
  }
  const ver = version || readBuiltinVersion();
  await fs.writeFile(path.join(root, "APP_VERSION"), `${ver}\n`);
  steps.push(`APP_VERSION → ${ver}`);
}

/**
 * Schedule a full app restart (UI server + Electron) after a successful update.
 * Spawns a detached helper so the current process can exit cleanly.
 */
export function scheduleAppRestart(opts?: { port?: string; appRoot?: string }): void {
  const port = opts?.port || process.env.GROKHUB_PORT || "18765";
  let appRoot = path.resolve(
    opts?.appRoot || process.env.GROKHUB_HOME || process.cwd(),
  );
  // Prefer user install when cwd is $HOME or root lacks UI
  try {
    const home = process.env.HOME || "";
    const ui = path.join(appRoot, ".output", "server", "index.mjs");
    if (!existsSync(ui) || (home && appRoot === path.resolve(home))) {
      for (const cand of [
        process.env.GROKHUB_HOME,
        home && path.join(home, ".local/lib/grokhub"),
        home && path.join(home, ".local/share/grokhub"),
        "/usr/lib/grokhub",
      ].filter(Boolean) as string[]) {
        if (existsSync(path.join(cand, ".output", "server", "index.mjs"))) {
          appRoot = path.resolve(cand);
          break;
        }
      }
    }
  } catch {
    /* ignore */
  }
  const runtime = process.env.XDG_RUNTIME_DIR || "/tmp";
  const pidfile = path.join(runtime, "grokhub", "ui.pid");
  const lockfile = path.join(runtime, "grokhub", "ui.lock");
  const uiEntry = path.join(appRoot, ".output", "server", "index.mjs");
  const userBin = process.env.HOME
    ? path.join(process.env.HOME, ".local", "bin", "grokhub")
    : "";

  // Never fuser -k; never relative node .output (cwd=$HOME bug)
  const script = `
set +e
export HOME="\${HOME:-/tmp}"
export GROKHUB_HOME="${appRoot}"
export GROKHUB_PORT="${port}"
mkdir -p "${runtime}/grokhub"
exec >>/tmp/grokhub-ui-restart.log 2>&1
echo "[restart-ts] $(date -Iseconds) root=${appRoot} entry=${uiEntry}"
sleep 1.2
for f in "${pidfile}" "${lockfile}"; do
  if [ -f "\$f" ]; then
    old=\$(cat "\$f" 2>/dev/null || true)
    if [ -n "\$old" ]; then
      cmd=\$(tr "\0" " " </proc/\$old/cmdline 2>/dev/null || true)
      case "\$cmd" in
        *node*|*ELECTRON_RUN_AS_NODE*)
          case "\$cmd" in
            *.output/server*|*index.mjs*|*grokhub*) kill "\$old" 2>/dev/null || true ;;
          esac
          ;;
      esac
    fi
    rm -f "\$f"
  fi
done
sleep 0.3
cd "${appRoot}" || { echo "FATAL cannot cd ${appRoot}"; exit 1; }
if [ -x "${appRoot}/packaging/aur/grokhub.sh" ]; then
  nohup env HOME="\$HOME" GROKHUB_HOME="${appRoot}" bash "${appRoot}/packaging/aur/grokhub.sh" >/dev/null 2>&1 &
  exit 0
fi
if [ -n "${userBin}" ] && [ -x "${userBin}" ]; then
  nohup env HOME="\$HOME" GROKHUB_HOME="${appRoot}" "${userBin}" >/dev/null 2>&1 &
  exit 0
fi
if [ -f "${appRoot}/desktop/main.mjs" ] && command -v electron >/dev/null 2>&1; then
  if [ -f "${uiEntry}" ]; then
    (
      cd "${appRoot}" || exit 1
      export PORT="${port}" NITRO_PORT="${port}" HOST=127.0.0.1 NITRO_HOST=127.0.0.1 GROKHUB_HOME="${appRoot}"
      nohup node "${uiEntry}" >>/tmp/grokhub-ui-restart.log 2>&1 &
      echo \$! > "${pidfile}"
      echo \$! > "${lockfile}"
    )
    for i in \$(seq 1 40); do
      curl -sf -o /dev/null --max-time 1 "http://127.0.0.1:${port}/" && break
      sleep 0.2
    done
  fi
  nohup env HOME="\$HOME" GROKHUB_HOME="${appRoot}" electron --class=grokhub --name=grokhub "${appRoot}/desktop/main.mjs" >/dev/null 2>&1 &
  exit 0
fi
echo "no launcher found"
exit 1
`.trim();

  const child = spawn("bash", ["-c", script], {
    detached: true,
    stdio: "ignore",
    cwd: appRoot,
    env: {
      ...process.env,
      GROKHUB_HOME: appRoot,
      GROKHUB_PORT: String(port),
    },
  });
  child.unref();
}

/**
 * Install latest from GitHub.
 * Uses tarball only — never git reset --hard (safe for packaged + dev trees).
 */
export async function applyUpdate(opts?: {
  repo?: string;
  branch?: string;
  token?: string;
  force?: boolean;
  /** When true (default in desktop), schedule full app restart after success */
  restart?: boolean;
}): Promise<UpdateResult> {
  const steps: string[] = [];
  const repo = opts?.repo || process.env.GROKHUB_REPO || DEFAULT_REPO;
  const branch = opts?.branch || process.env.GROKHUB_BRANCH || DEFAULT_BRANCH;
  const token =
    opts?.token ||
    process.env.GITHUB_TOKEN ||
    process.env.GH_TOKEN ||
    process.env.GROKHUB_GITHUB_TOKEN ||
    "";
  const shouldRestart = opts?.restart === true;

  const statusBefore = await checkForUpdate({ repo, branch, token: token || undefined });
  if (!statusBefore.updateAvailable && !opts?.force) {
    return {
      ok: true,
      detail: statusBefore.detail || "Already up to date",
      steps: [statusBefore.detail || "Already up to date"],
      newSha: statusBefore.currentSha || undefined,
      newVersion: statusBefore.currentVersion,
      status: statusBefore,
      restarting: false,
    };
  }

  let root =
    process.env.GROKHUB_HOME ||
    (await findInstallRoot()) ||
    path.join(os.homedir(), ".local/share/grokhub");

  if (!process.env.GROKHUB_HOME && (await isAppRoot(process.cwd()))) {
    root = process.cwd();
  }

  if (!(await isAppRoot(root))) {
    await fs.mkdir(root, { recursive: true });
    steps.push(`Created install root ${root}`);
  }

  steps.push(`Install root: ${root}`);
  steps.push(
    `Target: ${repo}@${branch}${statusBefore.remoteSha ? ` (${statusBefore.remoteSha})` : ""}`,
  );

  const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
  const tarball = path.join(tmp, "update.tar.gz");
  const extractDir = path.join(tmp, "extract");

  try {
    steps.push("Downloading GitHub archive…");
    await downloadGithubTarball({
      repo,
      branch,
      token: token || undefined,
      destFile: tarball,
    });
    const st = await fs.stat(tarball);
    steps.push(`Downloaded ${(st.size / 1024 / 1024).toFixed(1)} MB`);

    steps.push("Extracting archive…");
    const extracted = await extractTarball(tarball, extractDir);
    steps.push(`Extracted ${path.basename(extracted)}`);

    const deployed = await deployExtracted(extracted, root, steps);

    // Prefer full remote SHA from API for reliable future checks
    let newSha = statusBefore.remoteSha || deployed.sha;
    if (newSha && newSha.length < 40 && statusBefore.remoteSha) {
      // statusBefore.remoteSha is already short (12); fetch full if we only have short
      try {
        const head = await fetchRemoteHead(repo, branch, token || undefined);
        if (head?.sha) newSha = head.sha;
      } catch {
        /* keep short */
      }
    } else if (!newSha) {
      try {
        const head = await fetchRemoteHead(repo, branch, token || undefined);
        newSha = head?.sha;
      } catch {
        /* ignore */
      }
    }

    let newVersion = deployed.version || readBuiltinVersion();
    try {
      const pkg = JSON.parse(
        await fs.readFile(path.join(root, "package.json"), "utf8"),
      ) as { version?: string };
      if (pkg.version) newVersion = String(pkg.version);
    } catch {
      /* ignore */
    }

    await stampInstall(root, newSha, newVersion, steps);

    // Rebuild only if no .output shipped
    let hasOutput = false;
    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      hasOutput = true;
    } catch {
      hasOutput = false;
    }

    let hasPkg = false;
    try {
      await fs.stat(path.join(root, "package.json"));
      hasPkg = true;
    } catch {
      hasPkg = false;
    }

    if (!hasOutput && hasPkg) {
      steps.push("No prebuilt .output — running npm install + desktop build");
      try {
        await run("npm", ["ci", "--ignore-scripts"], { cwd: root, timeout: 600_000 });
      } catch {
        await run("npm", ["install", "--ignore-scripts"], { cwd: root, timeout: 600_000 });
      }
      await run("npm", ["run", "build"], {
        cwd: root,
        timeout: 600_000,
        env: { ...process.env, GROKHUB_DESKTOP: "1" },
      });
      steps.push("Build finished");
    } else if (hasOutput) {
      steps.push("Using prebuilt .output (no rebuild needed)");
    }

    // Optional system reinstall
    const installScript = path.join(root, "scripts", "install-arch.sh");
    let hasInstallScript = false;
    try {
      await fs.stat(installScript);
      hasInstallScript = true;
    } catch {
      hasInstallScript = false;
    }

    let canRoot = false;
    try {
      canRoot = typeof process.getuid === "function" && process.getuid() === 0;
      if (canRoot) await fs.access("/usr/lib", fs.constants.W_OK);
      else canRoot = false;
    } catch {
      canRoot = false;
    }

    const systemTarget =
      root === "/usr/lib/grokhub" || process.env.GROKHUB_SYSTEM_INSTALL === "1";
    if (hasInstallScript && canRoot && systemTarget) {
      steps.push("Running scripts/install-arch.sh");
      try {
        await run("bash", [installScript], { cwd: root, timeout: 180_000 });
        steps.push("System files updated under /usr/lib/grokhub");
        // stamp again on system path
        await stampInstall("/usr/lib/grokhub", newSha, newVersion, steps);
      } catch (e) {
        steps.push(
          `System reinstall failed (non-fatal): ${
            e instanceof Error ? e.message.slice(0, 300) : "error"
          }`,
        );
      }
    } else if (hasInstallScript && canRoot && !systemTarget) {
      steps.push("Root session but non-system root — skipped install-arch.sh");
    } else {
      steps.push("Runtime files updated in place");
    }

    try {
      await fs.stat(path.join(root, ".output", "server", "index.mjs"));
      steps.push("Verified .output/server/index.mjs");
    } catch {
      throw new Error(
        "Update finished but .output/server/index.mjs is missing — archive may be incomplete",
      );
    }

    // Re-check so UI clears "Update available"
    const statusAfter = await checkForUpdate({ repo, branch, token: token || undefined });
    // Force local stamp into status if check still races
    if (newSha && statusAfter.updateAvailable && shaMatch(newSha, statusAfter.remoteSha)) {
      statusAfter.updateAvailable = false;
      statusAfter.currentSha = newSha.slice(0, 12);
      statusAfter.currentVersion = newVersion;
      statusAfter.detail = `Up to date · v${newVersion} · ${newSha.slice(0, 12)}`;
    }

    let restarting = false;
    if (shouldRestart) {
      steps.push("Restarting GrokHub…");
      scheduleAppRestart({ appRoot: root });
      restarting = true;
    } else {
      steps.push("Done — restart GrokHub to load the new build");
    }

    return {
      ok: true,
      detail: `Updated to v${newVersion} (${(newSha || "latest").slice(0, 12)}) from ${repo}@${branch}`,
      steps,
      newSha: newSha?.slice(0, 12),
      newVersion,
      restarting,
      status: statusAfter,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg.slice(0, 2000), steps, restarting: false };
  } finally {
    await fs.rm(tmp, { recursive: true, force: true }).catch(() => null);
  }
}
