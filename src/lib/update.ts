/**
 * GitHub update helpers — Node only (server / Electron main).
 * Pulls latest from the GrokHub repo and reinstalls into /usr/lib/grokhub when root.
 */
import { exec as execCb } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";

const execAsync = promisify(execCb);

export const DEFAULT_REPO = "blackviperxiii-ui/spring-dove-reef-apple";
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

function installRoots(): string[] {
  return [
    process.env.GROKHUB_HOME || "",
    "/usr/lib/grokhub",
    path.join(os.homedir(), ".local/share/grokhub"),
    path.resolve(process.cwd()),
  ].filter(Boolean);
}

async function findInstallRoot(): Promise<string | null> {
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

async function readLocalVersion(root: string | null): Promise<{ version: string; sha: string | null }> {
  let version = APP_VERSION;
  let sha: string | null = null;
  if (!root) return { version, sha };
  try {
    const pkg = JSON.parse(await fs.readFile(path.join(root, "package.json"), "utf8")) as {
      version?: string;
    };
    if (pkg.version) version = pkg.version.replace(/\.0$/, "") || version;
  } catch {
    /* ignore */
  }
  try {
    const { stdout } = await execAsync("git rev-parse HEAD", { cwd: root, timeout: 8000 });
    sha = stdout.trim().slice(0, 12);
  } catch {
    try {
      const v = await fs.readFile(path.join(root, "VERSION"), "utf8");
      sha = v.trim().slice(0, 40) || null;
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
        ? "Could not read remote commit (check repo access)."
        : "Could not read remote commit (private repo may need a GitHub token).";
    } else if (local.sha && remote.sha.startsWith(local.sha)) {
      detail = "Already on latest commit.";
    } else if (local.sha && remote.sha.slice(0, 12) === local.sha.slice(0, 12)) {
      detail = "Already on latest commit.";
    } else if (!local.sha) {
      detail = "Local SHA unknown — update recommended.";
    } else {
      detail = "Update available from GitHub.";
    }
  } catch (e) {
    detail = e instanceof Error ? e.message : "Update check failed";
  }

  const remoteShort = remote?.sha.slice(0, 12) ?? null;
  const localShort = local.sha?.slice(0, 12) ?? null;
  const updateAvailable = Boolean(
    remote && (!localShort || remoteShort !== localShort),
  );

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

export async function applyUpdate(opts?: {
  repo?: string;
  branch?: string;
  token?: string;
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

  const root = (await findInstallRoot()) || process.cwd();
  steps.push(`Install root: ${root}`);

  const gitDir = path.join(root, ".git");
  let hasGit = false;
  try {
    await fs.stat(gitDir);
    hasGit = true;
  } catch {
    hasGit = false;
  }

  try {
    if (hasGit) {
      steps.push("git fetch + reset --hard origin/" + branch);
      if (token) {
        // Use token only for https remotes when needed
        await execAsync(`git -C "${root}" remote get-url origin`, { timeout: 10000 }).catch(
          () => null,
        );
      }
      await execAsync(`git -C "${root}" fetch origin ${branch}`, {
        timeout: 180000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
      await execAsync(`git -C "${root}" reset --hard origin/${branch}`, {
        timeout: 60000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
    } else {
      steps.push("No .git — cloning fresh into temp and syncing");
      const tmp = await fs.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
      const cloneUrl = token
        ? `https://x-access-token:${token}@github.com/${repo}.git`
        : `https://github.com/${repo}.git`;
      await execAsync(`git clone --depth 1 --branch ${branch} "${cloneUrl}" "${tmp}/src"`, {
        timeout: 300000,
        env: { ...process.env, GIT_TERMINAL_PROMPT: "0" },
      });
      // Copy source over (preserve local .env)
      await execAsync(
        `rsync -a --delete --exclude node_modules --exclude .env --exclude .env.local "${tmp}/src/" "${root}/"`,
        { timeout: 120000 },
      );
      await fs.rm(tmp, { recursive: true, force: true });
    }

    steps.push("npm install (ignore-scripts) + desktop build");
    await execAsync("npm ci --ignore-scripts || npm install --ignore-scripts", {
      cwd: root,
      timeout: 600000,
      env: { ...process.env, GROKHUB_DESKTOP: "1" },
    });
    await execAsync("GROKHUB_DESKTOP=1 npm run build", {
      cwd: root,
      timeout: 600000,
      env: { ...process.env, GROKHUB_DESKTOP: "1" },
    });

    // Reinstall system files if we can write to /usr/lib/grokhub
    try {
      await fs.access("/usr/bin", fs.constants.W_OK);
      steps.push("Reinstalling system launcher → /usr/lib/grokhub");
      await execAsync(`bash "${root}/scripts/install-arch.sh"`, {
        cwd: root,
        timeout: 120000,
      });
    } catch {
      steps.push("Skipped system reinstall (no write access) — restart from this tree");
      // Still refresh local VERSION stamp
      try {
        const { stdout } = await execAsync("git rev-parse HEAD", { cwd: root });
        await fs.writeFile(path.join(root, "VERSION"), stdout.trim() + "\n");
      } catch {
        /* ignore */
      }
    }

    let newSha: string | undefined;
    try {
      const { stdout } = await execAsync("git rev-parse HEAD", { cwd: root });
      newSha = stdout.trim().slice(0, 12);
    } catch {
      /* ignore */
    }

    steps.push("Done — restart GrokHub to load the new build");
    return {
      ok: true,
      detail: `Updated to ${newSha || "latest"} from ${repo}@${branch}`,
      steps,
      newSha,
    };
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    steps.push(`Failed: ${msg}`);
    return { ok: false, detail: msg, steps };
  }
}
