//#region node_modules/.nitro/vite/services/ssr/assets/self-mod-client-CTO9n7NH.js
function api() {
	return typeof window !== "undefined" ? window.grokhubDesktop?.selfmod : void 0;
}
async function selfModInfo() {
	const s = api();
	if (!s?.info) return { ok: false };
	return s.info();
}
async function selfModList(rel) {
	const s = api();
	if (!s?.list) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.list(rel);
}
async function selfModRead(rel) {
	const s = api();
	if (!s?.read) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.read(rel);
}
async function selfModWrite(rel, content, opts) {
	const s = api();
	if (!s?.write) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.write(rel, content, opts);
}
async function selfModPatch(rel, find, replace, opts) {
	const s = api();
	if (!s?.patch) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.patch(rel, find, replace, opts);
}
async function selfModSnapshot(note) {
	const s = api();
	if (!s?.snapshot) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.snapshot(note);
}
async function selfModRestore(id) {
	const s = api();
	if (!s?.restore) return {
		ok: false,
		error: "Desktop self-mod unavailable"
	};
	return s.restore(id);
}
async function factoryReinstall(opts) {
	const g = typeof window !== "undefined" ? window.grokhubDesktop?.grok : void 0;
	if (!g?.factoryReinstall) {
		if (g?.applyUpdate) return g.applyUpdate({
			force: true,
			factory: true,
			...opts
		});
		return {
			ok: false,
			error: "Factory reinstall requires the desktop app"
		};
	}
	return g.factoryReinstall(opts || {});
}
/**
* Lines:
* SELF_MOD: list src/components
* SELF_MOD: read src/lib/version.ts
* SELF_MOD: write path/to/file
* <<<CONTENT
* ...file body...
* CONTENT>>>
* SELF_MOD: patch path/to/file
* <<<FIND
* old
* FIND>>>
* <<<REPLACE
* new
* REPLACE>>>
* SELF_MOD: snapshot note text
*/
function extractSelfModCommands(text) {
	const out = [];
	const lines = text.split("\n");
	for (let i = 0; i < lines.length; i++) {
		const m = (lines[i] || "").match(/^\s*SELF_MOD:\s*(list|read|write|patch|snapshot)\s*(.*)$/i);
		if (!m) continue;
		const op = m[1].toLowerCase();
		const rest = (m[2] || "").trim();
		if (op === "list") {
			out.push({
				kind: "list",
				path: rest || "src"
			});
			continue;
		}
		if (op === "read") {
			if (rest) out.push({
				kind: "read",
				path: rest
			});
			continue;
		}
		if (op === "snapshot") {
			out.push({
				kind: "snapshot",
				note: rest || "agent"
			});
			continue;
		}
		if (op === "write") {
			let content = "";
			let j = i + 1;
			if ((lines[j] || "").trim() === "<<<CONTENT") {
				j++;
				const buf = [];
				while (j < lines.length && (lines[j] || "").trim() !== "CONTENT>>>") {
					buf.push(lines[j]);
					j++;
				}
				content = buf.join("\n");
				i = j;
			}
			if (rest) out.push({
				kind: "write",
				path: rest,
				content
			});
			continue;
		}
		if (op === "patch") {
			let find = "";
			let replace = "";
			let j = i + 1;
			while (j < lines.length) {
				const L = (lines[j] || "").trim();
				if (L === "<<<FIND") {
					j++;
					const buf = [];
					while (j < lines.length && (lines[j] || "").trim() !== "FIND>>>") {
						buf.push(lines[j]);
						j++;
					}
					find = buf.join("\n");
					j++;
					continue;
				}
				if (L === "<<<REPLACE") {
					j++;
					const buf = [];
					while (j < lines.length && (lines[j] || "").trim() !== "REPLACE>>>") {
						buf.push(lines[j]);
						j++;
					}
					replace = buf.join("\n");
					j++;
					break;
				}
				if (/^\s*SELF_MOD:/i.test(lines[j] || "")) break;
				j++;
			}
			i = j - 1;
			if (rest && find) out.push({
				kind: "patch",
				path: rest,
				find,
				replace
			});
		}
	}
	return out.slice(0, 6);
}
function stripSelfModCommands(text) {
	let out = text;
	out = out.replace(/^\s*SELF_MOD:.*$/gim, "");
	out = out.replace(/<<<CONTENT\n[\s\S]*?\nCONTENT>>>/g, "");
	out = out.replace(/<<<FIND\n[\s\S]*?\nFIND>>>/g, "");
	out = out.replace(/<<<REPLACE\n[\s\S]*?\nREPLACE>>>/g, "");
	return out.replace(/\n{3,}/g, "\n\n").trim();
}
//#endregion
export { extractSelfModCommands, factoryReinstall, selfModInfo, selfModList, selfModPatch, selfModRead, selfModRestore, selfModSnapshot, selfModWrite, stripSelfModCommands };
