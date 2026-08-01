import { t as APP_VERSION } from "./version-Od4YDoyU.mjs";
import { XAI_BASE, callXaiChat, callXaiChatStream, callXaiImagine, probeXaiBearer } from "./grok-BHPoOd67.mjs";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { createWriteStream, existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { execFile, spawn } from "node:child_process";
import { promisify } from "node:util";
import fs$1 from "node:fs/promises";
import os from "node:os";
//#region node_modules/.nitro/vite/services/ssr/assets/api-handlers-UsMyVIWh.js
/**
* GitHub update helpers — Node only (server / Electron main).
*
* Packaged Arch installs live at /usr/lib/grokhub with only `.output` + `desktop`
* (no .git / package.json). Updates download a GitHub tarball and swap those
* trees — never `git reset --hard` (that wipes local work).
*/
var execFileAsync = promisify(execFile);
async function run(cmd, args, opts = {}) {
	try {
		const { stdout, stderr } = await execFileAsync(cmd, args, {
			cwd: opts.cwd,
			timeout: opts.timeout ?? 12e4,
			env: {
				...process.env,
				...opts.env,
				GIT_TERMINAL_PROMPT: "0"
			},
			maxBuffer: 20 * 1024 * 1024
		});
		return {
			stdout: String(stdout || ""),
			stderr: String(stderr || "")
		};
	} catch (e) {
		const err = e;
		const stderr = String(err.stderr || "");
		const stdout = String(err.stdout || "");
		const msg = [
			err.message,
			stderr,
			stdout
		].filter(Boolean).join("\n").slice(0, 4e3);
		throw new Error(msg || `Command failed: ${cmd} ${args.join(" ")}`);
	}
}
/** Compare git SHAs allowing 7–40 char prefixes. */
function shaMatch(a, b) {
	if (!a || !b) return false;
	const x = a.trim().toLowerCase();
	const y = b.trim().toLowerCase();
	if (!x || !y) return false;
	const n = Math.min(x.length, y.length);
	if (n < 7) return x === y;
	return x.slice(0, n) === y.slice(0, n);
}
function installRoots() {
	return [
		process.env.GROKHUB_HOME || "",
		"/usr/lib/grokhub",
		path.join(os.homedir(), ".local/share/grokhub"),
		path.resolve(process.cwd())
	].filter(Boolean);
}
async function isAppRoot(root) {
	try {
		await fs$1.stat(path.join(root, ".output", "server", "index.mjs"));
		return true;
	} catch {}
	try {
		await fs$1.stat(path.join(root, "package.json"));
		const pkg = JSON.parse(await fs$1.readFile(path.join(root, "package.json"), "utf8"));
		return pkg.name === "grokhub" || pkg.name === "GrokHub";
	} catch {
		return false;
	}
}
async function findInstallRoot() {
	for (const root of installRoots()) if (await isAppRoot(root)) return root;
	return null;
}
function readBuiltinVersion() {
	const candidates = [
		path.join(process.cwd(), "package.json"),
		path.join(process.env.GROKHUB_HOME || "", "package.json"),
		path.join("/usr/lib/grokhub", "package.json")
	];
	for (const f of candidates) try {
		if (!f || !existsSync(f)) continue;
		const pkg = JSON.parse(readFileSync(f, "utf8"));
		if (pkg.version && (pkg.name === "grokhub" || !pkg.name)) return String(pkg.version);
		if (pkg.version) return String(pkg.version);
	} catch {}
	return APP_VERSION;
}
async function readLocalVersion(root) {
	let version = readBuiltinVersion();
	let sha = null;
	if (!root) return {
		version,
		sha
	};
	try {
		const pkg = JSON.parse(await fs$1.readFile(path.join(root, "package.json"), "utf8"));
		if (pkg.version) version = String(pkg.version);
	} catch {}
	try {
		const v = (await fs$1.readFile(path.join(root, "APP_VERSION"), "utf8")).trim();
		if (v) version = v;
	} catch {}
	try {
		const v = (await fs$1.readFile(path.join(root, "VERSION"), "utf8")).trim();
		if (v) sha = v.split(/\s+/)[0] || null;
	} catch {}
	if (!sha) try {
		const { stdout } = await run("git", ["rev-parse", "HEAD"], {
			cwd: root,
			timeout: 8e3
		});
		sha = stdout.trim() || null;
	} catch {}
	return {
		version,
		sha
	};
}
async function fetchRemoteHead(repo, branch, token) {
	const headers = {
		accept: "application/vnd.github+json",
		"user-agent": "GrokHub-Updater"
	};
	if (token) headers.authorization = `Bearer ${token}`;
	const url = `https://api.github.com/repos/${repo}/commits/${encodeURIComponent(branch)}`;
	const res = await fetch(url, { headers });
	if (!res.ok) return null;
	const data = await res.json();
	if (!data.sha) return null;
	return {
		sha: data.sha,
		message: (data.commit?.message || "").split("\n")[0] || ""
	};
}
async function checkForUpdate(opts) {
	const repo = opts?.repo || process.env.GROKHUB_REPO || "blackviperxiii-ui/Grok-Hub";
	const branch = opts?.branch || process.env.GROKHUB_BRANCH || "main";
	const token = opts?.token || process.env.GITHUB_TOKEN || process.env.GH_TOKEN || process.env.GROKHUB_GITHUB_TOKEN || "";
	const installRoot = await findInstallRoot();
	const local = await readLocalVersion(installRoot);
	let remote = null;
	let detail = "";
	try {
		remote = await fetchRemoteHead(repo, branch, token || void 0);
		if (!remote) detail = token ? "Could not read remote commit (check repo access / token scopes)." : "Could not read remote commit (private repo needs a GitHub token).";
		else if (shaMatch(local.sha, remote.sha)) detail = `Up to date · v${local.version} · ${local.sha?.slice(0, 12) || "local"}`;
		else if (!local.sha) detail = "Local VERSION unknown — install recommended.";
		else detail = `Update available · ${local.sha.slice(0, 12)} → ${remote.sha.slice(0, 12)}`;
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
		detail
	};
}
async function downloadGithubTarball(opts) {
	const headers = {
		accept: "application/vnd.github+json",
		"user-agent": "GrokHub-Updater"
	};
	if (opts.token) headers.authorization = `Bearer ${opts.token}`;
	const urls = [`https://api.github.com/repos/${opts.repo}/tarball/${encodeURIComponent(opts.branch)}`, `https://codeload.github.com/${opts.repo}/tar.gz/refs/heads/${encodeURIComponent(opts.branch)}`];
	let lastErr = "download failed";
	for (const url of urls) try {
		const res = await fetch(url, {
			headers: url.includes("api.github.com") ? headers : opts.token ? {
				...headers,
				authorization: `Bearer ${opts.token}`
			} : { "user-agent": "GrokHub-Updater" },
			redirect: "follow"
		});
		if (!res.ok || !res.body) {
			lastErr = `HTTP ${res.status} from ${url}`;
			continue;
		}
		await pipeline(Readable.fromWeb(res.body), createWriteStream(opts.destFile));
		if ((await fs$1.stat(opts.destFile)).size < 1e3) {
			lastErr = "Downloaded archive too small";
			continue;
		}
		return;
	} catch (e) {
		lastErr = e instanceof Error ? e.message : String(e);
	}
	throw new Error(`Could not download update archive: ${lastErr}`);
}
async function extractTarball(tarball, destDir) {
	await fs$1.mkdir(destDir, { recursive: true });
	await run("tar", [
		"-xzf",
		tarball,
		"-C",
		destDir
	], { timeout: 12e4 });
	const entries = await fs$1.readdir(destDir);
	if (entries.length === 1) {
		const only = path.join(destDir, entries[0]);
		if ((await fs$1.stat(only)).isDirectory()) return only;
	}
	return destDir;
}
async function copyTree(src, dest) {
	await fs$1.mkdir(dest, { recursive: true });
	try {
		await run("cp", [
			"-a",
			`${src}/.`,
			dest
		], { timeout: 12e4 });
		return;
	} catch {}
	await fs$1.cp(src, dest, {
		recursive: true,
		force: true
	});
}
async function replaceDir(src, dest, steps) {
	try {
		await fs$1.stat(src);
	} catch {
		steps.push(`Skip missing ${path.basename(src)}`);
		return;
	}
	const backup = `${dest}.bak-${Date.now()}`;
	let hadDest = false;
	try {
		await fs$1.stat(dest);
		hadDest = true;
		await fs$1.rename(dest, backup);
	} catch {
		hadDest = false;
	}
	try {
		await copyTree(src, dest);
		if (hadDest) await fs$1.rm(backup, {
			recursive: true,
			force: true
		}).catch(() => null);
		steps.push(`Updated ${path.basename(dest)}`);
	} catch (e) {
		if (hadDest) {
			await fs$1.rm(dest, {
				recursive: true,
				force: true
			}).catch(() => null);
			await fs$1.rename(backup, dest).catch(() => null);
		}
		throw e;
	}
}
async function deployExtracted(extracted, root, steps) {
	await replaceDir(path.join(extracted, ".output"), path.join(root, ".output"), steps);
	await replaceDir(path.join(extracted, "desktop"), path.join(root, "desktop"), steps);
	let version;
	for (const name of [
		"package.json",
		"package-lock.json",
		"scripts",
		"packaging"
	]) {
		const src = path.join(extracted, name);
		try {
			const st = await fs$1.stat(src);
			if (st.isDirectory()) await replaceDir(src, path.join(root, name), steps);
			else if (st.isFile()) {
				await fs$1.copyFile(src, path.join(root, name));
				steps.push(`Updated ${name}`);
				if (name === "package.json") try {
					const pkg = JSON.parse(await fs$1.readFile(src, "utf8"));
					if (pkg.version) version = String(pkg.version);
				} catch {}
			}
		} catch {}
	}
	let sha;
	const m = path.basename(extracted).match(/-([0-9a-f]{7,40})$/i);
	if (m?.[1]) sha = m[1];
	try {
		const v = (await fs$1.readFile(path.join(extracted, "VERSION"), "utf8")).trim();
		if (v) sha = v.split(/\s+/)[0] || sha;
	} catch {}
	return {
		sha,
		version
	};
}
async function stampInstall(root, sha, version, steps) {
	if (sha) {
		await fs$1.writeFile(path.join(root, "VERSION"), `${sha}\n`);
		steps.push(`VERSION → ${sha.slice(0, 12)}`);
	}
	const ver = version || readBuiltinVersion();
	await fs$1.writeFile(path.join(root, "APP_VERSION"), `${ver}\n`);
	steps.push(`APP_VERSION → ${ver}`);
}
/**
* Schedule a full app restart (UI server + Electron) after a successful update.
* Spawns a detached helper so the current process can exit cleanly.
*/
function scheduleAppRestart(opts) {
	const port = opts?.port || process.env.GROKHUB_PORT || "18765";
	const appRoot = opts?.appRoot || process.env.GROKHUB_HOME || path.resolve(process.cwd());
	const runtime = process.env.XDG_RUNTIME_DIR || "/tmp";
	const pidfile = path.join(runtime, "grokhub", "ui.pid");
	spawn("bash", ["-c", `
set +e
sleep 1.2
# Stop old UI server
if [ -f "${pidfile}" ]; then
  kill "$(cat "${pidfile}")" 2>/dev/null || true
  rm -f "${pidfile}"
fi
fuser -k ${port}/tcp >/dev/null 2>&1 || true
# Prefer system launcher
if command -v grokhub >/dev/null 2>&1; then
  nohup grokhub >/dev/null 2>&1 &
  exit 0
fi
# Fallback: run packaged launcher or electron main
export GROKHUB_HOME="${appRoot}"
export GROKHUB_PORT="${port}"
if [ -x "${appRoot}/packaging/aur/grokhub.sh" ]; then
  nohup bash "${appRoot}/packaging/aur/grokhub.sh" >/dev/null 2>&1 &
elif [ -f "${appRoot}/desktop/main.mjs" ] && command -v electron >/dev/null 2>&1; then
  # start UI if present
  if [ -f "${appRoot}/.output/server/index.mjs" ]; then
    (
      cd "${appRoot}"
      export PORT="${port}" NITRO_PORT="${port}" HOST=127.0.0.1 NITRO_HOST=127.0.0.1
      nohup node .output/server/index.mjs >/tmp/grokhub-ui-restart.log 2>&1 &
      echo $! > "${pidfile}"
    )
    sleep 0.8
  fi
  nohup electron --class=GrokHub --name=GrokHub "${appRoot}/desktop/main.mjs" >/dev/null 2>&1 &
fi
`.trim()], {
		detached: true,
		stdio: "ignore",
		env: process.env
	}).unref();
}
/**
* Install latest from GitHub.
* Uses tarball only — never git reset --hard (safe for packaged + dev trees).
*/
async function applyUpdate(opts) {
	const steps = [];
	const repo = opts?.repo || process.env.GROKHUB_REPO || "blackviperxiii-ui/Grok-Hub";
	const branch = opts?.branch || process.env.GROKHUB_BRANCH || "main";
	const token = opts?.token || process.env.GITHUB_TOKEN || process.env.GH_TOKEN || process.env.GROKHUB_GITHUB_TOKEN || "";
	const shouldRestart = opts?.restart === true;
	const statusBefore = await checkForUpdate({
		repo,
		branch,
		token: token || void 0
	});
	if (!statusBefore.updateAvailable && !opts?.force) return {
		ok: true,
		detail: statusBefore.detail || "Already up to date",
		steps: [statusBefore.detail || "Already up to date"],
		newSha: statusBefore.currentSha || void 0,
		newVersion: statusBefore.currentVersion,
		status: statusBefore,
		restarting: false
	};
	let root = process.env.GROKHUB_HOME || await findInstallRoot() || path.join(os.homedir(), ".local/share/grokhub");
	if (!process.env.GROKHUB_HOME && await isAppRoot(process.cwd())) root = process.cwd();
	if (!await isAppRoot(root)) {
		await fs$1.mkdir(root, { recursive: true });
		steps.push(`Created install root ${root}`);
	}
	steps.push(`Install root: ${root}`);
	steps.push(`Target: ${repo}@${branch}${statusBefore.remoteSha ? ` (${statusBefore.remoteSha})` : ""}`);
	const tmp = await fs$1.mkdtemp(path.join(os.tmpdir(), "grokhub-up-"));
	const tarball = path.join(tmp, "update.tar.gz");
	const extractDir = path.join(tmp, "extract");
	try {
		steps.push("Downloading GitHub archive…");
		await downloadGithubTarball({
			repo,
			branch,
			token: token || void 0,
			destFile: tarball
		});
		const st = await fs$1.stat(tarball);
		steps.push(`Downloaded ${(st.size / 1024 / 1024).toFixed(1)} MB`);
		steps.push("Extracting archive…");
		const extracted = await extractTarball(tarball, extractDir);
		steps.push(`Extracted ${path.basename(extracted)}`);
		const deployed = await deployExtracted(extracted, root, steps);
		let newSha = statusBefore.remoteSha || deployed.sha;
		if (newSha && newSha.length < 40 && statusBefore.remoteSha) try {
			const head = await fetchRemoteHead(repo, branch, token || void 0);
			if (head?.sha) newSha = head.sha;
		} catch {}
		else if (!newSha) try {
			newSha = (await fetchRemoteHead(repo, branch, token || void 0))?.sha;
		} catch {}
		let newVersion = deployed.version || readBuiltinVersion();
		try {
			const pkg = JSON.parse(await fs$1.readFile(path.join(root, "package.json"), "utf8"));
			if (pkg.version) newVersion = String(pkg.version);
		} catch {}
		await stampInstall(root, newSha, newVersion, steps);
		let hasOutput = false;
		try {
			await fs$1.stat(path.join(root, ".output", "server", "index.mjs"));
			hasOutput = true;
		} catch {
			hasOutput = false;
		}
		let hasPkg = false;
		try {
			await fs$1.stat(path.join(root, "package.json"));
			hasPkg = true;
		} catch {
			hasPkg = false;
		}
		if (!hasOutput && hasPkg) {
			steps.push("No prebuilt .output — running npm install + desktop build");
			try {
				await run("npm", ["ci", "--ignore-scripts"], {
					cwd: root,
					timeout: 6e5
				});
			} catch {
				await run("npm", ["install", "--ignore-scripts"], {
					cwd: root,
					timeout: 6e5
				});
			}
			await run("npm", ["run", "build"], {
				cwd: root,
				timeout: 6e5,
				env: {
					...process.env,
					GROKHUB_DESKTOP: "1"
				}
			});
			steps.push("Build finished");
		} else if (hasOutput) steps.push("Using prebuilt .output (no rebuild needed)");
		const installScript = path.join(root, "scripts", "install-arch.sh");
		let hasInstallScript = false;
		try {
			await fs$1.stat(installScript);
			hasInstallScript = true;
		} catch {
			hasInstallScript = false;
		}
		let canRoot = false;
		try {
			canRoot = typeof process.getuid === "function" && process.getuid() === 0;
			if (canRoot) await fs$1.access("/usr/lib", fs$1.constants.W_OK);
			else canRoot = false;
		} catch {
			canRoot = false;
		}
		const systemTarget = root === "/usr/lib/grokhub" || process.env.GROKHUB_SYSTEM_INSTALL === "1";
		if (hasInstallScript && canRoot && systemTarget) {
			steps.push("Running scripts/install-arch.sh");
			try {
				await run("bash", [installScript], {
					cwd: root,
					timeout: 18e4
				});
				steps.push("System files updated under /usr/lib/grokhub");
				await stampInstall("/usr/lib/grokhub", newSha, newVersion, steps);
			} catch (e) {
				steps.push(`System reinstall failed (non-fatal): ${e instanceof Error ? e.message.slice(0, 300) : "error"}`);
			}
		} else if (hasInstallScript && canRoot && !systemTarget) steps.push("Root session but non-system root — skipped install-arch.sh");
		else steps.push("Runtime files updated in place");
		try {
			await fs$1.stat(path.join(root, ".output", "server", "index.mjs"));
			steps.push("Verified .output/server/index.mjs");
		} catch {
			throw new Error("Update finished but .output/server/index.mjs is missing — archive may be incomplete");
		}
		const statusAfter = await checkForUpdate({
			repo,
			branch,
			token: token || void 0
		});
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
		} else steps.push("Done — restart GrokHub to load the new build");
		return {
			ok: true,
			detail: `Updated to v${newVersion} (${(newSha || "latest").slice(0, 12)}) from ${repo}@${branch}`,
			steps,
			newSha: newSha?.slice(0, 12),
			newVersion,
			restarting,
			status: statusAfter
		};
	} catch (e) {
		const msg = e instanceof Error ? e.message : String(e);
		steps.push(`Failed: ${msg}`);
		return {
			ok: false,
			detail: msg.slice(0, 2e3),
			steps,
			restarting: false
		};
	} finally {
		await fs$1.rm(tmp, {
			recursive: true,
			force: true
		}).catch(() => null);
	}
}
/**
* xAI Grok OAuth (device-code) — same public client used by Grok CLI / OpenClaw.
* SuperGrok or X Premium+ accounts get API access tokens without a console API key.
* Node-only (server / Electron main).
*/
var XAI_OAUTH_CLIENT_ID = "b1a00492-073a-47ea-816f-4c329264a828";
var XAI_OAUTH_SCOPE = "openid profile email offline_access grok-cli:access api:access";
var XAI_OAUTH_ISSUER = "https://auth.x.ai";
var XAI_OAUTH_DISCOVERY = `${XAI_OAUTH_ISSUER}/.well-known/openid-configuration`;
var XAI_DEVICE_CODE_GRANT = "urn:ietf:params:oauth:grant-type:device_code";
var XAI_UA = "GrokHub/0.2.6 (xAI OAuth; Linux)";
function formBody(data) {
	return new URLSearchParams(data).toString();
}
function trustedXai(url) {
	const u = new URL(url);
	if (u.protocol !== "https:") throw new Error("xAI OAuth requires https");
	if (u.hostname !== "x.ai" && !u.hostname.endsWith(".x.ai")) throw new Error(`Untrusted xAI host: ${u.hostname}`);
	return url;
}
function decodeJwtPayload(token) {
	try {
		const part = token.split(".")[1];
		if (!part) return {};
		const json = Buffer.from(part, "base64url").toString("utf8");
		return JSON.parse(json);
	} catch {
		return {};
	}
}
function pickPicture(...candidates) {
	for (const c of candidates) if (typeof c === "string" && /^https?:\/\//i.test(c.trim())) return c.trim();
}
function pickName(...candidates) {
	for (const c of candidates) if (typeof c === "string" && c.trim()) return c.trim();
}
async function discovery() {
	const res = await fetch(XAI_OAUTH_DISCOVERY, { headers: {
		accept: "application/json",
		"user-agent": XAI_UA
	} });
	if (!res.ok) throw new Error(`xAI OIDC discovery failed (${res.status})`);
	const j = await res.json();
	const device = j.device_authorization_endpoint;
	const token = j.token_endpoint;
	const userinfo = j.userinfo_endpoint;
	if (typeof device !== "string" || typeof token !== "string") throw new Error("xAI discovery missing device/token endpoints");
	return {
		deviceAuthorizationEndpoint: trustedXai(device),
		tokenEndpoint: trustedXai(token),
		userinfoEndpoint: typeof userinfo === "string" ? trustedXai(userinfo) : `${XAI_OAUTH_ISSUER}/oauth2/userinfo`
	};
}
function parseTokens(json) {
	const accessToken = json.access_token;
	if (typeof accessToken !== "string" || !accessToken) throw new Error("Token response missing access_token");
	const refreshToken = typeof json.refresh_token === "string" && json.refresh_token ? json.refresh_token : void 0;
	const idToken = typeof json.id_token === "string" && json.id_token ? json.id_token : void 0;
	let expiresAt;
	if (typeof json.expires_in === "number") expiresAt = Date.now() + json.expires_in * 1e3;
	else if (typeof json.expires_in === "string" && /^\d+$/.test(json.expires_in)) expiresAt = Date.now() + Number(json.expires_in) * 1e3;
	return {
		accessToken,
		refreshToken,
		idToken,
		expiresAt
	};
}
async function startXaiDeviceCode() {
	const d = await discovery();
	const res = await fetch(d.deviceAuthorizationEndpoint, {
		method: "POST",
		headers: {
			"content-type": "application/x-www-form-urlencoded",
			accept: "application/json",
			"user-agent": XAI_UA
		},
		body: formBody({
			client_id: XAI_OAUTH_CLIENT_ID,
			scope: XAI_OAUTH_SCOPE
		})
	});
	const json = await res.json().catch(() => ({}));
	if (!res.ok) {
		const msg = typeof json.error_description === "string" && json.error_description || typeof json.error === "string" && json.error || `device code failed (${res.status})`;
		throw new Error(msg);
	}
	const deviceCode = String(json.device_code || "");
	const userCode = String(json.user_code || "");
	const verificationUri = String(json.verification_uri || "");
	if (!deviceCode || !userCode || !verificationUri) throw new Error("Invalid device code response from xAI");
	trustedXai(verificationUri);
	return {
		deviceCode,
		userCode,
		verificationUri,
		verificationUriComplete: typeof json.verification_uri_complete === "string" ? trustedXai(json.verification_uri_complete) : void 0,
		expiresIn: typeof json.expires_in === "number" ? json.expires_in : 1800,
		interval: typeof json.interval === "number" ? json.interval : 5
	};
}
async function pollXaiDeviceCode(deviceCode) {
	const d = await discovery();
	const res = await fetch(d.tokenEndpoint, {
		method: "POST",
		headers: {
			"content-type": "application/x-www-form-urlencoded",
			accept: "application/json",
			"user-agent": XAI_UA
		},
		body: formBody({
			grant_type: XAI_DEVICE_CODE_GRANT,
			client_id: XAI_OAUTH_CLIENT_ID,
			device_code: deviceCode
		})
	});
	const json = await res.json().catch(() => ({}));
	if (res.ok && typeof json.access_token === "string") {
		const base = parseTokens(json);
		let email;
		let name;
		let picture;
		if (base.idToken) {
			const claims = decodeJwtPayload(base.idToken);
			email = pickName(claims.email) || email;
			name = pickName(claims.name, claims.preferred_username, claims.given_name) || name;
			picture = pickPicture(claims.picture, claims.avatar_url, claims.profile_image_url);
		}
		try {
			const ui = await fetchUserinfo(base.accessToken, d.userinfoEndpoint);
			email = ui.email || email;
			name = ui.name || name;
			picture = ui.picture || picture;
		} catch {}
		return {
			status: "ready",
			tokens: {
				...base,
				email,
				name,
				picture,
				connectedAt: Date.now()
			}
		};
	}
	const err = typeof json.error === "string" ? json.error : "unknown";
	if (err === "authorization_pending") return {
		status: "pending",
		error: err
	};
	if (err === "slow_down") return { status: "slow_down" };
	if (err === "expired_token" || err === "access_denied") return {
		status: err === "expired_token" ? "expired" : "denied",
		error: typeof json.error_description === "string" && json.error_description || err
	};
	return {
		status: "pending",
		error: typeof json.error_description === "string" && json.error_description || `waiting (${res.status})`
	};
}
async function fetchUserinfo(accessToken, endpoint) {
	const res = await fetch(endpoint, { headers: {
		authorization: `Bearer ${accessToken}`,
		accept: "application/json",
		"user-agent": XAI_UA
	} });
	if (!res.ok) return {};
	const j = await res.json();
	return {
		email: pickName(j.email),
		name: pickName(j.name, j.preferred_username, j.given_name),
		picture: pickPicture(j.picture, j.avatar_url, j.profile_image_url, j.image)
	};
}
async function refreshXaiOAuth(refreshToken) {
	const d = await discovery();
	const res = await fetch(d.tokenEndpoint, {
		method: "POST",
		headers: {
			"content-type": "application/x-www-form-urlencoded",
			accept: "application/json",
			"user-agent": XAI_UA
		},
		body: formBody({
			grant_type: "refresh_token",
			client_id: XAI_OAUTH_CLIENT_ID,
			refresh_token: refreshToken
		})
	});
	const text = await res.text();
	if (!res.ok) {
		if (/cloudflare|<!doctype html/i.test(text)) throw new Error("xAI blocked token refresh (Cloudflare). Re-run Grok OAuth sign-in.");
		let msg = `refresh failed (${res.status})`;
		try {
			const j = JSON.parse(text);
			msg = j.error_description || j.error || msg;
		} catch {}
		throw new Error(msg);
	}
	return parseTokens(JSON.parse(text));
}
/** Resolve a usable access token, refreshing if near expiry. */
async function ensureAccessToken(tokens) {
	if (!tokens?.accessToken) throw new Error("No OAuth access token — connect Grok OAuth in Settings");
	if (!(typeof tokens.expiresAt === "number" && tokens.expiresAt - 6e4 < Date.now())) return {
		accessToken: tokens.accessToken,
		tokens,
		refreshed: false
	};
	if (!tokens.refreshToken) throw new Error("Grok OAuth session expired — sign in again");
	const next = await refreshXaiOAuth(tokens.refreshToken);
	const merged = {
		...tokens,
		accessToken: next.accessToken,
		refreshToken: next.refreshToken || tokens.refreshToken,
		expiresAt: next.expiresAt,
		idToken: next.idToken || tokens.idToken
	};
	return {
		accessToken: merged.accessToken,
		tokens: merged,
		refreshed: true
	};
}
/**
* Unified JSON API handlers for /api/grok and /api/update (Node only).
*/
async function resolveChatAuth(body) {
	const apiKey = body.apiKey ? String(body.apiKey) : void 0;
	let accessToken = body.accessToken ? String(body.accessToken) : void 0;
	let tokensOut;
	let refreshed = false;
	if (body.tokens && typeof body.tokens === "object") try {
		const ensured = await ensureAccessToken(body.tokens);
		accessToken = ensured.accessToken;
		tokensOut = ensured.tokens;
		refreshed = ensured.refreshed;
	} catch (e) {
		if (!accessToken && !body.tokens.accessToken) throw e;
		accessToken = accessToken || body.tokens.accessToken;
	}
	if (!accessToken && body.tokens && typeof body.tokens === "object") {
		const t = body.tokens;
		if (t.accessToken) accessToken = t.accessToken;
	}
	return {
		apiKey,
		accessToken,
		tokensOut,
		refreshed
	};
}
/** SSE ReadableStream for chat streaming over HTTP */
function createGrokChatSseStream(body) {
	const encoder = new TextEncoder();
	const messages = body.messages || [];
	const mode = body.mode || "auto";
	const model = body.model ? String(body.model) : void 0;
	return new ReadableStream({ async start(controller) {
		const send = (obj) => {
			controller.enqueue(encoder.encode(`data: ${JSON.stringify(obj)}\n\n`));
		};
		try {
			const auth = await resolveChatAuth(body);
			send({
				type: "status",
				content: "streaming"
			});
			const result = await callXaiChatStream({
				messages,
				mode,
				model,
				apiKey: auth.apiKey,
				accessToken: auth.accessToken
			}, {
				onDelta: (delta) => send({
					type: "delta",
					delta
				}),
				onStatus: (status) => send({
					type: "status",
					content: status
				})
			});
			if (result.aborted) send({
				type: "error",
				error: "Stopped",
				ok: false
			});
			else if (!result.ok) send({
				type: "error",
				error: result.error || "stream failed",
				ok: false
			});
			else send({
				type: "done",
				ok: true,
				content: result.content,
				model: result.model,
				tokens: auth.tokensOut,
				refreshed: auth.refreshed
			});
		} catch (e) {
			send({
				type: "error",
				error: e instanceof Error ? e.message : "stream failed",
				ok: false
			});
		} finally {
			controller.enqueue(encoder.encode("data: [DONE]\n\n"));
			controller.close();
		}
	} });
}
async function dispatchApi(route, action, body) {
	if (route === "grok") {
		if (action === "oauthStart") return {
			ok: true,
			...await startXaiDeviceCode()
		};
		if (action === "oauthPoll") {
			const deviceCode = String(body.deviceCode || "");
			if (!deviceCode) throw new Error("deviceCode required");
			return pollXaiDeviceCode(deviceCode);
		}
		if (action === "oauthEnsure") {
			const tokens = body.tokens;
			if (!tokens?.accessToken) throw new Error("tokens required");
			const ensured = await ensureAccessToken(tokens);
			const probe = await probeXaiBearer(ensured.accessToken);
			return {
				ok: probe.ok,
				detail: probe.detail,
				refreshed: ensured.refreshed,
				tokens: ensured.tokens
			};
		}
		if (action === "probe" || action === "status") {
			const accessToken = String(body.accessToken || "");
			const apiKey = String(body.apiKey || "");
			const bearer = accessToken || apiKey || process.env.XAI_API_KEY || process.env.GROK_API_KEY || "";
			if (!bearer) return {
				ok: false,
				detail: "No Grok OAuth session or API key",
				envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY)
			};
			return {
				...await probeXaiBearer(bearer),
				envConfigured: Boolean(process.env.XAI_API_KEY || process.env.GROK_API_KEY),
				authMode: accessToken ? "oauth" : apiKey ? "apiKey" : "env"
			};
		}
		if (action === "models") {
			const bearer = String(body.accessToken || "") || String(body.apiKey || "") || process.env.XAI_API_KEY || process.env.GROK_API_KEY || "";
			if (!bearer) return { models: [] };
			try {
				const res = await fetch(`${XAI_BASE}/models`, { headers: { authorization: `Bearer ${bearer}` } });
				if (!res.ok) return { models: [] };
				return { models: ((await res.json()).data || []).map((m) => m.id || "").filter(Boolean) };
			} catch {
				return { models: [] };
			}
		}
		if (action === "imagine") {
			const prompt = String(body.prompt || "");
			let accessToken = body.accessToken ? String(body.accessToken) : void 0;
			const apiKey = body.apiKey ? String(body.apiKey) : void 0;
			if (body.tokens && typeof body.tokens === "object") try {
				accessToken = (await ensureAccessToken(body.tokens)).accessToken;
			} catch {}
			return callXaiImagine({
				prompt,
				accessToken,
				apiKey
			});
		}
		if (action === "chatStream") {
			const messages = body.messages || [];
			const mode = body.mode || "auto";
			const model = body.model ? String(body.model) : void 0;
			const auth = await resolveChatAuth(body);
			let content = "";
			const result = await callXaiChatStream({
				messages,
				mode,
				model,
				apiKey: auth.apiKey,
				accessToken: auth.accessToken
			}, { onDelta: (d) => {
				content += d;
			} });
			return {
				...result,
				content: result.content || content,
				...auth.tokensOut ? { tokens: auth.tokensOut } : {},
				refreshed: auth.refreshed
			};
		}
		if (action === "chat") {
			const messages = body.messages || [];
			const mode = body.mode || "auto";
			const model = body.model ? String(body.model) : void 0;
			try {
				const auth = await resolveChatAuth(body);
				return {
					...await callXaiChat({
						messages,
						mode,
						model,
						apiKey: auth.apiKey,
						accessToken: auth.accessToken
					}),
					...auth.tokensOut ? { tokens: auth.tokensOut } : {},
					refreshed: auth.refreshed
				};
			} catch (e) {
				return {
					ok: false,
					error: e instanceof Error ? e.message : "OAuth refresh failed"
				};
			}
		}
		throw new Error(`Unknown grok action: ${action}`);
	}
	if (route === "update") {
		if (action === "check") return checkForUpdate({
			repo: body.repo ? String(body.repo) : void 0,
			branch: body.branch ? String(body.branch) : void 0,
			token: body.token ? String(body.token) : void 0
		});
		if (action === "apply") return applyUpdate({
			repo: body.repo ? String(body.repo) : void 0,
			branch: body.branch ? String(body.branch) : void 0,
			token: body.token ? String(body.token) : void 0,
			force: Boolean(body.force),
			restart: body.restart !== false
		});
		throw new Error(`Unknown update action: ${action}`);
	}
	throw new Error(`Unknown route: ${route}`);
}
//#endregion
export { createGrokChatSseStream, dispatchApi };
