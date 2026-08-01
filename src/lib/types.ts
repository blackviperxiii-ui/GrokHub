export type NavId =
  | "chat"
  | "history"
  | "command"
  | "connectors"
  | "skills"
  | "automations"
  | "agents"
  | "imagine"
  | "desktop"
  | "settings";

/** Matches Grok web model modes (Auto / Fast / Expert / Heavy / Build). */
export type GrokModeId = "auto" | "fast" | "expert" | "heavy" | "build";

export type GrokMode = {
  id: GrokModeId;
  label: string;
  subtitle: string;
  /** Human-readable model family shown in UI */
  model: string;
  /** xAI API model id used for live requests */
  modelId: string;
  icon: "auto" | "fast" | "expert" | "heavy" | "build";
  latencyMs: [number, number];
  depth: "light" | "standard" | "deep" | "team" | "code";
};

export type ConnectorStatus = "connected" | "disconnected" | "error";

export type Connector = {
  id: string;
  name: string;
  category: string;
  description: string;
  status: ConnectorStatus;
  tools: string[];
  lastUsed?: number;
};

export type Skill = {
  id: string;
  name: string;
  description: string;
  kind: "builtin" | "custom";
  enabled: boolean;
  slash: string;
  instructions: string;
  runs: number;
};

export type AutomationSchedule =
  | "once"
  | "daily"
  | "weekdays"
  | "weekly"
  | "monthly";

export type Automation = {
  id: string;
  name: string;
  instructions: string;
  schedule: AutomationSchedule;
  time: string;
  enabled: boolean;
  connectorIds: string[];
  skillIds: string[];
  lastRun?: number;
  nextRun?: number;
  runCount: number;
};

export type RunStatus = "running" | "success" | "failed" | "queued";

export type ActivityItem = {
  id: string;
  ts: number;
  kind:
    | "automation"
    | "skill"
    | "connector"
    | "chat"
    | "system"
    | "agent"
    | "imagine"
    | "desktop"
    | "usage"
    | "auth";
  title: string;
  detail: string;
  status: RunStatus;
};

export type AgentStatus = "idle" | "working" | "offline";

export type Agent = {
  id: string;
  name: string;
  role: string;
  model: string;
  status: AgentStatus;
  tasks: number;
  color: string;
};

export type ChatRole = "user" | "assistant" | "system";

export type ChatMessage = {
  id: string;
  role: ChatRole;
  content: string;
  ts: number;
  mode?: GrokModeId;
};

/** Grok-style conversation history entry */
export type ChatThread = {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
  mode?: GrokModeId;
};

export type ImagineAspect = "1:1" | "16:9" | "9:16" | "4:3" | "3:2" | "2:3";


export type ImagineJob = {
  id: string;
  prompt: string;
  aspect: ImagineAspect;
  ts: number;
  status: "rendering" | "ready" | "failed";
  mode?: GrokModeId;
  imageDataUrl?: string;
};

export type SubscriptionPlanId = "free" | "pro" | "super";



export type UsageBucket = "message" | "imagine" | "automation" | "host" | "skill";

export type UsageSnapshot = {
  plan: SubscriptionPlanId;
  periodStart: number;
  periodEnd: number;
  usedUnits: number;
  messages: number;
  imagine: number;
  automations: number;
  host: number;
  byMode: Record<GrokModeId, number>;
};

/** Profile synced after Grok OAuth / API connect — never seeded with personal defaults. */
export type GrokProfile = {
  displayName: string | null;
  email: string | null;
  imageUrl: string | null;
  models: string[];
  connectedAt: number | null;
};
