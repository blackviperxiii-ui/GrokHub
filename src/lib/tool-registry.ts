/**
 * Unified tool registry for the agent — risk classes + honesty about live vs status-only.
 */

export type ToolRisk = "read" | "write" | "network" | "destructive" | "selfmod";

export type ToolDef = {
  id: string;
  name: string;
  group: "host" | "github" | "website" | "selfmod" | "memory" | "workboard";
  risk: ToolRisk;
  live: boolean;
  description: string;
  protocol?: string;
};

export const TOOL_REGISTRY: ToolDef[] = [
  {
    id: "host.exec",
    name: "Shell exec",
    group: "host",
    risk: "write",
    live: true,
    description: "Run shell on this machine",
    protocol: "HOST_CMD: …",
  },
  {
    id: "host.read",
    name: "Read file / list dir",
    group: "host",
    risk: "read",
    live: true,
    description: "Filesystem read via host bridge",
    protocol: "HOST_CMD: cat|ls|…",
  },
  {
    id: "host.write",
    name: "Write file",
    group: "host",
    risk: "write",
    live: true,
    description: "Write files via host bridge",
    protocol: "HOST_CMD: …",
  },
  {
    id: "github.*",
    name: "GitHub tools",
    group: "github",
    risk: "network",
    live: true,
    description: "search_code, list_issues, etc. with PAT",
    protocol: "CONNECTOR_CMD: github …",
  },
  {
    id: "website.*",
    name: "Website connectors",
    group: "website",
    risk: "network",
    live: false,
    description: "Gmail/Drive/Calendar/etc. — status only until OAuth invoke exists",
    protocol: "CONNECTOR_CMD: (blocked)",
  },
  {
    id: "selfmod.*",
    name: "Self-modify install",
    group: "selfmod",
    risk: "selfmod",
    live: true,
    description: "Opt-in install-tree edits",
    protocol: "SELF_MOD: …",
  },
  {
    id: "memory.note",
    name: "Memory note",
    group: "memory",
    risk: "write",
    live: true,
    description: "Pin lasting notes",
    protocol: "MEMORY_NOTE: …",
  },
  {
    id: "workboard.pin",
    name: "Workboard pin",
    group: "workboard",
    risk: "write",
    live: true,
    description: "Pin tasks for review / goal loop",
    protocol: "WORK_PIN: …",
  },
];

export function liveTools(): ToolDef[] {
  return TOOL_REGISTRY.filter((t) => t.live);
}

export function formatToolRegistryForPrompt(opts?: { includeBlocked?: boolean }): string {
  const list = opts?.includeBlocked ? TOOL_REGISTRY : liveTools();
  return list
    .map(
      (t) =>
        `- ${t.id} [${t.risk}${t.live ? "" : ", NOT LIVE"}]: ${t.description}${
          t.protocol ? ` · ${t.protocol}` : ""
        }`,
    )
    .join("\n");
}

export function riskNeedsApproval(
  risk: ToolRisk,
  level: number,
  hostSafeMode: boolean,
): boolean {
  switch (risk) {
    case "destructive":
    case "selfmod":
      return true;
    case "write":
      return level < 3 || hostSafeMode;
    case "network":
      return level < 2;
    case "read":
    default:
      return false;
  }
}
