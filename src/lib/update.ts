/**
 * GitHub update helpers — Node only (server / Electron main).
 *
 * Packaged Arch installs live at /usr/lib/grokhub with only `.output` + `desktop`
 * (no .git / package.json). Updates download a GitHub tarball and swap those
 * trees — never `git reset --hard` (that wipes local work).
 */
import { execFile as execFileCb } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import { createWriteStream } from "node:fs";
import path from "node:path";
import os from "node:os";
import { pipeline } from "node:stream/promises";
import { Readable } from "node:stream";

const execFileAsync = promisify(execFileCb);

export const DEFAULT_REPO = "blackviperxiii-ui/Grok-Hub";
export const DEFAULT_BRANCH = "main";
export const APP_VERSION = "0.1";

export type UpdateStatus = {
  currentVersion: string;
  currentSha: string | null;
  remoteSha: string | null;
  remoteMessage: string | null;
  updateAvailable: boolean;
  repo: string;
  branch: string;
  installRoot: string | null;
  detail: string;
};

export type UpdateResult = {
  ok: boolean;
  detail: string;
  steps: string[];
  newSha?: string;
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

async function readLocalVersion(
  root: string | null,
): Promise<{ version: string; sha: string | null }> {
  let version = APP_VERSION;
  let sha: string | null = null;
  if (!root) return { version, sha };
  try {
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8")) as {
      version?: string;
    };
    if (pkg.version) version = String(pkg.version).replace(/\.0$/, "") || version;
  } catch {
    /* packaged installs may lack package.json */
  }
  try {
    const v = await fs.readFile(path.join(root, "VERSION"), "utf8");
    const line = v.trim();
    if (line) sha = line.slice(0, 40);
  } catch {
    /* ignore */
  }
  if (!sha) {
    try {
      const { stdout } = await run("git", ["rev-parse", "HEAD"], { cwd: root, timeout: 8000 });
      sha = stdout.trim().slice(0, 12);
    } catch {
      /* ignore */
    }
  }
  return { version, sha: sha ? sha.slice(0, 12) : null };
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
    } else if (local.sha && remote.sha.slice(0, 12) === local.sha.slice(0, 12)) {
      detail = "Already on latest commit.";
    } else if (!local.sha) {
      detail = "Local VERSION unknown — install recommended.";
    } else {
      detail = "Update available from GitHub.";
    }
  } catch (e) {
    detail = e instanceof Error ? e.message : "Update check failed";
  }

  const remoteShort = remote?.sha.slice(0, 12) ?? null;
  const localShort = local.sha?.slice(0, 12) ?? null;
  const updateAvailable = Boolean(remote && (!localShort || remoteShort !== localShort));

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
): Promise<string | undefined> {
  await replaceDir(path.join(extracted, ".output"), path.join(root, ".output"), steps);
  await replaceDir(path.join(extracted, "desktop"), path.join(root, "desktop"), steps);

  for (const name of ["package.json", "package-lock.json", "scripts", "packaging"]) {
    const src = path.join(extracted, name);
    try {
      const st = await fs.stat(src);
      if (st.isDirectory()) {
        await replaceDir(src, path.join(root, name), steps);
      } else if (st.isFile()) {
        await fs.copyFile(src, path.join(root, name));
        steps.push(`Updated ${name}`);
      }
    } catch {
      /* optional */
    }
  }

  // GitHub tarball folder ends with the commit short/long sha — most reliable
  const base = path.basename(extracted);
  const m = base.match(/-([0-9a-f]{7,40})$/i);
  if (m?.[1]) return m[1].slice(0, 12);
  try {
    const v = (await fs.readFile(path.join(extracted, "VERSION"), "utf8")).trim();
    if (v) return v.slice(0, 40);
  } catch {
    /* ignore */
  }
  return undefined;
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

  const status = await checkForUpdate({ repo, branch, token: token || undefined });
  if (!status.updateAvailable && !opts?.force) {
    steps.push(status.detail || "Already up to date");
  }

  // Prefer GROKHUB_HOME / packaged path; avoid using a random cwd when possible
  let root =
    process.env.GROKHUB_HOME ||
    (await findInstallRoot()) ||
    path.join(os.homedir(), ".local/share/grokhub");

  // If GROKHUB_HOME not set and cwd looks like the app, use cwd (dev)
  if (!process.env.GROKHUB_HOME && (await isAppRoot(process.cwd()))) {
    root = process.cwd();
  }

  if (!(await isAppRoot(root))) {
    await fs.mkdir(root, { recursive: true });
    steps.push(`Created install root ${root}`);
  }

  steps.push(`Install root: ${root}`);
  steps.push(`Target: ${repo}@${branch}${status.remoteSha ? ` (${status.remoteSha})` : ""}`);

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

    const extractedSha = await deployExtracted(extracted, root, steps);

    let newSha = status.remoteSha || extractedSha || undefined;
    if (!newSha) {
      // Parse short sha from GitHub folder name: owner-repo-<sha>
      const m = path.basename(extracted).match(/-([0-9a-f]{7,40})$/i);
      if (m?.[1]) newSha = m[1].slice(0, 12);
    }
    if (!newSha) {
      try {
        const head = await fetchRemoteHead(repo, branch, token || undefined);
        newSha = head?.sha.slice(0, 12);
      } catch {
        /* ignore */
      }
    }
    if (newSha) {
      await fs.writeFile(path.join(root, "VERSION"), `${newSha}\n`);
      steps.push(`VERSION → ${newSha}`);
    } else {
      steps.push("Warning: could not determine remote SHA for VERSION stamp");
    }

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
      // Only treat as root when we are actually uid 0 — write-access alone can
      // be misleading in some containers and we must not half-run install-arch.
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
      } catch (e) {
        steps.push(
          `System reinstall failed (non-fatal): ${
            e instanceof Error ? e.message.slice(0, 300) : "error"
          }`,
        );
      }
    } else if (hasInstallScript && canRoot && !systemTarget) {
      steps.push("Root session but non-system root — skipped install-arch.sh");
    } else if (hasInstallScript && !canRoot) {
      const userLib = path.join(os.homedir(), ".local/share/grokhub");
      if (path.resolve(root) !== path.resolve(userLib) && root !== "/usr/lib/grokhub") {
        // When updating a clone, also refresh user-local runtime if it exists
        try {
          await fs.stat(path.join(userLib, ".output"));
          steps.push(`Syncing runtime → ${userLib}`);
          await replaceDir(path.join(root, ".output"), path.join(userLib, ".output"), steps);
          await replaceDir(path.join(root, "desktop"), path.join(userLib, "desktop"), steps);
          if (newSha) await fs.writeFile(path.join(userLib, "VERSION"), `${newSha}\n`);
        } catch {
          /* no user lib */
        }
      }
      steps.push("No root — skipped /usr reinstall. Restart GrokHub to load the new build.");
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

    steps.push("Done — fully quit and relaunch GrokHub");
    return {
      ok: true,
      detail: `Updated to ${newSha || "latest"} from ${repo}@${branch}`,
      steps,
      newSha,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg.slice(0, 2000), steps };
  } finally {
    await fs.rm(tmp, { recursive: true, force: true }).catch(() => null);
  }
}
