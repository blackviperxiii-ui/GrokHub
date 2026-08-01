import { defaultOpenClawPaths } from "./openclaw-import-CL8OB3U2.mjs";
import path from "node:path";
import { exec, spawn } from "node:child_process";
import { promisify } from "node:util";
import fs from "node:fs/promises";
import os from "node:os";
//#region node_modules/.nitro/vite/services/ssr/assets/host-api-handlers-C1lp0pF7.js
/**
* Shared host handlers used by Vite middleware (preview), production Nitro,
* and Electron main (via host-bridge). Always runs in Node — never import from
* React client components.
*
* Unsandboxed: full shell, filesystem, and app launch as the process user.
*/
var execAsync = promisify(exec);
var MAX_STDOUT = 2e5;
var MAX_TIMEOUT = 12e4;
function clip(s, max = MAX_STDOUT) {
	if (!s) return "";
	if (s.length <= max) return s;
	return `${s.slice(0, max)}\n… [truncated ${s.length - max} chars]`;
}
/** Prefer home dir as the user-facing workspace, not the app install path. */
function defaultCwd() {
	try {
		return os.homedir() || process.cwd();
	} catch {
		return process.cwd();
	}
}
function hostEnv() {
	return {
		...process.env,
		GROKHUB_HOST: "1",
		PATH: process.env.PATH || "/usr/local/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin",
		HOME: process.env.HOME || os.homedir(),
		USER: process.env.USER || os.userInfo().username,
		SHELL: process.env.SHELL || "/bin/bash",
		LANG: process.env.LANG || "en_US.UTF-8",
		DISPLAY: process.env.DISPLAY || ":0",
		WAYLAND_DISPLAY: process.env.WAYLAND_DISPLAY || "",
		XDG_RUNTIME_DIR: process.env.XDG_RUNTIME_DIR || `/run/user/${process.getuid?.() ?? 1e3}`,
		DBUS_SESSION_BUS_ADDRESS: process.env.DBUS_SESSION_BUS_ADDRESS || ""
	};
}
async function handleHostInfo() {
	return {
		platform: process.platform,
		arch: process.arch,
		homedir: os.homedir(),
		cwd: defaultCwd(),
		user: os.userInfo().username,
		shell: process.env.SHELL || "/bin/bash",
		hostname: os.hostname(),
		bridge: "server",
		unsandboxed: true
	};
}
async function handleListDir(dirPath) {
	const target = path.resolve(dirPath || os.homedir());
	const names = await fs.readdir(target);
	const entries = [];
	for (const name of names.slice(0, 800)) {
		const full = path.join(target, name);
		try {
			const st = await fs.stat(full);
			entries.push({
				name,
				path: full,
				isDir: st.isDirectory(),
				size: st.size,
				mtimeMs: st.mtimeMs
			});
		} catch {}
	}
	entries.sort((a, b) => {
		if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
		return a.name.localeCompare(b.name);
	});
	return {
		path: target,
		entries
	};
}
async function handleReadFile(filePath, maxBytes = 256e3) {
	const target = path.resolve(filePath);
	const buf = await fs.readFile(target);
	return {
		path: target,
		content: buf.subarray(0, maxBytes).toString("utf8"),
		truncated: buf.length > maxBytes
	};
}
async function handleWriteFile(filePath, content) {
	const target = path.resolve(filePath);
	await fs.mkdir(path.dirname(target), { recursive: true });
	await fs.writeFile(target, content, "utf8");
	return {
		path: target,
		bytes: Buffer.byteLength(content, "utf8")
	};
}
async function handleExec(command, cwd, timeoutMs = 3e4) {
	const cmd = String(command || "").trim();
	if (!cmd) return {
		ok: false,
		code: 1,
		stdout: "",
		stderr: "empty command",
		cwd: cwd || defaultCwd(),
		command: "",
		ms: 0
	};
	const workdir = cwd ? path.resolve(cwd) : defaultCwd();
	try {
		await fs.mkdir(workdir, { recursive: true });
	} catch {}
	const started = Date.now();
	const timeout = Math.min(Math.max(timeoutMs || 3e4, 1e3), MAX_TIMEOUT);
	const shell = process.env.SHELL || "/bin/bash";
	try {
		const { stdout, stderr } = await execAsync(cmd, {
			cwd: workdir,
			timeout,
			maxBuffer: MAX_STDOUT,
			shell,
			env: hostEnv()
		});
		return {
			ok: true,
			code: 0,
			stdout: clip(String(stdout || "")),
			stderr: clip(String(stderr || "")),
			cwd: workdir,
			command: cmd,
			ms: Date.now() - started
		};
	} catch (err) {
		const e = err;
		return {
			ok: false,
			code: typeof e.code === "number" ? e.code : e.killed ? 124 : typeof e.code === "string" ? 1 : 1,
			stdout: clip(String(e.stdout || "")),
			stderr: clip(String(e.stderr || (e.killed ? `command timed out after ${timeout}ms` : e.message || "exec failed"))),
			cwd: workdir,
			command: cmd,
			ms: Date.now() - started
		};
	}
}
async function handleListApps() {
	const dirs = [
		"/usr/share/applications",
		"/usr/local/share/applications",
		path.join(os.homedir(), ".local/share/applications")
	];
	const apps = [];
	for (const dir of dirs) {
		let files = [];
		try {
			files = (await fs.readdir(dir)).filter((f) => f.endsWith(".desktop"));
		} catch {
			continue;
		}
		for (const file of files.slice(0, 500)) {
			const desktopFile = path.join(dir, file);
			try {
				const raw = await fs.readFile(desktopFile, "utf8");
				if (/^NoDisplay\s*=\s*true/im.test(raw)) continue;
				if (/^Hidden\s*=\s*true/im.test(raw)) continue;
				if (/^Type\s*=\s*(?!Application)/im.test(raw) && /^Type\s*=/m.test(raw)) {
					if (!/^Type\s*=\s*Application/im.test(raw)) continue;
				}
				const name = raw.match(/^Name\s*=\s*(.+)$/m)?.[1]?.trim() || file;
				const execLine = raw.match(/^Exec\s*=\s*(.+)$/m)?.[1]?.trim() || "";
				const terminal = /^Terminal\s*=\s*true/im.test(raw);
				const execCmd = execLine.replace(/\s+%[a-zA-Z]/g, "").trim();
				if (!execCmd) continue;
				apps.push({
					id: `${dir}:${file}`,
					name,
					exec: execCmd,
					desktopFile,
					terminal
				});
			} catch {}
		}
	}
	apps.sort((a, b) => a.name.localeCompare(b.name));
	const seen = /* @__PURE__ */ new Set();
	return apps.filter((a) => {
		const k = a.name.toLowerCase();
		if (seen.has(k)) return false;
		seen.add(k);
		return true;
	}).slice(0, 500);
}
async function handleOpenApp(opts) {
	try {
		if (opts.path) {
			spawn("xdg-open", [opts.path], {
				detached: true,
				stdio: "ignore",
				env: hostEnv()
			}).unref();
			return {
				ok: true,
				detail: `opened path ${opts.path}`
			};
		}
		if (opts.desktopFile) {
			spawn("gtk-launch", [path.basename(opts.desktopFile, ".desktop")], {
				detached: true,
				stdio: "ignore",
				env: hostEnv()
			}).unref();
			spawn("xdg-open", [opts.desktopFile], {
				detached: true,
				stdio: "ignore",
				env: hostEnv()
			}).unref();
			return {
				ok: true,
				detail: `launched ${opts.desktopFile}`
			};
		}
		if (opts.exec) {
			spawn(opts.exec, {
				shell: true,
				detached: true,
				stdio: "ignore",
				env: hostEnv()
			}).unref();
			return {
				ok: true,
				detail: `exec ${opts.exec}`
			};
		}
		return {
			ok: false,
			detail: "no target"
		};
	} catch (e) {
		return {
			ok: false,
			detail: e instanceof Error ? e.message : "open failed"
		};
	}
}
async function walkSkillMds(root, relBase, depth, out) {
	if (depth > 6 || out.length >= 200) return;
	let names;
	try {
		names = await fs.readdir(root);
	} catch {
		return;
	}
	for (const name of names) {
		if (name.startsWith(".") || name === "node_modules") continue;
		const full = path.join(root, name);
		const rel = relBase ? `${relBase}/${name}` : name;
		let st;
		try {
			st = await fs.stat(full);
		} catch {
			continue;
		}
		if (st.isDirectory()) await walkSkillMds(full, rel, depth + 1, out);
		else if (st.isFile() && /^skill\.md$/i.test(name) && st.size < 512e3) try {
			const content = await fs.readFile(full, "utf8");
			const dirName = path.basename(path.dirname(full));
			out.push({
				dirName,
				relativePath: rel,
				content
			});
		} catch {}
	}
}
/** Read an OpenClaw workspace directory into a portable payload. */
async function handleReadOpenClawWorkspace(dirPath) {
	const home = os.homedir();
	const candidates = defaultOpenClawPaths(home);
	let target = dirPath?.trim() ? path.resolve(dirPath.trim().replace(/^~(?=\/|$)/, home)) : "";
	if (!target) for (const c of candidates) try {
		if ((await fs.stat(c)).isDirectory()) {
			target = c;
			break;
		}
	} catch {}
	if (!target) return {
		ok: false,
		error: "No OpenClaw workspace found. Pass a path or create ~/.openclaw/workspace",
		root: "",
		files: [],
		skills: [],
		candidates
	};
	let st;
	try {
		st = await fs.stat(target);
	} catch {
		return {
			ok: false,
			error: `Path not found: ${target}`,
			root: target,
			files: [],
			skills: [],
			candidates
		};
	}
	if (!st.isDirectory()) return {
		ok: false,
		error: `Not a directory: ${target}`,
		root: target,
		files: [],
		skills: [],
		candidates
	};
	const files = [];
	for (const name of [
		"AGENTS.md",
		"SOUL.md",
		"USER.md",
		"IDENTITY.md",
		"TOOLS.md",
		"HEARTBEAT.md",
		"MEMORY.md",
		"BOOT.md",
		"BOOTSTRAP.md"
	]) {
		const full = path.join(target, name);
		try {
			const buf = await fs.readFile(full);
			if (buf.length > 4e5) continue;
			files.push({
				name,
				relativePath: name,
				content: buf.toString("utf8")
			});
		} catch {}
	}
	try {
		const memDir = path.join(target, "memory");
		const days = (await fs.readdir(memDir)).filter((n) => /^\d{4}-\d{2}-\d{2}\.md$/.test(n)).sort().reverse().slice(0, 3);
		for (const n of days) try {
			const buf = await fs.readFile(path.join(memDir, n));
			if (buf.length > 2e5) continue;
			files.push({
				name: n,
				relativePath: `memory/${n}`,
				content: buf.toString("utf8")
			});
		} catch {}
	} catch {}
	const skills = [];
	for (const skillRoot of [path.join(target, "skills"), path.join(target, ".agents", "skills")]) await walkSkillMds(skillRoot, path.relative(target, skillRoot) || "skills", 0, skills);
	if (skills.length === 0) await walkSkillMds(path.join(home, ".openclaw", "skills"), "managed-skills", 0, skills);
	let configHint = null;
	try {
		const cfg = path.join(home, ".openclaw", "openclaw.json");
		configHint = (await fs.readFile(cfg)).subarray(0, 8e4).toString("utf8");
	} catch {
		configHint = null;
	}
	return {
		ok: true,
		root: target,
		files,
		skills,
		configHint,
		candidates
	};
}
async function dispatchHost(action, body) {
	switch (action) {
		case "info": return handleHostInfo();
		case "listDir": return handleListDir(body.path);
		case "readFile": return handleReadFile(String(body.path || ""), typeof body.maxBytes === "number" ? body.maxBytes : void 0);
		case "writeFile": return handleWriteFile(String(body.path || ""), String(body.content ?? ""));
		case "exec": return handleExec(String(body.command || ""), body.cwd, typeof body.timeoutMs === "number" ? body.timeoutMs : void 0);
		case "listApps": return handleListApps();
		case "openApp": return handleOpenApp({
			exec: body.exec,
			desktopFile: body.desktopFile,
			path: body.path
		});
		case "readOpenClawWorkspace": return handleReadOpenClawWorkspace(body.path ? String(body.path) : void 0);
		default: throw new Error(`unknown host action: ${action}`);
	}
}
//#endregion
export { dispatchHost };
