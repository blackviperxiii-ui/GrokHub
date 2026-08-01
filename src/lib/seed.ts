import type {
  ActivityItem,
  Agent,
  Automation,
  ChatMessage,
  Connector,
  Skill,
} from "./types";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

export type SeedBundle = {
  connectors: Connector[];
  skills: Skill[];
  automations: Automation[];
  agents: Agent[];
  activity: ActivityItem[];
  chat: ChatMessage[];
  heartbeatAt: number;
};

/** Fresh relative timestamps so activity never freezes at a calendar date. */
export function createSeeds(now = Date.now()): SeedBundle {
  const connectors: Connector[] = [
    {
      id: "gmail",
      name: "Gmail & Calendar",
      category: "Google",
      description: "Read mail, draft replies, and manage calendar events.",
      status: "connected",
      tools: ["search_mail", "draft_reply", "list_events", "create_event"],
      lastUsed: now - 12 * MINUTE,
    },
    {
      id: "gdrive",
      name: "Google Drive",
      category: "Google",
      description: "Search, read, and update Docs, Sheets, and Slides.",
      status: "connected",
      tools: ["search_files", "read_doc", "update_sheet"],
      lastUsed: now - 40 * MINUTE,
    },
    {
      id: "github",
      name: "GitHub",
      category: "Code",
      description: "Repos, issues, PRs, and code search.",
      status: "connected",
      tools: ["list_issues", "create_pr_comment", "search_code"],
      lastUsed: now - 5 * MINUTE,
    },
    {
      id: "notion",
      name: "Notion",
      category: "Workspace",
      description: "Pages, databases, and wikis across your workspace.",
      status: "disconnected",
      tools: ["search_pages", "update_page", "query_db"],
    },
    {
      id: "outlook",
      name: "Outlook Mail & Calendar",
      category: "Microsoft",
      description: "Inbox triage, drafts, and meeting management.",
      status: "disconnected",
      tools: ["search_inbox", "draft_mail", "create_meeting"],
    },
    {
      id: "teams",
      name: "Microsoft Teams",
      category: "Microsoft",
      description: "Channels, chats, and message summaries.",
      status: "disconnected",
      tools: ["list_channels", "post_message", "summarize_thread"],
    },
    {
      id: "linear",
      name: "Linear",
      category: "Projects",
      description: "Issues, projects, and roadmap updates.",
      status: "connected",
      tools: ["list_issues", "create_issue", "update_status"],
      lastUsed: now - 90 * MINUTE,
    },
    {
      id: "onedrive",
      name: "OneDrive",
      category: "Microsoft",
      description: "Personal and shared files in OneDrive.",
      status: "disconnected",
      tools: ["search_files", "read_file"],
    },
    {
      id: "vercel",
      name: "Vercel",
      category: "Deploy",
      description: "Projects, deployments, and domain status.",
      status: "disconnected",
      tools: ["list_deploys", "get_status"],
    },
    {
      id: "custom-mcp",
      name: "Custom MCP",
      category: "Custom",
      description: "Bring your own MCP server for local or private tools.",
      status: "disconnected",
      tools: ["discover_tools", "invoke_tool"],
    },
  ];

  const skills: Skill[] = [
    {
      id: "docs",
      name: "Office Documents",
      description: "Create and edit polished Word, PowerPoint, Excel, and PDF files.",
      kind: "builtin",
      enabled: true,
      slash: "/docs",
      instructions:
        "Generate production-ready office documents with correct structure, styles, and tables.",
      runs: 48,
    },
    {
      id: "skill-creator",
      name: "Skill Creator",
      description: "Teach Grok a reusable workflow once; apply it forever.",
      kind: "builtin",
      enabled: true,
      slash: "/skillify",
      instructions: "Capture a conversation workflow into a named persistent skill.",
      runs: 12,
    },
    {
      id: "morning-brief",
      name: "Morning Brief",
      description: "Inbox highlights, calendar, open issues, and day plan.",
      kind: "custom",
      enabled: true,
      slash: "/morning",
      instructions:
        "Pull Gmail + Calendar + GitHub + Linear. Summarize top 5 actions. Keep under 12 bullets.",
      runs: 31,
    },
    {
      id: "standup",
      name: "Standup Notes",
      description: "Turn recent activity into a short standup update.",
      kind: "custom",
      enabled: true,
      slash: "/standup",
      instructions:
        "Summarize yesterday's shipped work, today's plan, and blockers in three bullets.",
      runs: 14,
    },
    {
      id: "print-queue",
      name: "Print Queue",
      description: "3D print job tracker and packaging checklist.",
      kind: "custom",
      enabled: false,
      slash: "/prints",
      instructions:
        "Track print jobs, material, failure notes, and packaging steps for finished parts.",
      runs: 4,
    },
    {
      id: "deep-research",
      name: "Deep Research",
      description: "Fan research out across parallel agents with source checks.",
      kind: "builtin",
      enabled: true,
      slash: "/deep-research",
      instructions:
        "Break the question into sub-queries, verify claims, return a cited report.",
      runs: 17,
    },
  ];

  const automations: Automation[] = [
    {
      id: "auto-morning",
      name: "08:00 Morning Brief",
      instructions:
        "Run /morning. Email me the brief and post open P0s from Linear + GitHub.",
      schedule: "weekdays",
      time: "08:00",
      enabled: true,
      connectorIds: ["gmail", "github", "linear"],
      skillIds: ["morning-brief", "docs"],
      lastRun: now - 14 * HOUR,
      nextRun: now + 10 * HOUR,
      runCount: 27,
    },
    {
      id: "auto-inbox",
      name: "Inbox triage",
      instructions:
        "When mail arrives from bills or work, summarize and draft a reply if needed.",
      schedule: "daily",
      time: "on email",
      enabled: true,
      connectorIds: ["gmail"],
      skillIds: [],
      lastRun: now - 35 * MINUTE,
      runCount: 64,
    },
    {
      id: "auto-weekly-review",
      name: "Weekly agent review",
      instructions:
        "Summarize all automation runs, skill usage, and connector health for the week.",
      schedule: "weekly",
      time: "Sun 18:00",
      enabled: false,
      connectorIds: ["github", "gmail"],
      skillIds: ["docs"],
      runCount: 3,
    },
  ];

  const agents: Agent[] = [
    {
      id: "primary",
      name: "Primary",
      role: "Primary co-pilot",
      model: "xai/grok-4.5 · Auto/Fast/Expert",
      status: "idle",
      tasks: 0,
      color: "#d4d4d8",
    },
    {
      id: "builder",
      name: "Build",
      role: "Build mode",
      model: "xai/grok-4.5 · Build",
      status: "idle",
      tasks: 0,
      color: "#7dd3fc",
    },
    {
      id: "research",
      name: "Research",
      role: "Heavy / Expert",
      model: "xai/grok-4.5 · Heavy",
      status: "offline",
      tasks: 0,
      color: "#34d399",
    },
    {
      id: "ops",
      name: "Ops",
      role: "Ops & automations",
      model: "xai/grok-4.5 · Fast",
      status: "idle",
      tasks: 0,
      color: "#fbbf24",
    },
  ];

  const activity: ActivityItem[] = [
    {
      id: "a1",
      ts: now - 5 * MINUTE,
      kind: "connector",
      title: "GitHub tools used",
      detail: "list_issues on example/app — 12 open, 2 P0",
      status: "success",
    },
    {
      id: "a2",
      ts: now - 35 * MINUTE,
      kind: "automation",
      title: "Inbox triage completed",
      detail: "3 messages summarized, 1 draft prepared",
      status: "success",
    },
    {
      id: "a3",
      ts: now - 90 * MINUTE,
      kind: "skill",
      title: "Morning Brief ran",
      detail: "Calendar + mail + Linear rolled into day plan",
      status: "success",
    },
    {
      id: "a4",
      ts: now - 3 * HOUR,
      kind: "system",
      title: "Build mode default",
      detail: "Auto / Fast / Expert / Heavy / Build + Imagine loaded",
      status: "success",
    },
    {
      id: "a5",
      ts: now - 8 * MINUTE,
      kind: "desktop",
      title: "Host bridge ready",
      detail: "Unsandboxed CLI · files · apps (Electron or server bridge)",
      status: "success",
    },
  ];

  const chat: ChatMessage[] = [
    {
      id: "c0",
      role: "system",
      content:
        "GrokHub desktop online (v0.1). Modes Auto/Fast/Expert/Heavy/Build map to live xAI Grok models. Add your API key in Settings to connect. Prefix shell with $ for host CLI.",

      ts: now - 2 * MINUTE,
    },
  ];

  return {
    connectors,
    skills,
    automations,
    agents,
    activity,
    chat,
    heartbeatAt: now,
  };
}

/** Module-load seeds for first paint; client remounts via createSeeds on reset. */
const boot = createSeeds();

export const seedConnectors = boot.connectors;
export const seedSkills = boot.skills;
export const seedAutomations = boot.automations;
export const seedAgents = boot.agents;
export const seedActivity = boot.activity;
export const seedChat = boot.chat;
export const SEED_HEARTBEAT = boot.heartbeatAt;
