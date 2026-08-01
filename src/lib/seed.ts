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
    },
    {
      id: "desktop-host",
      name: "Desktop Host",
      category: "Local",
      description: "Unsandboxed shell, files, and apps on this Arch machine.",
      status: "disconnected",
      tools: ["exec", "list_dir", "read_file", "open_app"],
    },
    {
      id: "gmail",
      name: "Gmail & Calendar",
      category: "Google",
      description: "Mail, drafts, and calendar events.",
      status: "disconnected",
      tools: ["search_mail", "draft_reply", "list_events", "create_event"],
    },
    {
      id: "gdrive",
      name: "Google Drive",
      category: "Google",
      description: "Docs, Sheets, and Slides.",
      status: "disconnected",
      tools: ["search_files", "read_doc", "update_sheet"],
    },
    {
      id: "github",
      name: "GitHub",
      category: "Code",
      description: "Repos, issues, PRs, and code search.",
      status: "disconnected",
      tools: ["list_issues", "create_pr_comment", "search_code"],
    },
    {
      id: "notion",
      name: "Notion",
      category: "Workspace",
      description: "Pages and databases.",
      status: "disconnected",
      tools: ["search_pages", "update_page", "query_db"],
    },
    {
      id: "outlook",
      name: "Outlook Mail & Calendar",
      category: "Microsoft",
      description: "Inbox and meetings.",
      status: "disconnected",
      tools: ["search_inbox", "draft_mail", "create_meeting"],
    },
    {
      id: "teams",
      name: "Microsoft Teams",
      category: "Microsoft",
      description: "Channels and chats.",
      status: "disconnected",
      tools: ["list_channels", "post_message", "summarize_thread"],
    },
    {
      id: "linear",
      name: "Linear",
      category: "Projects",
      description: "Issues and projects.",
      status: "disconnected",
      tools: ["list_issues", "create_issue", "update_status"],
    },
    {
      id: "custom-mcp",
      name: "Custom MCP",
      category: "Custom",
      description: "Your own MCP server.",
      status: "disconnected",
      tools: ["discover_tools", "invoke_tool"],
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
    messages: [
      {
        id: `sys_${now}`,
        role: "system",
        content:
          "Welcome to GrokHub. Sign in with Grok (Settings or Sign in), add your xAI API key if needed, then start chatting. History appears in the sidebar as you go.",
        ts: now,
      },
    ],
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
