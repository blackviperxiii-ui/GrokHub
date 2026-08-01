export type NavId =
  | "command"
  | "connectors"
  | "skills"
  | "automations"
  | "chat"
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
  model: string;
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
    | "usage";
  title: string;
  detail: string;
  status?: RunStatus;
};

export type ChatMessage = {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  ts: number;
  mode?: GrokModeId;
};

export type Agent = {
  id: string;
  name: string;
  role: string;
  model: string;
  status: "idle" | "working" | "offline";
  tasks: number;
  color: string;
};

export type ImagineAspect = "1:1" | "16:9" | "9:16" | "3:2" | "2:3";

export type ImagineJob = {
  id: string;
  prompt: string;
  aspect: ImagineAspect;
  ts: number;
  status: "queued" | "rendering" | "ready" | "failed";
  imageDataUrl?: string;
  mode: GrokModeId;
};

/** Subscription tier for quota metering */
export type SubscriptionPlanId = "free" | "super" | "pro";

export type UsageBucket = "message" | "imagine" | "automation" | "skill" | "host";

export type UsageSnapshot = {
  plan: SubscriptionPlanId;
  /** Billing period bounds (ms) */
  periodStart: number;
  periodEnd: number;
  /** Abstract compute units burned this period */
  usedUnits: number;
  /** Event counters */
  messages: number;
  imagine: number;
  automations: number;
  host: number;
  /** Per-mode message counts */
  byMode: Record<GrokModeId, number>;
};
