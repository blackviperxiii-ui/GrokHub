//#region node_modules/.nitro/vite/services/ssr/assets/host-client-WUUmAwRI.js
function electronHost() {
	return typeof window !== "undefined" ? window.grokhubDesktop?.host : void 0;
}
async function rpc(action, body = {}) {
	const res = await fetch("/api/host", {
		method: "POST",
		headers: {
			"content-type": "application/json",
			accept: "application/json"
		},
		body: JSON.stringify({
			action,
			...body
		})
	});
	const text = await res.text();
	if (text.trimStart().startsWith("<!DOCTYPE") || text.trimStart().startsWith("<html") || (res.headers.get("content-type") || "").includes("text/html")) throw new Error("Host API returned HTML instead of JSON — desktop bridge is offline. Relaunch GrokHub (Electron) or update to a build that includes /api/host.");
	let data;
	try {
		data = JSON.parse(text);
	} catch {
		throw new Error(`Host API invalid JSON (${res.status}): ${text.slice(0, 160)}`);
	}
	if (!res.ok) throw new Error(data.error || `host rpc ${res.status}`);
	if (data && typeof data === "object" && "error" in data && data.error) throw new Error(String(data.error));
	return data;
}
async function hostInfo() {
	const e = electronHost();
	if (e?.info) try {
		return {
			...await e.info(),
			bridge: "electron",
			unsandboxed: true
		};
	} catch (err) {
		console.warn("[host] electron info failed, trying HTTP", err);
	}
	try {
		return await rpc("info");
	} catch (err) {
		const msg = err instanceof Error ? err.message : "host unavailable";
		console.error("[host]", msg);
		return {
			platform: typeof navigator !== "undefined" ? navigator.platform : "unknown",
			arch: "unknown",
			homedir: "~",
			cwd: ".",
			user: "user",
			shell: "/bin/bash",
			hostname: "local",
			bridge: "none",
			unsandboxed: false
		};
	}
}
async function hostListDir(p) {
	const e = electronHost();
	if (e?.listDir) return e.listDir(p);
	return rpc("listDir", { path: p });
}
async function hostReadFile(p, maxBytes) {
	const e = electronHost();
	if (e?.readFile) return e.readFile(p, maxBytes);
	return rpc("readFile", {
		path: p,
		maxBytes
	});
}
async function hostWriteFile(p, content) {
	const e = electronHost();
	if (e?.writeFile) return e.writeFile(p, content);
	return rpc("writeFile", {
		path: p,
		content
	});
}
async function hostExec(command, cwd, timeoutMs) {
	const e = electronHost();
	if (e?.exec) return e.exec(command, cwd, timeoutMs);
	return rpc("exec", {
		command,
		cwd,
		timeoutMs
	});
}
async function hostListApps() {
	const e = electronHost();
	if (e?.listApps) return e.listApps();
	return rpc("listApps");
}
async function hostOpenApp(opts) {
	const e = electronHost();
	if (e?.openApp) return e.openApp(opts);
	return rpc("openApp", opts);
}
function isDesktopShell() {
	return typeof window !== "undefined" && Boolean(window.grokhubDesktop);
}
//#endregion
export { hostExec, hostInfo, hostListApps, hostListDir, hostOpenApp, hostReadFile, hostWriteFile, isDesktopShell };
