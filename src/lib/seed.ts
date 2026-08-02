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

/** Core integrations only — keep GrokHub simple and agent-first. */
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
      description: "Unsandboxed shell, files, and apps on this machine.",
      status: "disconnected",
      tools: ["exec", "list_dir", "read_file", "open_app"],
      liveTools: true,
      source: "local",
    },
    {
      id: "github",
      name: "GitHub",
      category: "Code",
      description: "Optional: repos and issues via a personal access token in Settings.",
      status: "disconnected",
      tools: ["user", "list_repos", "list_issues", "search_code", "search_issues"],
      liveTools: true,
      source: "token",
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
