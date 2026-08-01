import { r as __exportAll } from "../_runtime.mjs";
import { t as __exportAll$1 } from "./rolldown-runtime-D7D4PA-g.mjs";
//#region node_modules/.nitro/vite/services/ssr/assets/seed-C2Zadwtn.js
var seed_C2Zadwtn_exports = /* @__PURE__ */ __exportAll({
	n: () => seed_exports,
	t: () => createSeeds
});
var seed_exports = /* @__PURE__ */ __exportAll$1({ createSeeds: () => createSeeds });
/** Catalog of integrations — all disconnected until the user connects them. */
function catalogConnectors() {
	return [
		{
			id: "grok-xai",
			name: "Grok (xAI)",
			category: "Grok",
			description: "Live Grok via SuperGrok / X Premium OAuth or API key.",
			status: "disconnected",
			tools: [
				"chat",
				"models",
				"imagine"
			],
			liveTools: true,
			source: "local"
		},
		{
			id: "desktop-host",
			name: "Desktop Host",
			category: "Local",
			description: "Unsandboxed shell, files, and apps on this Arch machine.",
			status: "disconnected",
			tools: [
				"exec",
				"list_dir",
				"read_file",
				"open_app"
			],
			liveTools: true,
			source: "local"
		},
		{
			id: "github",
			name: "GitHub",
			category: "Code",
			description: "Repos, issues, PRs, and code search (PAT or website link).",
			status: "disconnected",
			tools: [
				"user",
				"list_repos",
				"list_issues",
				"search_code",
				"search_issues"
			],
			liveTools: true,
			source: "token"
		},
		{
			id: "gmail",
			name: "Gmail",
			category: "Google",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"search_mail",
				"draft_reply",
				"list_events",
				"create_event"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "google-calendar",
			name: "Google Calendar",
			category: "Google",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_events", "create_event"],
			liveTools: false,
			source: "website"
		},
		{
			id: "gdrive",
			name: "Google Drive",
			category: "Google",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"search_files",
				"read_doc",
				"update_sheet"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "notion",
			name: "Notion",
			category: "Workspace",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"search_pages",
				"update_page",
				"query_db"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "outlook",
			name: "Outlook",
			category: "Microsoft",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"search_inbox",
				"draft_mail",
				"create_meeting"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "outlook-calendar",
			name: "Outlook Calendar",
			category: "Microsoft",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_events", "create_meeting"],
			liveTools: false,
			source: "website"
		},
		{
			id: "teams",
			name: "Microsoft Teams",
			category: "Microsoft",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"list_channels",
				"post_message",
				"summarize_thread"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "linear",
			name: "Linear",
			category: "Projects",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: [
				"list_issues",
				"create_issue",
				"update_status"
			],
			liveTools: false,
			source: "website"
		},
		{
			id: "box",
			name: "Box",
			category: "Featured",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_files", "search_files"],
			liveTools: false,
			source: "website"
		},
		{
			id: "canva",
			name: "Canva",
			category: "Featured",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_designs"],
			liveTools: false,
			source: "website"
		},
		{
			id: "stripe",
			name: "Stripe",
			category: "Featured",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_payments"],
			liveTools: false,
			source: "website"
		},
		{
			id: "vercel",
			name: "Vercel",
			category: "Featured",
			description: "Sync status from Grok website when linked.",
			status: "disconnected",
			tools: ["list_projects", "list_deployments"],
			liveTools: false,
			source: "website"
		},
		{
			id: "custom-mcp",
			name: "Custom MCP",
			category: "Custom",
			description: "Your own MCP server.",
			status: "disconnected",
			tools: ["discover_tools", "invoke_tool"],
			liveTools: false,
			source: "local"
		}
	];
}
/** Builtin skill templates only — zero runs, no custom personal workflows. */
function catalogSkills() {
	return [
		{
			id: "docs",
			name: "Office Documents",
			description: "Create Word, PowerPoint, Excel, and PDF files.",
			kind: "builtin",
			enabled: true,
			slash: "/docs",
			instructions: "Generate production-ready office documents with correct structure and styles.",
			runs: 0
		},
		{
			id: "skill-creator",
			name: "Skill Creator",
			description: "Capture a reusable workflow as a slash skill.",
			kind: "builtin",
			enabled: true,
			slash: "/skillify",
			instructions: "Turn the conversation into a named persistent skill.",
			runs: 0
		},
		{
			id: "deep-research",
			name: "Deep Research",
			description: "Parallel research with source checks.",
			kind: "builtin",
			enabled: true,
			slash: "/deep-research",
			instructions: "Break the question into sub-queries, verify claims, return a cited report.",
			runs: 0
		}
	];
}
function emptyThread(now) {
	return {
		id: `thread_${now}`,
		title: "New chat",
		createdAt: now,
		updatedAt: now,
		messages: [{
			id: `sys_${now}`,
			role: "system",
			content: "Welcome to GrokHub. Sign in with Grok (Settings or Sign in), add your xAI API key if needed, then start chatting. History appears in the sidebar as you go.",
			ts: now
		}]
	};
}
/** Fresh clean install — no personal preferences or fake history. */
function createSeeds(now = Date.now()) {
	const thread = emptyThread(now);
	return {
		connectors: catalogConnectors(),
		skills: catalogSkills(),
		automations: [],
		agents: [],
		activity: [],
		chat: thread.messages,
		threads: [thread],
		activeThreadId: thread.id,
		heartbeatAt: now
	};
}
createSeeds().connectors;
createSeeds().skills;
createSeeds().chat;
//#endregion
export { seed_C2Zadwtn_exports as n, createSeeds as t };
