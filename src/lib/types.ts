export type NavId =
  | "chat"
  | "history"
  | "command"
  | "connectors"
  | "skills"
  | "automations"
  | "agents"
  | "workboard"
  | "queue"
  | "imagine"
  | "desktop"
  | "settings";

/** Matches Grok web model modes (Auto / Fast / Expert / Heavy / Build). */
export type GrokModeId = "auto" | "fast" | "expert" | "heavy" | "max" | "build";

export type GrokMode = {
  id: GrokModeId;
  label: string;
  subtitle: string;
  /** Human-readable model family shown in UI */
  model: string;
  /** xAI API model id used for live requests */
  modelId: string;
  icon: "auto" | "fast" | "expert" | "heavy" | "max" | "build";
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
  /** Account email/login when known (website sync or OAuth) */
  accountLabel?: string | null;
  /** Where connection was established */
  source?: "local" | "website" | "token";
  /** True when GrokHub can execute tools for this connector */
  liveTools?: boolean;
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
  | "monthly"
  /** Fire on each app heartbeat tick while enabled */
  | "heartbeat";

export type Automation = {
  id: string;
  name: string;
  instructions: string;
  schedule: AutomationSchedule;
  /**
   * Primary time (HH:mm) — kept for backwards compatibility.
   * Prefer `times` when multiple slots are set.
   */
  time: string;
  /** One or more daily/weekly clock times (HH:mm local). */
  times?: string[];
  /** When schedule is heartbeat, min minutes between runs (default 5). */
  heartbeatEveryMin?: number;
  enabled: boolean;
  connectorIds: string[];
  skillIds: string[];
  lastRun?: number;
  nextRun?: number;
  runCount: number;
  /** Consecutive failed automation runs (reset on success). */
  failCount?: number;
  /** Thread that last ran this automation (deep-link). */
  lastThreadId?: string | null;
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
  /** Adaptive tier actually used (⚡ Fast / 🧠 Think / 🔬 Deep / …) */
  routeTier?: "fast" | "think" | "deep" | "build" | "imagine";
  /** Human explanation for hover */
  routeReason?: string;
  /** Concrete model id used */
  routeModel?: string;
  /** True while tokens are still arriving */
  streaming?: boolean;
  /** Stopped by user mid-stream */
  stopped?: boolean;
  /** Reply-to another message in this thread */
  replyToId?: string;
  replyToPreview?: string;
  replyToRole?: ChatRole;
  /** User edited this message after send */
  edited?: boolean;
};

/** Grok-style conversation history entry */
export type ChatThread = {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: ChatMessage[];
  mode?: GrokModeId;
  /** Pin to top of history / sidebar */
  pinned?: boolean;
  /** Optional folder label (e.g. Work, Personal) */
  folder?: string | null;
  /**
   * When true, user renamed this chat — stop auto title updates.
   * Auto titles keep refreshing until the first manual rename.
   */
  titleLocked?: boolean;
  /**
   * Compacted summary of older turns (API context only; full messages kept).
   */
  summary?: string;
  /** Last message id included in `summary` */
  summaryUpToId?: string | null;
  /** When the summary was last built */
  compactedAt?: number;
  /** How many messages were folded into the summary */
  compactedMessageCount?: number;
};

/** Resume card — only set when a stream was interrupted (Stop / abort). */
export type SessionResume = {
  threadId: string;
  title: string;
  preview: string;
  mode?: GrokModeId;
  ts: number;
  /** Always interrupted — success completions do not create a card */
  kind: "interrupted";
  /** Last user prompt to regenerate / continue from */
  pendingPrompt?: string;
  /** Partial assistant text when stopped */
  partialContent?: string;
  /** Assistant message id that was stopped (if still in thread) */
  stoppedMessageId?: string;
};

export type ImagineAspect = "auto" | "1:1" | "16:9" | "9:16" | "4:3" | "3:2" | "2:3";

export type ImagineMediaKind = "image" | "video";

/** Speed ≈ faster/cheaper draft; Quality ≈ higher fidelity */
export type ImagineQuality = "speed" | "quality";

export type ImagineJob = {
  id: string;
  prompt: string;
  aspect: ImagineAspect;
  ts: number;
  status: "rendering" | "ready" | "failed";
  mode?: GrokModeId;
  imageDataUrl?: string;
  /** video object URL / data url when available */
  videoDataUrl?: string;
  /**
   * Disk-relative path under userData/imagine-media (survives updates).
   * Loaded back into imageDataUrl/videoDataUrl on app start.
   */
  imageRelPath?: string;
  videoRelPath?: string;
  mediaKind?: ImagineMediaKind;
  quality?: ImagineQuality;
  model?: string;
  source?: "xai" | "local";
  error?: string;
  /** optional reference image used for img2img / restyle */
  referenceDataUrl?: string;
};

export type SubscriptionPlanId = "free" | "super" | "pro";

export type UsageBucket = "message" | "imagine" | "automation" | "host" | "skill";

export type UsageSnapshot = {
  plan: SubscriptionPlanId;
  periodStart: number;
  periodEnd: number;
  /** Compute units used this period (token-derived + local host/automation) */
  usedUnits: number;
  messages: number;
  imagine: number;
  automations: number;
  host: number;
  byMode: Record<GrokModeId, number>;
  /** Live token totals from xAI responses this period */
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  /** Last successful usage poll */
  lastPolledAt: number;
  /** How the meter was last updated */
  source: "local" | "live" | "website";
  /** Optional remaining quota from rate-limit headers */
  rateLimitRemaining?: number | null;
  rateLimitLimit?: number | null;
  rateLimitResetAt?: number | null;
  /** Grok website weekly pool (Settings → Usage) */
  website?: {
    planLabel: string;
    creditUsagePercent: number;
    periodType: "weekly" | "monthly" | "unknown";
    periodStart: number | null;
    periodEnd: number | null;
    productUsage: Array<{ product: string; label: string; usagePercent: number }>;
    prepaidBalanceCents: number;
    onDemandCapCents: number;
    onDemandUsedCents: number;
    error?: string | null;
  } | null;
};

/** Profile synced after Grok OAuth / API connect — never seeded with personal defaults. */
export type GrokProfile = {
  displayName: string | null;
  email: string | null;
  imageUrl: string | null;
  models: string[];
  connectedAt: number | null;
};
