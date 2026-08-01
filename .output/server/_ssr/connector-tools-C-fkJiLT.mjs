//#region node_modules/.nitro/vite/services/ssr/assets/connector-tools-C-fkJiLT.js
/** CONNECTOR_CMD: github search_code query:foo language:ts */
function extractConnectorCommands(text) {
	const out = [];
	const seen = /* @__PURE__ */ new Set();
	for (const line of text.split("\n")) {
		const m = line.match(/^\s*CONNECTOR_CMD:\s*(\S+)\s+(\S+)(?:\s+(.+))?\s*$/i);
		if (!m) continue;
		const raw = line.trim();
		const key = raw.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		out.push({
			connectorId: m[1].toLowerCase(),
			tool: m[2].toLowerCase(),
			args: (m[3] || "").trim(),
			raw
		});
	}
	return out.slice(0, 4);
}
function stripConnectorCommands(text) {
	return text.split("\n").filter((line) => !/^\s*CONNECTOR_CMD:\s*/i.test(line)).join("\n").replace(/\s*CONNECTOR_CMD:\s*[^\n]+/gi, "").replace(/\n{3,}/g, "\n\n").trim();
}
function parseArgs(args) {
	const out = {};
	if (!args) return out;
	if (!args.includes(":")) {
		out.q = args;
		out.query = args;
		return out;
	}
	const re = /(\w+):(?:"([^"]*)"|(\S+))/g;
	let m;
	let any = false;
	while (m = re.exec(args)) {
		any = true;
		out[m[1].toLowerCase()] = m[2] ?? m[3] ?? "";
	}
	if (!any) {
		out.q = args;
		out.query = args;
	}
	return out;
}
async function gh(token, path, init) {
	const res = await fetch(`https://api.github.com${path}`, {
		...init,
		headers: {
			accept: "application/vnd.github+json",
			authorization: `Bearer ${token}`,
			"user-agent": "GrokHub",
			"x-github-api-version": "2022-11-28",
			...init?.headers || {}
		}
	});
	const text = await res.text();
	let json = null;
	try {
		json = JSON.parse(text);
	} catch {}
	return {
		ok: res.ok,
		status: res.status,
		json,
		text
	};
}
async function runGithubTool(tool, args, token) {
	if (!token.trim()) return {
		ok: false,
		connectorId: "github",
		tool,
		detail: "No GitHub token. Settings → paste a classic/fine-grained PAT with repo scope."
	};
	const a = parseArgs(args);
	try {
		if (tool === "user" || tool === "me") {
			const r = await gh(token, "/user");
			if (!r.ok) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: `GitHub ${r.status}: ${r.text.slice(0, 200)}`
			};
			const u = r.json;
			return {
				ok: true,
				connectorId: "github",
				tool,
				detail: `Authenticated as ${u.login}${u.name ? ` (${u.name})` : ""} · ${u.public_repos ?? "?"} public repos`,
				data: u
			};
		}
		if (tool === "list_repos" || tool === "repos") {
			const r = await gh(token, "/user/repos?per_page=20&sort=updated");
			if (!r.ok) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: `GitHub ${r.status}`
			};
			const repos = r.json;
			return {
				ok: true,
				connectorId: "github",
				tool,
				detail: (repos || []).slice(0, 20).map((x) => `- ${x.full_name}${x.private ? " (private)" : ""} — ${x.description || ""}`).join("\n") || "(no repos)",
				data: repos
			};
		}
		if (tool === "list_issues" || tool === "issues") {
			const repo = a.repo || a.repository || a.q || "";
			if (!repo.includes("/")) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: "Need repo:owner/name  e.g. CONNECTOR_CMD: github list_issues repo:vercel/next.js"
			};
			const r = await gh(token, `/repos/${repo}/issues?state=open&per_page=15`);
			if (!r.ok) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: `GitHub ${r.status}: ${r.text.slice(0, 200)}`
			};
			const issues = r.json;
			return {
				ok: true,
				connectorId: "github",
				tool,
				detail: (issues || []).filter((i) => i.number != null).map((i) => `#${i.number} ${i.title} (@${i.user?.login || "?"})`).join("\n") || "No open issues",
				data: issues
			};
		}
		if (tool === "search_code" || tool === "code_search") {
			const q = a.q || a.query || args;
			if (!q) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: "Need query:… for search_code"
			};
			const r = await gh(token, `/search/code?q=${encodeURIComponent(q)}&per_page=10`);
			if (!r.ok) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: `GitHub ${r.status}: ${r.text.slice(0, 240)}`
			};
			const body = r.json;
			return {
				ok: true,
				connectorId: "github",
				tool,
				detail: (body.items || []).map((it) => `- ${it.repository?.full_name}/${it.path}\n  ${it.html_url}`).join("\n") || "No code matches",
				data: body
			};
		}
		if (tool === "search_issues") {
			const q = a.q || a.query || args;
			const r = await gh(token, `/search/issues?q=${encodeURIComponent(q)}&per_page=10`);
			if (!r.ok) return {
				ok: false,
				connectorId: "github",
				tool,
				detail: `GitHub ${r.status}`
			};
			const body = r.json;
			return {
				ok: true,
				connectorId: "github",
				tool,
				detail: (body.items || []).map((it) => `- ${it.title}\n  ${it.html_url}`).join("\n") || "No matches",
				data: body
			};
		}
		if (tool === "create_pr_comment" || tool === "comment") return {
			ok: false,
			connectorId: "github",
			tool,
			detail: "create_pr_comment requires repo + issue/PR number + body — use GitHub web for writes unless repo: issue: body: are all provided."
		};
		return {
			ok: false,
			connectorId: "github",
			tool,
			detail: `Unknown GitHub tool "${tool}". Try: user, list_repos, list_issues, search_code, search_issues`
		};
	} catch (e) {
		return {
			ok: false,
			connectorId: "github",
			tool,
			detail: e instanceof Error ? e.message : "GitHub tool failed"
		};
	}
}
async function runConnectorTool(opts) {
	const id = opts.connectorId.replace(/^gh$/, "github");
	if (id === "github") return runGithubTool(opts.tool, opts.args, opts.githubToken || "");
	if (opts.websiteConnected) return {
		ok: false,
		connectorId: id,
		tool: opts.tool,
		detail: [
			`${id} is linked on the Grok website${opts.accountLabel ? ` as ${opts.accountLabel}` : ""}.`,
			"Desktop cannot invoke Grok website connector tools yet (tokens stay on xAI).",
			"Options: use this connector in the Grok website chat, or add a local token in Settings when GrokHub supports native OAuth for this provider.",
			`Requested: ${opts.tool} ${opts.args}`.trim()
		].join("\n")
	};
	return {
		ok: false,
		connectorId: id,
		tool: opts.tool,
		detail: `${id} is not connected. Link it via Grok website (Settings → Link Grok website) or Connect in the Connectors tab.`
	};
}
/** Map website product names → our connector ids */
var WEBSITE_CONNECTOR_ALIASES = {
	github: "github",
	"git hub": "github",
	notion: "notion",
	"microsoft teams": "teams",
	teams: "teams",
	outlook: "outlook",
	"outlook mail": "outlook",
	"outlook calendar": "outlook-calendar",
	"google calendar": "google-calendar",
	"google drive": "gdrive",
	gdrive: "gdrive",
	gmail: "gmail",
	"gmail & calendar": "gmail",
	box: "box",
	canva: "canva",
	stripe: "stripe",
	vercel: "vercel",
	linear: "linear"
};
function mapWebsiteConnectorName(name) {
	const k = name.trim().toLowerCase();
	if (WEBSITE_CONNECTOR_ALIASES[k]) return WEBSITE_CONNECTOR_ALIASES[k];
	for (const [alias, id] of Object.entries(WEBSITE_CONNECTOR_ALIASES)) if (k.includes(alias) || alias.includes(k)) return id;
	return null;
}
//#endregion
export { extractConnectorCommands, mapWebsiteConnectorName, runConnectorTool, stripConnectorCommands };
