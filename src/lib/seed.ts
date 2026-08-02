/**
 * Clean-install seeds only. No personal history, no fake "connected" state.
 * Real data is filled after Grok OAuth / API connection.
 */
import type {
  ActivityItem,
  Agent,
  Automation,
  ChatMessage,
  ChatThread,
  Connector,
  Skill,
} from "./types";

export type SeedBundle = {
  connectors: Connector[];
  skills: Skill[];
  automations: Automation[];
  agents: Agent[];
  activity: ActivityItem[];
  chat: ChatMessage[];
  threads: ChatThread[];
  activeThreadId: string | null;
  heartbeatAt: number;
};

/** Catalog of integrations — all disconnected until the user connects them. */
function catalogConnectors(): Connector[] {
  return [
    {
      id: "grok-xai",
      name: "Grok (xAI)",
      category: "Grok",
      description: "Live Grok via SuperGrok / X Premium OAuth or API key.",
      status: "disconnected",
      tools: ["chat", "models", "imagine"],
      liveTools: true,
      source: "local",
    },
    {
      id: "desktop-host",
      name: "Desktop Host",
      category: "Local",
      description: "Unsandboxed shell, files, and apps on this Arch machine.",
      status: "disconnected",
      tools: ["exec", "list_dir", "read_file", "open_app"],
      liveTools: true,
      source: "local",
    },
    {
      id: "github",
      name: "GitHub",
      category: "Code",
      description: "Repos, issues, PRs, and code search (PAT or website link).",
      status: "disconnected",
      tools: ["user", "list_repos", "list_issues", "search_code", "search_issues"],
      liveTools: true,
      source: "token",
    },
    {
      id: "gmail",
      name: "Gmail",
      category: "Google",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["search_mail", "draft_reply", "list_events", "create_event"],
      liveTools: false,
      source: "website",
    },
    {
      id: "google-calendar",
      name: "Google Calendar",
      category: "Google",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_events", "create_event"],
      liveTools: false,
      source: "website",
    },
    {
      id: "gdrive",
      name: "Google Drive",
      category: "Google",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["search_files", "read_doc", "update_sheet"],
      liveTools: false,
      source: "website",
    },
    {
      id: "notion",
      name: "Notion",
      category: "Workspace",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["search_pages", "update_page", "query_db"],
      liveTools: false,
      source: "website",
    },
    {
      id: "outlook",
      name: "Outlook",
      category: "Microsoft",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["search_inbox", "draft_mail", "create_meeting"],
      liveTools: false,
      source: "website",
    },
    {
      id: "outlook-calendar",
      name: "Outlook Calendar",
      category: "Microsoft",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_events", "create_meeting"],
      liveTools: false,
      source: "website",
    },
    {
      id: "teams",
      name: "Microsoft Teams",
      category: "Microsoft",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_channels", "post_message", "summarize_thread"],
      liveTools: false,
      source: "website",
    },
    {
      id: "linear",
      name: "Linear",
      category: "Projects",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_issues", "create_issue", "update_status"],
      liveTools: false,
      source: "website",
    },
    {
      id: "box",
      name: "Box",
      category: "Featured",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_files", "search_files"],
      liveTools: false,
      source: "website",
    },
    {
      id: "canva",
      name: "Canva",
      category: "Featured",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_designs"],
      liveTools: false,
      source: "website",
    },
    {
      id: "stripe",
      name: "Stripe",
      category: "Featured",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_payments"],
      liveTools: false,
      source: "website",
    },
    {
      id: "vercel",
      name: "Vercel",
      category: "Featured",
      description: "Website status only when grok.com is linked — not executable tools from this app.",
      status: "disconnected",
      tools: ["list_projects", "list_deployments"],
      liveTools: false,
      source: "website",
    },
    {
      id: "custom-mcp",
      name: "Custom MCP",
      category: "Custom",
      description: "Your own MCP server.",
      status: "disconnected",
      tools: ["discover_tools", "invoke_tool"],
      liveTools: false,
      source: "local",
    },
  ];
}

/** Builtin skill templates only — zero runs, no custom personal workflows. */
function catalogSkills(): Skill[] {
  return [
    {
      id: "docs",
      name: "Office Documents",
      description: "Create Word, PowerPoint, Excel, and PDF files.",
      kind: "builtin",
      enabled: true,
      slash: "/docs",
      instructions:
        "Generate production-ready office documents with correct structure and styles.",
      runs: 0,
    },
    {
      id: "skill-creator",
      name: "Skill Creator",
      description: "Capture a reusable workflow as a slash skill.",
      kind: "builtin",
      enabled: true,
      slash: "/skillify",
      instructions: "Turn the conversation into a named persistent skill.",
      runs: 0,
    },
    {
      id: "deep-research",
      name: "Deep Research",
      description: "Parallel research with source checks.",
      kind: "builtin",
      enabled: true,
      slash: "/deep-research",
      instructions: "Break the question into sub-queries, verify claims, return a cited report.",
      runs: 0,
    },
  ];
}

function emptyThread(now: number): ChatThread {
  const id = `thread_${now}`;
  return {
    id,
    title: "New chat",
    createdAt: now,
    updatedAt: now,
    // Empty messages so the adaptive empty-state welcome (Fast + learning) can render
    messages: [],
  };
}

/** Fresh clean install — no personal preferences or fake history. */
export function createSeeds(now = Date.now()): SeedBundle {
  const thread = emptyThread(now);
  return {
    connectors: catalogConnectors(),
    skills: catalogSkills(),
    automations: [] as Automation[],
    agents: [] as Agent[],
    activity: [] as ActivityItem[],
    chat: thread.messages,
    threads: [thread],
    activeThreadId: thread.id,
    heartbeatAt: now,
  };
}

export const seedConnectors = createSeeds().connectors;
export const seedSkills = createSeeds().skills;
export const seedAutomations: Automation[] = [];
export const seedAgents: Agent[] = [];
export const seedActivity: ActivityItem[] = [];
export const seedChat = createSeeds().chat;
export const SEED_HEARTBEAT = Date.now();
