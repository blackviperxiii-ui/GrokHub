import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { persistentStorage } from "./persistent-storage";
import { redactSecrets } from "./redact";
import { friendlyAssistantError, formatUserError } from "./format-error";
import { toolResultMarkdown, toolRunningMarkdown, toolLoopWaitMarkdown } from "./tool-status";
import {
  buildContext,
  compactMessages,
  formatContextReport,
  mergeFlushIntoMemory,
  estimateThreadContextPercent,
  CONTEXT_BUDGET_TOKENS,
} from "./context-manager";
import {
  loadMemoryPinBundle,
  memoryAppend,
  memoryAppendFacts,
  memoryRead,
  memoryWrite,
  migrateNotesToFileMemory,
  syncLearningToDisk,
  ensureFileMemory,
} from "./file-memory";
import {
  emptyLearning,
  normalizeLearning,
  learnFromTurn,
  learnFromFeedback,
  reflectLearning,
  learningPinBundle,
  routeLearningBias,
  pruneStaleRoutingInsights,
  learningSummaryLine,
  learningStatusMarkdown,
  learningSnapshotMarkdown,
  pushLearningEvent,
  type LearningState,
} from "./learning";
import {
  emptyWorkboard,
  normalizeWorkboard,
  pinWorkItem as pinWorkItemHelper,
  setWorkItemStatus as setWorkStatusHelper,
  updateWorkItem as updateWorkItemHelper,
  removeWorkItem as removeWorkItemHelper,
  extractWorkCommands,
  workboardContextBlock,
  type WorkboardState,
  type WorkItemStatus,
  type WorkPriority,
} from "./workboard";
import {
  buildProjectSummary,
  projectContextBlock,
  projectNameFromPath,
  type ProjectWorkspace,
} from "./project-workspace";
import { compactMessagesSmart, reflectLearningSmart } from "./smart-compact";
import {
  applyTurnLearning,
  extractMemoryNotes,
  applyAgentMemoryNotes,
} from "./session-learn";
import { renderImaginePreview } from "./imagine";
import { getMode, resolveMode, resolveModeWithCatalog, stripAssistantChrome, modelIdForMode, autoRouteFor, cleanModelOverrides, type ModelModeOverrides } from "./modes";
import { buildCatalog, emptyCatalog, applyGrokPlan, needsGrokClassification, type ResolvedCatalog, type GrokSlotPlan } from "./models-catalog";
import { createSeeds } from "./seed";
import type {
  ActivityItem,
  Agent,
  Automation,
  AutomationSchedule,
  ChatMessage,
  ChatThread,
  SessionResume,
  Connector,
  GrokModeId,
  GrokProfile,
  ImagineAspect,
  ImagineJob,
  ImagineMediaKind,
  ImagineQuality,
  NavId,
  Skill,
  SubscriptionPlanId,
  UsageBucket,
  UsageSnapshot,
  ChatRole,
} from "./types";
import {
  costFor,
  createUsage,
  ensurePeriod,
  inferPlanFromAuth,
  PLAN_LIMITS,
  unitsFromTokens,
  usagePercent,
} from "./usage";
import { uid } from "./utils";
import { computeNextRun } from "./automation-schedule";
import {
  accountKey as setupAccountKey,
  buildSetupPack,
  mergeSetupPack,
  parseSetupPack,
  pullSetupFromGist,
  pushSetupToGist,
  type SetupPack,
} from "./setup-sync";
import {
  emptyQuickAssistMemory,
  normalizeMemory,
  rememberChipClick,
  rememberTypedPrompt,
  rememberChipOutcome,
  rememberChipDismiss,
  topHabitLabels,
  type QuickAssistMemory,
} from "./quick-assist-memory";
import type { QuickChip } from "./quick-assistant";
import {
  buildWelcomePrompt,
  collectWelcomeContext,
  emptyWelcomeFallback,
  parseWelcomePayload,
  type WelcomePayload,
} from "./welcome-message";
import {
  buildChipSuggestPrompt,
  contextFingerprint,
  parseLlmChips,
} from "./quick-assist-llm";
import {
  type AgentJob,
  type AgentQueueState,
  type AutonomyConfig,
  type AutonomyLevel,
  approveJob,
  defaultAutonomyConfig,
  emptyAgentQueue,
  enqueueJob,
  normalizeAgentQueue,
  normalizeAutonomy,
  pickNextJob,
  rollBudgetDay,
  shouldAutoClaimWorkboard,
  shouldAutoGoalResume,
  shouldQueueWhenBusy,
  updateJob,
  queueStats,
  uidJob,
} from "./agent-jobs";
import { formatToolRegistryForPrompt } from "./tool-registry";
import { buildGoalStepPrompt, parseGoalOutcome } from "./goal-loop";
import { agentCoreEnqueue, agentCoreSetPaused, agentCoreSync } from "./agent-core-client";
import {
  scanProactiveIssues,
  markAutoContinue,
  proactiveSystemAddon,
  proactiveEnabled,
  planFreeRoamChores,
  canFreeRoam,
} from "./proactive";
import { runHealthPass, formatHealthMarkdown } from "./health-pass";
import { resolveModeArg, slashHelpMarkdown } from "./slash-commands";

/** In-flight LLM auto-titles (thread id). */
const autoTitleInflight = new Set<string>();
/** Prevent parallel agent queue drains */
let agentQueueDraining = false;
/** Last successful auto-title timestamp per thread */
const autoTitleLastAt = new Map<string, number>();
/** Message count when we last titled */
const autoTitleMsgCount = new Map<string, number>();

/** Waits for user approval of host commands (agent tool loop). */
let hostConfirmWaiter: ((allow: boolean) => void) | null = null;
/** In-flight host exec job id (for Stop → killExec). */
const activeHostJobIds = new Set<string>();
let activeHostJobId: string | null = null; // last job (compat)
/** Serialize post-turn learning so rapid turns don't drop insights or clobber disk */
let learningChain: Promise<void> = Promise.resolve();
function enqueueLearning(task: () => Promise<void>) {
  learningChain = learningChain.then(task).catch(() => {});
  return learningChain;
}

function requestHostConfirm(
  set: (partial: Partial<State> | ((s: State) => Partial<State>)) => void,
  cmds: string[],
  risks: string[],
  botId: string,
): Promise<boolean> {
  return new Promise((resolve) => {
    hostConfirmWaiter = resolve;
    set({
      pendingHostConfirm: { cmds, risks, botId },
      streamStatus: "Waiting for host approval…",
    });
  });
}

type State = {
  nav: NavId;
  mode: GrokModeId;
  modeMenuOpen: boolean;
  connectors: Connector[];
  skills: Skill[];
  automations: Automation[];
  activity: ActivityItem[];
  chat: ChatMessage[];
  threads: ChatThread[];
  activeThreadId: string | null;
  /** Last meaningful work for resume banner */
  sessionResume: SessionResume | null;
  agents: Agent[];
  profile: GrokProfile;
  imagineJobs: ImagineJob[];
  imaginePrompt: string;
  imagineAspect: ImagineAspect;
  imagineMediaKind: ImagineMediaKind;
  imagineQuality: ImagineQuality;
  imagineReference: string | null;
  imagineBusy: boolean;
  imagineError: string | null;
  desktop: {
    startMinimized: boolean;
    launchOnLogin: boolean;
    wayland: boolean;
    tray: boolean;
    /** Electron global accelerator e.g. Super+Space or CommandOrControl+Shift+Space; empty = off */
    globalHotkey: string;
    /** Prompt before running host commands from the agent */
    confirmHostCommands: boolean;
    /** When confirmHostCommands, only prompt for non-read-only commands */
    confirmDestructiveOnly: boolean;
    /** Allow agent SELF_MOD writes under the install tree */
    selfModifyEnabled: boolean;
    /** Block dangerous host shell patterns (rm -rf, sudo, pipe-to-shell, …) */
    hostSafeMode: boolean;
  };
  /** Agent generation / tool preferences */
  agentPrefs: {
    /** 0–1 sampling temperature for chat */
    temperature: number;
    /** Allow HOST_CMD execution from model replies */
    hostToolsEnabled: boolean;
    /** Allow CONNECTOR_CMD execution */
    connectorToolsEnabled: boolean;
    /** User freeform memory notes (persist across restarts) */
    memoryNotes: string;
  };
  /** Host commands awaiting user approval */
  pendingHostConfirm: {
    cmds: string[];
    risks: string[];
    botId: string;
  } | null;
  /** Per-thread composer drafts (survive chat switch + restart) */
  composerDrafts: Record<string, string>;
  setComposerDraft: (threadId: string | null, draft: string) => void;
  /** Host shell history for ↑/↓ recall */
  shellHistory: string[];
  pushShellHistory: (cmd: string) => void;
  /** Always-allow host command prefixes (skip confirm) */
  hostAllowlist: string[];
  addHostAllow: (prefix: string) => void;
  removeHostAllow: (prefix: string) => void;
  /** Soft-delete undo (threads / messages) */
  undoBuffer: {
    kind: "thread" | "messages";
    label: string;
    expiresAt: number;
    thread?: ChatThread;
    messages?: ChatMessage[];
    threadId?: string;
    wasActive?: boolean;
    prevActiveId?: string | null;
    prevChat?: ChatMessage[];
  } | null;
  clearUndoBuffer: () => void;
  undoLastDelete: () => boolean;
  /** Re-run last user turn */
  regenerateLast: () => Promise<void>;
  /** Adaptive quick-assist chip habits */
  quickAssistMemory: QuickAssistMemory;
  /** Chip values/ids the user dismissed */
  quickAssistDismissed: string[];
  /** Bumps to rotate alternate chip packs */
  quickAssistRotation: number;
  /** Fast-mode LLM chip suggestions for current context */
  quickAssistLlmChips: QuickChip[];
  quickAssistLlmAt: number;
  quickAssistLlmTag: string | null;
  quickAssistLlmBusy: boolean;
  /** Adaptive empty-chat welcome (Fast mode + learning) */
  welcomeMessage: WelcomePayload | null;
  welcomeBusy: boolean;
  /** Self-improvement: outcomes, feedback, distilled insights */
  learning: LearningState;
  workboard: WorkboardState;
  projectWorkspace: ProjectWorkspace | null;
  /** Always-on autonomy level + budgets */
  autonomy: AutonomyConfig;
  /** Durable agent job queue */
  agentQueue: AgentQueueState;
  usage: UsageSnapshot;
  heartbeatAt: number;
  running: boolean;
  /** Live status line while agent is working (streaming / host / stopped) */
  streamStatus: string | null;
  /** Id of the assistant message currently streaming */
  streamingMessageId: string | null;
  proactiveNotice: { message: string; at: number } | null;
  /** Live essential models from xAI */
  modelCatalog: ResolvedCatalog;
  /** User pins for mode → model id; unset modes stay auto-classified */
  modelOverrides: ModelModeOverrides;
  lastModelsFetchAt: number;
  /** xAI API key (local only; never sent to third parties except api.x.ai) */
  apiKey: string;
  /** Optional GitHub token for private-repo updates */
  githubToken: string;
  /** xAI Grok OAuth tokens (SuperGrok / X Premium device-code) */
  oauth: import("./xai-oauth").XaiOAuthTokens | null;
  /** grok.com SSO cookie for website Usage (Settings → Usage weekly limit) */
  ssoCookie: string;
  /** Imported OpenClaw workspace metadata + prompt context */
  openClawWorkspace: {
    root: string;
    importedAt: number;
    filesImported: string[];
    contextBundle: string;
    identityName: string | null;
  } | null;
  oauthPending: {
    deviceCode: string;
    userCode: string;
    verificationUri: string;
    verificationUriComplete?: string;
    expiresAt: number;
  } | null;
  grokConnected: boolean | null;
  grokStatusDetail: string;
  setNav: (nav: NavId) => void;
  setMode: (mode: GrokModeId) => void;
  setModeMenuOpen: (open: boolean) => void;
  setModelOverride: (mode: Exclude<GrokModeId, "auto">, modelId: string | null) => void;
  clearModelOverrides: () => void;
  setDesktop: (patch: Partial<State["desktop"]>) => void;
  resolveHostConfirm: (allow: boolean) => void;
  tickAutomations: (opts?: { heartbeatOnly?: boolean }) => Promise<void>;
  hydrateSecrets: () => Promise<void>;
  recordQuickAssistChip: (chip: QuickChip) => void;
  recordQuickAssistTyped: (text: string) => void;
  recordQuickAssistOutcome: (outcome: "success" | "failure") => void;
  clearQuickAssistMemory: () => void;
  rateMessage: (messageId: string, positive: boolean) => void;
  runSelfImprove: () => Promise<{ ok: boolean; detail: string }>;
  clearLearning: () => void;
  flushLearningToDisk: () => Promise<void>;
  pinWorkItem: (input: {
    title: string;
    detail?: string;
    priority?: WorkPriority;
    source?: "agent" | "user";
  }) => void;
  setWorkItemStatus: (idOrTitle: string, status: WorkItemStatus) => void;
  removeWorkItem: (id: string) => void;
  updateWorkItem: (
    id: string,
    patch: Partial<{ title: string; detail: string; priority: WorkPriority; status: WorkItemStatus }>,
  ) => void;
  bindProjectWorkspace: (path: string) => Promise<{ ok: boolean; detail: string }>;
  clearProjectWorkspace: () => void;
  exportDiagnostics: () => Promise<{ ok: boolean; text?: string; error?: string }>;
  dismissQuickAssistChip: (chip: QuickChip) => void;
  rotateQuickAssist: () => void;
  /** Refresh context chips via Fast mode */
  refreshQuickAssistLlm: (opts?: { force?: boolean }) => Promise<void>;
  refreshWelcomeMessage: (opts?: { force?: boolean }) => Promise<void>;
  syncWebsiteConnectors: () => Promise<{ ok: boolean; detail: string; count: number }>;
  setApiKey: (key: string) => void;
  setGithubToken: (token: string) => void;
  startGrokOAuth: () => Promise<void>;
  pollGrokOAuth: () => Promise<"pending" | "ready" | "failed">;
  setupSyncMeta: import("./setup-sync").SetupSyncMeta;
  setSetupSyncMeta: (patch: Partial<import("./setup-sync").SetupSyncMeta>) => void;
  scheduleSetupAutoPush: () => void;
  pushSetupSync: (opts?: { passphrase?: string }) => Promise<{ ok: boolean; detail: string }>;
  pullSetupSync: (opts?: { passphrase?: string }) => Promise<{ ok: boolean; detail: string }>;
  syncSetupWithGrokAccount: (opts?: { passphrase?: string }) => Promise<{ ok: boolean; detail: string }>;
  exportSetupPackJson: (opts?: { passphrase?: string }) => Promise<string>;
  importSetupPackJson: (
    json: string,
    opts?: { passphrase?: string },
  ) => Promise<{ ok: boolean; detail: string }>;
  clearGrokOAuth: () => void;
  setSsoCookie: (cookie: string) => void;
  linkGrokWebsiteSession: () => Promise<{ ok: boolean; detail: string }>;
  importOpenClawWorkspace: (path?: string) => Promise<{
    ok: boolean;
    detail: string;
    skills?: number;
    automations?: number;
  }>;
  clearOpenClawWorkspace: () => void;
  probeGrok: () => Promise<boolean>;
  /** Proactive OAuth refresh (~30m before 6h expiry); persists tokens to secrets. */
  refreshOAuthSession: (opts?: { force?: boolean }) => Promise<{
    ok: boolean;
    refreshed: boolean;
    detail: string;
  }>;
  syncFromGrok: (opts?: { displayName?: string | null; email?: string | null; imageUrl?: string | null }) => Promise<void>;
  newThread: () => void;
  selectThread: (id: string) => void;
  deleteThread: (id: string) => void;
  renameThread: (id: string, title: string) => void;
  /** Sidebar/History: Fast-mode auto title (unlocks manual freeze) */
  autoRenameThread: (id: string) => Promise<void>;
  pinThread: (id: string, pinned?: boolean) => void;
  setThreadFolder: (id: string, folder: string | null) => void;
  dismissSessionResume: () => void;
  updateBanner: { available: boolean; version?: string; detail?: string } | null;
  setUpdateBanner: (v: { available: boolean; version?: string; detail?: string } | null) => void;
  checkUpdateQuiet: () => Promise<void>;
  resumeLastSession: () => void;
  /** After an interrupt: drop partial assistant reply and re-run last user prompt */
  continueInterruptedSession: () => Promise<void>;
  /** Resume a stalled/incomplete agent reply without waiting for interrupt banner */
  keepGoingChat: () => Promise<void>;
  setAgentPrefs: (patch: Partial<{ temperature: number; hostToolsEnabled: boolean; connectorToolsEnabled: boolean; memoryNotes: string }>) => void;
  /** Compact older turns into a summary (API window); full chat kept */
  compactThread: (threadId?: string | null) => { ok: boolean; detail: string };
  /** Live context budget stats for active chat */
  getContextStats: () => { percent: number; tokensEst: number; budget: number; shouldCompact: boolean; report: string };
  editChatMessage: (id: string, content: string, resend?: boolean) => Promise<void>;
  /** Delete one or more messages from the active thread */
  deleteChatMessages: (ids: string | string[]) => void;
  /** Compose a reply quoting a specific message */
  replyTo: { id: string; preview: string; role: ChatRole } | null;
  setReplyTo: (msg: { id: string; content: string; role: ChatRole } | null) => void;
  exportThreadMarkdown: (id?: string) => string;
  clearChat: () => void;
  setPlan: (plan: SubscriptionPlanId) => void;
  /** When true, allow free website session + free-model cascade if paid access fails */
  preferFreeGrok: boolean;
  setPreferFreeGrok: (v: boolean) => void;
  /** App chrome theme */
  uiTheme: "dark" | "light" | "system";
  setUiTheme: (t: "dark" | "light" | "system") => void;
  /** Collapse Tools section in sidebar */
  toolsNavCollapsed: boolean;
  setToolsNavCollapsed: (v: boolean) => void;

  recordUsage: (bucket: UsageBucket, mode?: GrokModeId) => { ok: boolean; cost: number };
  recordTokenUsage: (
    tokens: { prompt_tokens?: number; completion_tokens?: number; total_tokens?: number },
    mode?: GrokModeId,
    rateLimit?: { remaining: number | null; limit: number | null; resetAt: number | null },
  ) => { ok: boolean; cost: number };
  refreshUsage: () => Promise<void>;
  resetUsagePeriod: () => void;
  toggleConnector: (id: string) => void;
  connectConnector: (id: string) => Promise<void>;
  toggleSkill: (id: string) => void;
  addSkill: (input: {
    name: string;
    description: string;
    instructions: string;
    slash: string;
  }) => void;
  runSkill: (id: string) => Promise<void>;
  startWorkItem: (id: string) => Promise<void>;
  setAutonomy: (patch: Partial<AutonomyConfig>) => void;
  pauseAutonomy: (paused: boolean) => void;
  enqueueAgentJob: (
    input: Omit<AgentJob, "id" | "createdAt" | "updatedAt" | "status"> & {
      id?: string;
      status?: AgentJob["status"];
    },
  ) => string;
  cancelAgentJob: (id: string) => void;
  approveAgentJob: (id: string, grant: boolean) => void;
  processAgentQueue: () => Promise<void>;
  runProactiveHousekeeping: () => Promise<{ ok: boolean; detail: string; fixed: number }>;
  dismissProactiveNotice: () => void;
  runHealthCheck: () => Promise<{ ok: boolean; detail: string }>;
  claimWorkboardJobs: () => number;
  /** Internal: run one job through sendChat */
  _runAgentJob: (job: AgentJob) => Promise<void>;
  toggleAutomation: (id: string) => void;
  runAutomation: (id: string) => Promise<void>;
  addAutomation: (input: {
    name: string;
    instructions: string;
    schedule: AutomationSchedule;
    time: string;
    times?: string[];
    heartbeatEveryMin?: number;
  }) => void;
  sendChat: (text: string) => Promise<void>;
  stopChat: () => void;
  refreshModels: (opts?: { force?: boolean }) => Promise<void>;
  setImaginePrompt: (v: string) => void;
  setImagineAspect: (v: ImagineAspect) => void;
  setImagineMediaKind: (v: ImagineMediaKind) => void;
  setImagineQuality: (v: ImagineQuality) => void;
  setImagineReference: (v: string | null) => void;
  runImagine: (prompt?: string) => Promise<void>;
  /** Remove one generated image/video from the Imagine gallery */
  removeImagineJob: (id: string) => void;
  /** Clear all Imagine gallery items */
  clearImagineJobs: () => void;
  pushActivity: (item: Omit<ActivityItem, "id" | "ts"> & { ts?: number }) => void;
  tickHeartbeat: () => void;
  setAgentStatus: (id: string, status: Agent["status"], tasks?: number) => void;
  resetDemo: () => void;
  refreshStaleTimes: () => void;
};

function replyFor(text: string, s: State, routed: GrokModeId): string {
  /** Offline-only honest status — never invents inbox/calendar/Linear data. */
  const lower = text.toLowerCase();
  const connected = s.connectors.filter((c) => c.status === "connected");
  const liveConnected = connected.filter((c) => c.liveTools);
  const statusOnly = connected.filter((c) => !c.liveTools);
  const enabledSkills = s.skills.filter((sk) => sk.enabled);
  const plan = PLAN_LIMITS[s.usage.plan];
  const pct = Math.round(usagePercent(s.usage));
  const m = getMode(routed);

  if (
    lower.includes("usage") ||
    lower.includes("quota") ||
    lower.includes("limit") ||
    lower.includes("subscription")
  ) {
    return [
      "Subscription usage (local meter)",
      "",
      `Plan: ${plan.label}`,
      `Units: ${s.usage.usedUnits.toFixed(1)} / ${plan.units} (${pct}%)`,
      `Messages ${s.usage.messages}/${plan.messages} · Imagine ${s.usage.imagine}/${plan.imagine}`,
      `Automations ${s.usage.automations}/${plan.automations} · Host ${s.usage.host}/${plan.host}`,
      "",
      "Open Settings → Usage for website pool details when Grok website is linked.",
    ].join("\n");
  }

  if (lower.includes("imagine") || lower.startsWith("/imagine")) {
    return [
      "Imagine",
      "",
      "Open the Imagine tab to generate images or video.",
      `Quota this period: ${s.usage.imagine}/${plan.imagine} (5 units each).`,
      "Tip: describe a scene in chat while Adaptive is on — GrokHub will switch to Imagine when the ask is visual.",
    ].join("\n");
  }

  if (
    lower.includes("mode") ||
    lower.includes("fast") ||
    lower.includes("expert") ||
    lower.includes("heavy") ||
    lower.includes("adaptive")
  ) {
    return [
      "Modes",
      "",
      "- Adaptive — routes ⚡ Fast · 🧠 Think · 🔬 Deep · 🛠️ Build",
      "- Fast / Expert / Heavy / Build — manual lock",
      "",
      `Active: ${getMode(s.mode).label}${s.mode === "auto" ? ` (this turn → ${m.label})` : ""}`,
    ].join("\n");
  }

  if (lower.includes("connector") || lower.includes("connect")) {
    return [
      "Connectors",
      "",
      "Live tools (agent can call):",
      ...(liveConnected.length
        ? liveConnected.map((c) => `- ${c.name}: connected · ${c.tools.slice(0, 4).join(", ")}`)
        : ["- (none connected)"]),
      "",
      "Website status only (not executable from this app yet):",
      ...(statusOnly.length
        ? statusOnly.map((c) => `- ${c.name}: ${c.status}`)
        : ["- (none)"]),
      "",
      "GitHub PAT + Grok OAuth/API power live tools. Gmail/Drive/etc. show link status from grok.com when website SSO is linked.",
    ].join("\n");
  }

  if (lower.includes("automat") || lower.includes("schedule")) {
    return [
      "Automations",
      "",
      ...s.automations.map(
        (a) =>
          `- ${a.enabled ? "ON" : "OFF"} ${a.name} (${a.schedule}${a.failCount ? ` · ${a.failCount} fails` : ""}) · ${a.runCount} runs`,
      ),
      s.automations.length ? "" : "(none)",
    ]
      .filter((line, i, arr) => line !== "" || i < arr.length - 1)
      .join("\n");
  }

  if (lower.includes("skill")) {
    return [
      "Skills",
      "",
      ...enabledSkills.map((sk) => `- ${sk.slash} — ${sk.name}`),
      "",
      "Running a skill sends a real agent turn with that skill’s instructions (tools allowed).",
    ].join("\n");
  }

  if (lower.includes("workboard") || lower.includes("work board") || lower.startsWith("/board")) {
    const open = (s.workboard?.items || []).filter(
      (i) => !["done", "dismissed"].includes(i.status),
    );
    return [
      "Workboard",
      "",
      `Open items: ${open.length}`,
      ...open.slice(0, 8).map((i) => `- [${i.status}] ${i.title}`),
      open.length ? "" : "Pin tasks with WORK_PIN or the Workboard tab.",
    ]
      .filter(Boolean)
      .join("\n");
  }

  // Generic offline status (no fictional calendar/inbox)
  return [
    "GrokHub offline status",
    "",
    `Mode this turn: ${m.label}`,
    `Usage: ${pct}% of ${plan.label}`,
    `Live connectors: ${liveConnected.map((c) => c.name).join(", ") || "none"}`,
    `Skills enabled: ${enabledSkills.length}`,
    `Open workboard items: ${(s.workboard?.items || []).filter((i) => !["done", "dismissed"].includes(i.status)).length}`,
    "",
    "Connect Grok (OAuth, free website, or API key) for full agent replies.",
    "Local slash helpers never invent inbox, calendar, or Linear data.",
  ].join("\n");
}


function emptyProfile(): GrokProfile {
  return {
    displayName: null,
    email: null,
    imageUrl: null,
    models: [],
    connectedAt: null,
  };
}

function titleFromMessages(messages: ChatMessage[]): string {
  const clean = (raw: string) =>
    raw
      .replace(/\[attachment:[^\]]+\]/gi, " ")
      .replace(/\[Replying to[^\]]*\]:\s*"[^"]*"\s*/gi, " ")
      .replace(/```[\s\S]*?```/g, " ")
      .replace(/`[^`]+`/g, " ")
      .replace(/https?:\/\/\S+/gi, " ")
      .replace(/^\/[a-z]+\s*/i, "")
      .replace(/\bHOST_CMD:\s*/gi, " ")
      .replace(/\bCONNECTOR_CMD:\s*/gi, " ")
      .replace(/^\$\s*/gm, " ")
      .replace(/[_*#>|[\](){}]/g, " ")
      .replace(/\s+/g, " ")
      .trim();

  const isThin = (s: string) =>
    !s ||
    s.length < 8 ||
    /^(hi|hello|hey|ok|okay|thanks|thank you|yo|sup|continue|go on|yes|no|sure|please|help|test|hmm+|uh+|um+)[.!?]*$/i.test(
      s,
    );

  const users = messages
    .filter((m) => m.role === "user")
    .map((m) => clean(m.content))
    .filter((s) => s && !isThin(s));

  if (!users.length) return "New chat";

  // First substantive user message is usually the thread topic
  return summarizeChatTitle(users[0]!, users.slice(1, 4));
}

/** Super-short topic label for the sidebar (≈2–5 words). */
function summarizeChatTitle(primary: string, extras: string[] = []): string {
  let s = primary
    .replace(
      /^(can you|could you|would you|please|hey|hi|hello|ok so|so+|um+|uh+|lets|let'?s)\s+/i,
      "",
    )
    .replace(
      /^(i (want|need|would like) (you )?to|help me|please)\s+/i,
      "",
    )
    .replace(/\b(please|thanks|thank you)\b/gi, " ")
    .replace(/\?+$/g, "")
    .replace(/\s+/g, " ")
    .trim();

  // Pattern → short labels (prefer start-of-prompt intents)
  const patterns: Array<[RegExp, (m: RegExpMatchArray) => string]> = [
    [
      /^(fix|debug|repair)\s+(?:the\s+)?(.+?)(?:\s+(?:bug|issue|error|problem))?$/i,
      (m) => `Fix ${clipTitleWords(m[2]!, 3)}`,
    ],
    [
      /^(build|create|make|add)\s+(?:a\s+|an\s+|the\s+)?(.+)$/i,
      (m) => `${softTitleCase(m[1]!)} ${clipTitleWords(m[2]!, 3)}`,
    ],
    [
      /^(?:deep dive|investigate|diagnos\w*)\s+(?:into\s+|on\s+|for\s+)?(?:the\s+)?(.+)$/i,
      (m) => `${clipTitleWords(m[1]!, 3)} dive`,
    ],
    [
      /^how (?:do|to|can) (?:i|we|you)\s+(.+)$/i,
      (m) => `How: ${clipTitleWords(m[1]!, 3)}`,
    ],
    [/^why\s+(?:does\s+|is\s+|do\s+)?(.+)$/i, (m) => `Why ${clipTitleWords(m[1]!, 3)}`],
    [
      /\b(push|publish)\b.*\b(github|release|update)\b/i,
      () => "GitHub push",
    ],
    [
      /\b(auto.?renam\w*|chat title|sidebar title)\b/i,
      () => "Chat titles",
    ],
    [
      /\b(usage meter|imagine tab|oauth|connectors?|automations?|adaptive mode|voice chat|taskbar|streaming|install|readme)\b/i,
      (m) => softTitleCase(m[1]!),
    ],
  ];
  for (const [re, fn] of patterns) {
    const m = s.match(re);
    if (m) {
      const label = stripTitleTrail(fn(m).replace(/\s+/g, " ").trim());
      if (label.length >= 3) return finalizeChatTitle(label);
    }
  }

  // Keyword bag from primary + later user msgs
  const bag = [s, ...extras].join(" ");
  const stop = new Set(
    (
      "a an the and or but if then so to of in on for with from at by as is are was were be been being " +
      "i me my we you your it its this that these those do does did can could would should will just " +
      "like also very really about into over out up down not no yes please help me my our their what " +
      "when where who how why which than now still something anything everything nothing more most " +
      "some any all get got need want try make sure also asked agent why off onto upon via per"
    ).split(" "),
  );
  const tokens = bag
    .toLowerCase()
    .replace(/[^a-z0-9.\-_\s]/gi, " ")
    .split(/\s+/)
    .filter((w) => w.length > 1 && !stop.has(w) && !/^\d+$/.test(w));

  const seen = new Set<string>();
  const picked: string[] = [];
  for (const w of tokens) {
    if (seen.has(w)) continue;
    seen.add(w);
    picked.push(w);
    if (picked.length >= 4) break;
  }

  if (!picked.length) {
    return finalizeChatTitle(clipTitleWords(s, 4) || "Chat");
  }
  return finalizeChatTitle(
    stripTitleTrail(picked.map((w) => softTitleCase(w)).join(" ")),
  );
}

function clipTitleWords(s: string, n: number): string {
  const stopEnd = new Set(
    "a an the and or for with from to of in on at by as is are was be my your our their please".split(
      " ",
    ),
  );
  const parts = s
    .replace(/[^a-z0-9.\-_\s]/gi, " ")
    .split(/\s+/)
    .filter(Boolean);
  const out = parts.slice(0, n);
  while (out.length > 1 && stopEnd.has(out[out.length - 1]!.toLowerCase())) {
    out.pop();
  }
  return out.join(" ");
}

function stripTitleTrail(s: string): string {
  return s
    .replace(
      /\b(and|or|for|with|from|to|of|in|on|at|by|the|a|an|please|my|your)\s*$/i,
      "",
    )
    .trim();
}

function softTitleCase(w: string): string {
  const x = w.trim();
  if (!x) return x;
  if (/^(api|ui|ux|cli|cpu|gpu|ssh|oauth|http|https|json|css|html|sql|aur)$/i.test(x)) {
    return x.toUpperCase();
  }
  if (/^[A-Z0-9.\-_]+$/.test(x) && x.length <= 6) return x;
  // multi-word soft title
  if (/\s/.test(x)) {
    return x
      .split(/\s+/)
      .map((p) => softTitleCase(p))
      .join(" ");
  }
  return x.charAt(0).toUpperCase() + x.slice(1).toLowerCase();
}

function finalizeChatTitle(label: string): string {
  let title = stripTitleTrail(label.replace(/\s+/g, " ").trim());
  // hard cap ~28 chars — super short sidebar label
  if (title.length > 28) {
    const cut = title.slice(0, 28);
    const sp = cut.lastIndexOf(" ");
    title = stripTitleTrail((sp > 10 ? cut.slice(0, sp) : cut).trimEnd());
  }
  title = title.replace(/[,:;.\-–—]+$/g, "").trim();
  if (title.length < 2) return "Chat";
  return title;
}


/** Build a tiny transcript for fast-mode title naming. */
function buildTitleTranscript(messages: ChatMessage[], maxTurns = 6): string {
  const lines: string[] = [];
  const slice = messages.filter((m) => m.role === "user" || m.role === "assistant").slice(-maxTurns);
  for (const m of slice) {
    const who = m.role === "user" ? "User" : "Assistant";
    const body = String(m.content || "")
      .replace(/```[\s\S]*?```/g, "[code]")
      .replace(/\s+/g, " ")
      .trim()
      .slice(0, 280);
    if (!body) continue;
    lines.push(`${who}: ${body}`);
  }
  return lines.join("\n");
}

/**
 * Use Fast mode for a super-short chat name (summary → 2–5 words).
 * Respects titleLocked unless opts.force (user-initiated from sidebar).
 */
async function autoRenameThreadWithFast(
  get: () => State,
  set: (partial: Partial<State> | ((s: State) => Partial<State>)) => void,
  threadId: string | null | undefined,
  opts?: { force?: boolean },
): Promise<void> {
  if (!threadId) return;
  const force = Boolean(opts?.force);
  const th = get().threads.find((x) => x.id === threadId);
  if (!th) return;
  if (th.titleLocked && !force) return;
  const msgs = th.id === get().activeThreadId ? get().chat : th.messages || [];
  const users = msgs.filter((m) => m.role === "user");
  const assts = msgs.filter((m) => m.role === "assistant" && !m.streaming);
  if (!users.length) return;
  if (!force && !assts.length) return;

  // Don't spam: only re-title when new content arrived (or still "New chat")
  const n = msgs.length;
  const lastN = autoTitleMsgCount.get(threadId) || 0;
  const title = (th.title || "").trim();
  const stillGeneric =
    !title ||
    /^new chat$/i.test(title) ||
    title.length < 3 ||
    /^chat$/i.test(title);
  if (!force) {
    if (!stillGeneric && n - lastN < 2) return;
    const lastAt = autoTitleLastAt.get(threadId) || 0;
    if (Date.now() - lastAt < 12_000 && !stillGeneric) return;
  }
  if (autoTitleInflight.has(threadId)) return;

  const can =
    Boolean(get().oauth?.accessToken || get().apiKey || get().ssoCookie);
  if (!can) return;

  // User-initiated auto-name unlocks manual freeze
  if (force && th.titleLocked) {
    set((s) => ({
      threads: s.threads.map((x) =>
        x.id === threadId ? { ...x, titleLocked: false } : x,
      ),
    }));
  }

  autoTitleInflight.add(threadId);
  try {
    const transcript = buildTitleTranscript(msgs, 8);
    if (!transcript.trim()) return;

    const { grokChat } = await import("./grok-client");
    const { pickSlotModel } = await import("./models-catalog");
    const catalog = get().modelCatalog;
    const fastModel =
      catalog?.slots?.fast ||
      pickSlotModel("fast", catalog?.all || []) ||
      undefined;

    const prompt = [
      "Summarize what this chat is about in your head, then output ONLY a super short chat title.",
      "Rules:",
      "- 2 to 5 words max",
      "- No quotes, no trailing punctuation, no emojis",
      "- No phrases like Chat about / Discussion of / Conversation",
      "- Prefer a concrete topic (feature, bug, file, product)",
      "- Title case preferred",
      "",
      "Transcript:",
      transcript,
      "",
      "Title:",
    ].join("\n");

    const result = await grokChat({
      messages: [{ role: "user", content: prompt }],
      mode: "fast",
      model: fastModel,
      apiKey: get().apiKey || undefined,
      accessToken: get().oauth?.accessToken,
      tokens: get().oauth,
      ssoCookie: get().ssoCookie || undefined,
      freeTier: false,
    });

    if (!result.ok || !result.content) return;
    // Re-check lock — user may have manually renamed mid-flight
    const live = get().threads.find((x) => x.id === threadId);
    if (!live) return;
    if (live.titleLocked) return;

    let raw =
      String(result.content || "")
        .split("\n")
        .map((l) => l.trim())
        .find((l) => l && !/^title\s*:/i.test(l)) || String(result.content);
    raw = raw
      .replace(/^title\s*:\s*/i, "")
      .replace(/^["'“”]+|["'“”]+$/g, "")
      .replace(/\s+/g, " ")
      .trim();
    const next = finalizeChatTitle(raw);
    if (next.length < 2 || /^new chat$/i.test(next)) return;

    set((s) => ({
      threads: s.threads.map((x) =>
        x.id === threadId
          ? {
              ...x,
              title: next,
              titleLocked: false,
              updatedAt: Date.now(),
            }
          : x,
      ),
    }));
    autoTitleLastAt.set(threadId, Date.now());
    autoTitleMsgCount.set(threadId, n);
  } catch {
    /* keep heuristic title */
  } finally {
    autoTitleInflight.delete(threadId);
  }
}

/** Update thread messages (+ auto title unless user locked it). */
function threadWithMessages(
  th: ChatThread,
  messages: ChatMessage[],
  extra: Partial<ChatThread> = {},
): ChatThread {
  const next: ChatThread = {
    ...th,
    ...extra,
    messages,
    updatedAt: Date.now(),
  };
  if (!th.titleLocked) {
    next.title = titleFromMessages(messages);
  }
  return next;
}

function initialFromSeeds() {
  const s = createSeeds();
  return {
    connectors: s.connectors,
    skills: s.skills,
    automations: s.automations,
    activity: s.activity,
    chat: s.chat,
    threads: s.threads,
    activeThreadId: s.activeThreadId,
    agents: s.agents,
    heartbeatAt: s.heartbeatAt,
    profile: emptyProfile(),
  };
}

const boot = initialFromSeeds();

/** Flush debounced/parked memory so Settings stick across restart/update. */
function scheduleSettingsPersist() {
  void import("./persistent-storage").then(({ flushPersistentStorage }) => {
    void flushPersistentStorage();
  });
}

const CORE_CONNECTOR_IDS = new Set(["grok-xai", "desktop-host", "github"]);

function pruneToCoreConnectors(list: Connector[]): Connector[] {
  const byId = new Map<string, Connector>();
  for (const c of list || []) {
    if (CORE_CONNECTOR_IDS.has(c.id)) byId.set(c.id, c);
  }
  // Ensure cores always exist
  for (const seed of createSeeds().connectors) {
    if (!byId.has(seed.id)) byId.set(seed.id, seed);
  }
  return ["grok-xai", "desktop-host", "github"]
    .map((id) => byId.get(id)!)
    .filter(Boolean);
}

export const useGrokHub = create<State>()(
  persist(
    (set, get) => ({
      nav: "chat",
      mode: "auto",
      modeMenuOpen: false,
      connectors: boot.connectors,
      skills: boot.skills,
      automations: boot.automations,
      activity: boot.activity,
      chat: boot.chat,
      threads: boot.threads,
      activeThreadId: boot.activeThreadId,
      sessionResume: null,
      updateBanner: null,
      replyTo: null,
      agents: boot.agents,
      profile: boot.profile,
      imagineJobs: [],
      imaginePrompt: "",
      imagineAspect: "auto",
      imagineMediaKind: "image",
      imagineQuality: "speed",
      imagineReference: null,
      imagineBusy: false,
      imagineError: null,
      desktop: {
        startMinimized: false,
        launchOnLogin: false,
        wayland: true,
        tray: true,
        globalHotkey: "Super+Space",
        confirmHostCommands: true,
        confirmDestructiveOnly: true,
        selfModifyEnabled: false,
        hostSafeMode: false,
      },
      agentPrefs: {
        temperature: 0.7,
        hostToolsEnabled: true,
        connectorToolsEnabled: true,
        memoryNotes: "",
      },
      usage: createUsage("pro"),
      heartbeatAt: boot.heartbeatAt,
      running: false,
      streamStatus: null,
      streamingMessageId: null,
      proactiveNotice: null,
      pendingHostConfirm: null,
      composerDrafts: {},
      shellHistory: [],
      hostAllowlist: [],
      undoBuffer: null,
      quickAssistMemory: emptyQuickAssistMemory(),
      quickAssistDismissed: [],
      quickAssistRotation: 0,
      quickAssistLlmChips: [],
      quickAssistLlmAt: 0,
      quickAssistLlmTag: null,
      quickAssistLlmBusy: false,
      welcomeMessage: null,
      welcomeBusy: false,
      learning: emptyLearning(),
      workboard: emptyWorkboard(),
      projectWorkspace: null,
      autonomy: defaultAutonomyConfig(),
      agentQueue: emptyAgentQueue(),
      modelCatalog: emptyCatalog(),
      modelOverrides: {},
      lastModelsFetchAt: 0,
      apiKey: "",
      githubToken: "",
      oauth: null,
      ssoCookie: "",
      openClawWorkspace: null,
      oauthPending: null,
      setupSyncMeta: { autoPullOnLogin: true, autoPushOnChange: false },
      grokConnected: null,
      preferFreeGrok: false,
      uiTheme: "dark" as const,
      toolsNavCollapsed: false,
      grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",

      setNav: (nav) => {
        set({ nav, modeMenuOpen: false });
        scheduleSettingsPersist();
      },
      setMode: (mode) => {
        set({ mode, modeMenuOpen: false });
        scheduleSettingsPersist();
        get().pushActivity({
          kind: "system",
          title: `Mode → ${getMode(mode).label}`,
          detail: getMode(mode).subtitle,
          status: "success",
        });
      },
      setModeMenuOpen: (open) => set({ modeMenuOpen: open }),
      setModelOverride: (mode, modelId) => {
        set((s) => {
          const next = { ...cleanModelOverrides(s.modelOverrides) };
          if (modelId && modelId.trim()) next[mode] = modelId.trim();
          else delete next[mode];
          return { modelOverrides: next };
        });
        scheduleSettingsPersist();
      },
      clearModelOverrides: () => {
        set({ modelOverrides: {} });
        scheduleSettingsPersist();
      },
      setDesktop: (patch) => {
        set((s) => ({ desktop: { ...s.desktop, ...patch } }));
        scheduleSettingsPersist();
        if (typeof patch.hostSafeMode === "boolean" && typeof window !== "undefined") {
          try {
            void window.grokhubDesktop?.host?.setSafeMode?.(patch.hostSafeMode);
          } catch {
            /* ignore */
          }
        }
        if ("globalHotkey" in patch && typeof window !== "undefined") {
          try {
            const accel = String(patch.globalHotkey || "").trim() || "off";
            void window.grokhubDesktop?.setGlobalHotkey?.(accel);
          } catch {
            /* ignore */
          }
        }
        get().scheduleSetupAutoPush();
      },

      resolveHostConfirm: (allow) => {
        const pending = hostConfirmWaiter;
        hostConfirmWaiter = null;
        set({ pendingHostConfirm: null });
        pending?.(allow);
      },

      setComposerDraft: (threadId, draft) => {
        const id = threadId || get().activeThreadId;
        if (!id) return;
        set((s) => {
          const next = { ...s.composerDrafts };
          if (!draft) delete next[id];
          else next[id] = draft.slice(0, 50_000);
          // cap keys
          const keys = Object.keys(next);
          if (keys.length > 60) {
            for (const k of keys.slice(0, keys.length - 60)) delete next[k];
          }
          return { composerDrafts: next };
        });
      },

      pushShellHistory: (cmd) => {
        const c = cmd.trim();
        if (!c) return;
        set((s) => {
          const prev = (s.shellHistory || []).filter((x) => x !== c);
          return { shellHistory: [c, ...prev].slice(0, 80) };
        });
      },

      addHostAllow: (prefix) => {
        const p = prefix.trim();
        if (!p) return;
        set((s) => {
          if (s.hostAllowlist.includes(p)) return s;
          return { hostAllowlist: [...s.hostAllowlist, p].slice(0, 40) };
        });
      },

      removeHostAllow: (prefix) => {
        set((s) => ({
          hostAllowlist: s.hostAllowlist.filter((x) => x !== prefix),
        }));
      },

      clearUndoBuffer: () => set({ undoBuffer: null }),

      undoLastDelete: () => {
        const u = get().undoBuffer;
        if (!u || u.expiresAt < Date.now()) {
          set({ undoBuffer: null });
          return false;
        }
        if (u.kind === "thread" && u.thread) {
          set((s) => ({
            threads: [u.thread!, ...s.threads.filter((t) => t.id !== u.thread!.id)],
            activeThreadId: u.wasActive ? u.thread!.id : s.activeThreadId,
            chat: u.wasActive ? u.thread!.messages || [] : s.chat,
            undoBuffer: null,
          }));
          get().pushActivity({
            kind: "system",
            title: "Restored chat",
            detail: u.thread.title,
            status: "success",
          });
          return true;
        }
        if (u.kind === "messages" && u.messages?.length && u.threadId) {
          set((s) => {
            const tid = u.threadId!;
            const restore = u.messages!;
            const chat =
              s.activeThreadId === tid
                ? (() => {
                    const byId = new Map(s.chat.map((m) => [m.id, m]));
                    for (const m of restore) byId.set(m.id, m);
                    // stable-ish order by createdAt
                    return [...byId.values()].sort(
                      (a, b) => (a.ts || 0) - (b.ts || 0),
                    );
                  })()
                : s.chat;
            return {
              chat,
              threads: s.threads.map((th) => {
                if (th.id !== tid) return th;
                const byId = new Map((th.messages || []).map((m) => [m.id, m]));
                for (const m of restore) byId.set(m.id, m);
                return {
                  ...th,
                  messages: [...byId.values()].sort(
                    (a, b) => (a.ts || 0) - (b.ts || 0),
                  ),
                  updatedAt: Date.now(),
                };
              }),
              undoBuffer: null,
            };
          });
          get().pushActivity({
            kind: "chat",
            title: "Messages restored",
            detail: `${u.messages.length} message(s)`,
            status: "success",
          });
          return true;
        }
        set({ undoBuffer: null });
        return false;
      },

      regenerateLast: async () => {
        if (get().running) return;
        const chat = get().chat;
        let userIdx = -1;
        for (let i = chat.length - 1; i >= 0; i--) {
          if (chat[i]?.role === "user") {
            userIdx = i;
            break;
          }
        }
        if (userIdx < 0) return;
        const content = String(chat[userIdx]!.content || "");
        if (!content.trim()) return;
        // Drop the user turn + everything after so sendChat re-adds a clean user message
        const kept = chat.slice(0, userIdx);
        set((s) => ({
          chat: kept,
          threads: s.threads.map((th) =>
            th.id === s.activeThreadId ? threadWithMessages(th, kept) : th,
          ),
          replyTo: null,
        }));
        await get().sendChat(content);
      },

      hydrateSecrets: async () => {
        try {
          const { loadAllSecrets } = await import("./secrets-client");
          const sec = await loadAllSecrets();
          const patch: Partial<State> = {};
          if (sec.apiKey) patch.apiKey = sec.apiKey;
          if (sec.githubToken) patch.githubToken = sec.githubToken;
          if (sec.ssoCookie) patch.ssoCookie = sec.ssoCookie;
          if (sec.oauth) {
            try {
              patch.oauth = JSON.parse(sec.oauth);
            } catch {
              /* ignore */
            }
          }
          // Recover website session from Electron partition if secrets empty
          if (
            !patch.ssoCookie &&
            typeof window !== "undefined" &&
            window.grokhubDesktop?.grok?.getWebsiteSso
          ) {
            try {
              const sso = await window.grokhubDesktop.grok.getWebsiteSso();
              if (sso?.cookie) {
                patch.ssoCookie = sso.cookie;
                void import("./secrets-client").then((m) =>
                  m.secretsSet("ssoCookie", sso.cookie!),
                );
              }
            } catch {
              /* ignore */
            }
          }
          if (Object.keys(patch).length) set(patch);
        } catch {
          /* ignore */
        }
      },

      recordQuickAssistChip: (chip) => {
        const tag = contextFingerprint(get().chat, get().activity);
        set((s) => ({
          quickAssistMemory: rememberChipClick(s.quickAssistMemory, chip, tag),
          quickAssistRotation: (s.quickAssistRotation || 0) + 1,
        }));
      },

      recordQuickAssistTyped: (text) => {
        const trimmed = text.trim();
        if (!trimmed) return;
        set((s) => ({
          quickAssistMemory: rememberTypedPrompt(s.quickAssistMemory, trimmed),
        }));
      },

      recordQuickAssistOutcome: (outcome) => {
        set((s) => ({
          quickAssistMemory: rememberChipOutcome(s.quickAssistMemory, outcome),
        }));
      },

      clearQuickAssistMemory: () => {
        set({
          quickAssistMemory: emptyQuickAssistMemory(),
          quickAssistDismissed: [],
          quickAssistRotation: 0,
          quickAssistLlmChips: [],
          quickAssistLlmAt: 0,
          quickAssistLlmTag: null,
        });
      },

      rateMessage: (messageId, positive) => {
        const s = get();
        const msg = s.chat.find((m) => m.id === messageId);
        if (!msg || msg.role !== "assistant") return;
        set({
          learning: learnFromFeedback(s.learning, {
            positive,
            messagePreview: msg.content || "",
            routeTier: msg.routeTier,
            mode: msg.mode,
            threadId: s.activeThreadId || undefined,
          }),
        });
        get().pushActivity({
          kind: "chat",
          title: positive ? "Marked helpful" : "Marked unhelpful",
          detail: (msg.content || "").slice(0, 80),
          status: "success",
        });
        void get().flushLearningToDisk();
      },

      runSelfImprove: async () => {
        const s = get();
        const online = Boolean(s.oauth?.accessToken || s.apiKey);
        let learning = s.learning;
        try {
          const { extractSessionSignals } = await import("./session-learn");
          const { upsertInsight } = await import("./learning");
          const recent = s.chat
            .filter((m) => m.role === "user" || m.role === "assistant")
            .slice(-16);
          for (let i = 0; i < recent.length; i++) {
            const u = recent[i];
            if (u?.role !== "user") continue;
            const a = recent[i + 1]?.role === "assistant" ? recent[i + 1] : null;
            const sig = extractSessionSignals(u.content || "", a?.content || "");
            for (const p of [
              ...sig.prefs,
              ...sig.facts,
              ...sig.topics.map((t) => `Focus: ${t}`),
            ]) {
              learning = upsertInsight(learning, {
                key: `reflect-chat:${p.toLowerCase().slice(0, 40)}`,
                text: p,
                confidence: 0.55,
                source: "distill",
              });
            }
            if (sig.prefs.length) {
              await memoryAppendFacts(sig.prefs, { target: "USER.md" });
            }
            if (sig.facts.length) {
              await memoryAppendFacts(sig.facts, { target: "MEMORY.md" });
            }
          }
        } catch {
          /* ignore */
        }
        const { state, markdown } = await reflectLearningSmart(learning, {
          online,
          bearer: s.oauth?.accessToken,
          apiKey: s.apiKey,
        });
        set({ learning: state });
        try {
          await memoryWrite("LEARNINGS.md", markdown);
          const tops = state.insights.slice(0, 8).map((i) => i.text);
          if (tops.length) await memoryAppendFacts(tops, { target: "MEMORY.md" });
          await memoryAppend(
            "today",
            `Full reflect · ${state.insights.length} insights · ${state.totalTurns} turns`,
          );
        } catch {
          /* browser ok */
        }
        get().pushActivity({
          kind: "chat",
          title: "Self-improve complete",
          detail: learningSummaryLine(state),
          status: "success",
        });
        void get().flushLearningToDisk();
        return {
          ok: true,
          detail: `Reflected into LEARNINGS.md + MEMORY.md · ${learningSummaryLine(state)}`,
        };
      },

      clearLearning: () => {
        set({ learning: emptyLearning() });
        void get().flushLearningToDisk();
      },

      flushLearningToDisk: async () => {
        try {
          try {
            const pruned = pruneStaleRoutingInsights(get().learning);
            if (pruned !== get().learning) set({ learning: pruned });
          } catch {
            /* ignore */
          }
          await ensureFileMemory();
          let root: string | undefined;
          let userData: string | undefined;
          try {
            const info = await window.grokhubDesktop?.memory?.info?.();
            root = info?.root;
            userData = info?.userData;
          } catch {
            /* ignore */
          }
          const L = get().learning;
          await syncLearningToDisk({
            statusMarkdown: learningStatusMarkdown(L, { root, userData }),
            learningsMarkdown: learningSnapshotMarkdown(L),
          });
        } catch {
          /* ignore */
        }
      },

      pinWorkItem: (input) => {
        const s = get();
        const { state, item } = pinWorkItemHelper(s.workboard, {
          ...input,
          threadId: s.activeThreadId,
          projectPath: s.projectWorkspace?.path || null,
        });
        set({ workboard: state });
        get().pushActivity({
          kind: "agent",
          title: "Workboard pin",
          detail: item.title,
          status: "success",
        });
      },

      setWorkItemStatus: (idOrTitle, status) => {
        set({ workboard: setWorkStatusHelper(get().workboard, idOrTitle, status) });
      },

      updateWorkItem: (id, patch) => {
        set({ workboard: updateWorkItemHelper(get().workboard, id, patch) });
      },

      removeWorkItem: (id) => {
        set({ workboard: removeWorkItemHelper(get().workboard, id) });
      },

      bindProjectWorkspace: async (path) => {
        let root = String(path || "").trim();
        if (root.startsWith("~/")) {
          try {
            const { hostInfo } = await import("./host-client");
            const h = await hostInfo();
            const home = h?.homedir || "";
            if (home) root = home.replace(/\/$/, "") + root.slice(1);
          } catch {
            /* leave as-is */
          }
        }
        if (!root) return { ok: false, detail: "Path required" };
        try {
          const summary = await buildProjectSummary(root);
          const ws = {
            path: root,
            name: projectNameFromPath(root),
            summary,
            boundAt: Date.now(),
            threadId: get().activeThreadId,
          };
          set({ projectWorkspace: ws });
          scheduleSettingsPersist();
          get().pushActivity({
            kind: "desktop",
            title: "Project bound",
            detail: root,
            status: "success",
          });
          return { ok: true, detail: `Bound ${ws.name}` };
        } catch (e) {
          return {
            ok: false,
            detail: e instanceof Error ? e.message : "bind failed",
          };
        }
      },

      clearProjectWorkspace: () => {
        set({ projectWorkspace: null });
        scheduleSettingsPersist();
      },

      exportDiagnostics: async () => {
        const { copyDiagnostics } = await import("./diagnostics");
        const { learningSummaryLine } = await import("./learning");
        const { workboardCounts } = await import("./workboard");
        const counts = workboardCounts(get().workboard);
        const open =
          counts.proposed + counts.approved + counts.staged + counts.in_progress;
        return copyDiagnostics({
          learningLine: learningSummaryLine(get().learning),
          workboardOpen: open,
        });
      },

      dismissQuickAssistChip: (chip) => {
        const key = (chip.value || chip.id || "").trim();
        if (!key) return;
        set((s) => ({
          quickAssistMemory: rememberChipDismiss(s.quickAssistMemory, chip),
          quickAssistDismissed: [...new Set([...(s.quickAssistDismissed || []), key, chip.id])].slice(
            -60,
          ),
          quickAssistRotation: (s.quickAssistRotation || 0) + 1,
          // Drop matching LLM chips immediately
          quickAssistLlmChips: (s.quickAssistLlmChips || []).filter(
            (c) => c.id !== chip.id && c.value.trim().toLowerCase() !== key.toLowerCase(),
          ),
        }));
      },

      rotateQuickAssist: () => {
        set((s) => ({
          quickAssistRotation: (s.quickAssistRotation || 0) + 1,
          // Clear a few oldest dismissals so Suggest can reintroduce useful chips
          quickAssistDismissed: (s.quickAssistDismissed || []).slice(-20),
        }));
        // Also ask Fast mode for a fresh pack when possible
        void get().refreshQuickAssistLlm({ force: true });
      },

      refreshQuickAssistLlm: async (opts) => {
        const force = Boolean(opts?.force);
        const s0 = get();
        if (s0.quickAssistLlmBusy) return;
        const chat = s0.chat;
        // Need some signal (or empty with force for defaults)
        if (!force && chat.length < 1) return;
        const tag = contextFingerprint(chat, s0.activity);
        // Skip if same context & recent (< 25s) unless forced
        if (
          !force &&
          s0.quickAssistLlmTag === tag &&
          s0.quickAssistLlmChips?.length &&
          Date.now() - (s0.quickAssistLlmAt || 0) < 25_000
        ) {
          return;
        }
        const can =
          Boolean(s0.oauth?.accessToken || s0.apiKey || s0.ssoCookie);
        if (!can) return;

        set({ quickAssistLlmBusy: true });
        try {
          const { grokChat } = await import("./grok-client");
          const habits = topHabitLabels(s0.quickAssistMemory, 6);
          const threadTitle =
            s0.threads.find((th) => th.id === s0.activeThreadId)?.title || null;
          const prompt = buildChipSuggestPrompt({
            chat,
            threadTitle,
            habits,
            dismissed: s0.quickAssistDismissed,
          });
          const fastModel = s0.modelCatalog?.slots?.fast;
          const result = await grokChat({
            messages: [{ role: "user", content: prompt }],
            mode: "fast",
            model: fastModel,
            apiKey: s0.apiKey || undefined,
            accessToken: s0.oauth?.accessToken,
            tokens: s0.oauth,
            ssoCookie: s0.ssoCookie || undefined,
            freeTier: false,
          });
          if (!result.ok || !result.content) {
            set({ quickAssistLlmBusy: false });
            return;
          }
          const chips = parseLlmChips(result.content).map((c, i) => ({
            ...c,
            id: `llm-${tag.slice(0, 12)}-${i}-${c.label.slice(0, 8).replace(/\s/g, "")}`,
            score: 99 - i,
          }));
          if (!chips.length) {
            set({ quickAssistLlmBusy: false });
            return;
          }
          set({
            quickAssistLlmChips: chips,
            quickAssistLlmAt: Date.now(),
            quickAssistLlmTag: tag,
            quickAssistLlmBusy: false,
          });
        } catch {
          set({ quickAssistLlmBusy: false });
        }
      },

      
      refreshWelcomeMessage: async (opts) => {
        const force = Boolean(opts?.force);
        const s0 = get();
        if (s0.welcomeBusy) return;
        // Cache ~10 min unless forced
        if (
          !force &&
          s0.welcomeMessage &&
          Date.now() - (s0.welcomeMessage.generatedAt || 0) < 10 * 60_000
        ) {
          return;
        }

        // Pull USER.md best-effort
        let userMd = "";
        try {
          const { memoryRead } = await import("./file-memory");
          const r = await memoryRead("USER.md");
          if (r?.ok && r.content) userMd = String(r.content);
        } catch {
          try {
            const desktop = typeof window !== "undefined" ? window.grokhubDesktop : null;
            const r = await desktop?.memory?.read?.("USER.md");
            if (r?.ok && r.content) userMd = String(r.content);
          } catch {
            /* ignore */
          }
        }

        const ctx = collectWelcomeContext({
          learning: s0.learning,
          quickAssistMemory: s0.quickAssistMemory,
          memoryNotes: s0.agentPrefs?.memoryNotes,
          userMd,
          threads: s0.threads,
          displayName: s0.profile?.displayName || null,
          planLabel: null,
        });
        const fallback = emptyWelcomeFallback({
          displayName: ctx.displayName,
          habits: ctx.habits,
          interests: [...ctx.interests, ...ctx.recentTopics].slice(0, 4),
        });

        const can =
          Boolean(s0.oauth?.accessToken || s0.apiKey || s0.ssoCookie);
        if (!can) {
          set({ welcomeMessage: fallback, welcomeBusy: false });
          return;
        }

        set({ welcomeBusy: true });
        try {
          const { grokChat } = await import("./grok-client");
          const prompt = buildWelcomePrompt(ctx);
          const result = await grokChat({
            messages: [{ role: "user", content: prompt }],
            mode: "fast",
            model: s0.modelCatalog?.slots?.fast,
            apiKey: s0.apiKey || undefined,
            accessToken: s0.oauth?.accessToken,
            tokens: s0.oauth,
            ssoCookie: s0.ssoCookie || undefined,
            freeTier: false,
          });
          if (!result.ok || !result.content) {
            set({ welcomeMessage: fallback, welcomeBusy: false });
            return;
          }
          const welcome = parseWelcomePayload(result.content, fallback);
          set({ welcomeMessage: welcome, welcomeBusy: false });
        } catch {
          set({ welcomeMessage: fallback, welcomeBusy: false });
        }
      },

syncWebsiteConnectors: async () => {
        // Website multi-connector catalog removed — keep core three only
        set((s) => ({ connectors: pruneToCoreConnectors(s.connectors) }));
        scheduleSettingsPersist();
        return { ok: true, detail: "Core tools only (Grok, Desktop, GitHub)", count: 3 };
      },

      setApiKey: (key) => {
        set({ apiKey: key, grokConnected: null });
        void import("./secrets-client").then((m) => m.secretsSet("apiKey", key));
        scheduleSettingsPersist();
      },
      setGithubToken: (token) => {
        set({ githubToken: token });
        void import("./secrets-client").then((m) => m.secretsSet("githubToken", token));
        scheduleSettingsPersist();
      },
      setSsoCookie: (cookie) => {
        const raw = cookie.trim();
        // Normalize bare tokens
        const normalized =
          raw && !raw.includes("=") ? `sso=${raw}` : raw;
        set((s) => ({
          ssoCookie: normalized,
          // Website session alone enables free Grok path
          grokConnected: normalized
            ? s.grokConnected === true
              ? true
              : s.oauth || s.apiKey
                ? s.grokConnected
                : true
            : s.grokConnected,
          grokStatusDetail: normalized
            ? s.oauth || s.apiKey
              ? s.grokStatusDetail
              : "Website session linked"
            : s.grokStatusDetail,
          usage:
            normalized && s.usage.plan !== "free" && !s.oauth && !s.apiKey
              ? { ...s.usage, plan: "free" as const }
              : s.usage,
        }));
        void import("./secrets-client").then((m) =>
          m.secretsSet("ssoCookie", normalized),
        );
        if (typeof window !== "undefined" && window.grokhubDesktop?.grok?.injectWebsiteCookie) {
          void window.grokhubDesktop.grok.injectWebsiteCookie(normalized);
        }
        void get().refreshUsage();
        void get().syncWebsiteConnectors();
        scheduleSettingsPersist();
      },

      startGrokOAuth: async () => {
        const { oauthStart } = await import("./grok-client");
        const start = await oauthStart();
        set({
          oauthPending: {
            deviceCode: start.deviceCode,
            userCode: start.userCode,
            verificationUri: start.verificationUri,
            verificationUriComplete: start.verificationUriComplete,
            expiresAt: Date.now() + (start.expiresIn || 1800) * 1000,
          },
          grokStatusDetail: `Approve code ${start.userCode} at accounts.x.ai`,
        });
        get().pushActivity({
          kind: "auth",
          title: "Grok OAuth started",
          detail: `Enter code ${start.userCode}`,
          status: "running",
        });
      },

      pollGrokOAuth: async () => {
        const pending = get().oauthPending;
        if (!pending) return "failed";
        if (Date.now() > pending.expiresAt) {
          set({
            oauthPending: null,
            grokStatusDetail: "OAuth code expired — start again",
          });
          return "failed";
        }
        const { oauthPoll } = await import("./grok-client");
        const r = await oauthPoll(pending.deviceCode);
        if (r.status === "ready") {
          void import("./secrets-client").then((m) =>
            m.secretsSet("oauth", JSON.stringify(r.tokens)),
          );
          set({
            oauth: r.tokens,
            oauthPending: null,
            grokConnected: true,
            grokStatusDetail: `Grok OAuth · ${r.tokens.email || r.tokens.name || "connected"}`,
          });
          // Mark a logical Grok connector if present
          set((s) => ({
            connectors: s.connectors.map((c) =>
              c.id === "custom-mcp" || c.name.toLowerCase().includes("grok")
                ? c
                : c,
            ),
          }));
          await get().syncFromGrok({
            displayName: r.tokens.name ?? null,
            email: r.tokens.email ?? null,
            imageUrl: r.tokens.picture ?? null,
          });
          // Ensure a connected "Grok" connector row exists
          set((s) => {
            const hasGrok = s.connectors.some((c) => c.id === "grok-xai");
            const grokConn = {
              id: "grok-xai",
              name: "Grok (xAI)",
              category: "Grok",
              description: "Live Grok via SuperGrok / X Premium OAuth or API key.",
              status: "connected" as const,
              tools: ["chat", "models", "imagine"],
              lastUsed: Date.now(),
            };
            return {
              connectors: hasGrok
                ? s.connectors.map((c) =>
                    c.id === "grok-xai"
                      ? { ...c, status: "connected" as const, lastUsed: Date.now() }
                      : c,
                  )
                : [grokConn, ...s.connectors],
            };
          });
          get().pushActivity({
            kind: "auth",
            title: "Grok OAuth connected",
            detail: r.tokens.email || r.tokens.name || "Session active",
            status: "success",
          });
          void get().syncSetupWithGrokAccount();
          return "ready";
        }
        if (r.status === "expired" || r.status === "denied") {
          set({
            oauthPending: null,
            grokConnected: false,
            grokStatusDetail: r.error || "OAuth failed",
          });
          get().pushActivity({
            kind: "auth",
            title: "Grok OAuth failed",
            detail: r.error,
            status: "failed",
          });
          return "failed";
        }
        return "pending";
      },


      setSetupSyncMeta: (patch) => {
        set((st) => ({
          setupSyncMeta: { ...st.setupSyncMeta, ...patch },
        }));
      },

      scheduleSetupAutoPush: () => {
        const meta = get().setupSyncMeta;
        if (!meta?.autoPushOnChange || !get().oauth?.accessToken) return;
        const w = globalThis as unknown as { __grokhubSetupPushTimer?: number };
        if (typeof window !== "undefined" && w.__grokhubSetupPushTimer) {
          window.clearTimeout(w.__grokhubSetupPushTimer);
        }
        if (typeof window === "undefined") return;
        w.__grokhubSetupPushTimer = window.setTimeout(() => {
          void get().pushSetupSync();
        }, 12_000);
      },

      exportSetupPackJson: async (opts) => {
        const s = get();
        const pack = buildSetupPack({
          oauth: s.oauth,
          mode: s.mode,
          desktop: s.desktop as unknown as Record<string, unknown>,
          agents: s.agents,
          skills: s.skills,
          automations: s.automations,
          connectors: s.connectors,
          openClawWorkspace: s.openClawWorkspace,
        });
        const plain = JSON.stringify(pack, null, 2);
        if (opts?.passphrase?.trim()) {
          const { encryptSetupJson } = await import("./setup-crypto");
          return JSON.stringify(await encryptSetupJson(plain, opts.passphrase), null, 2);
        }
        return plain;
      },

      importSetupPackJson: async (json, opts) => {
        try {
          const { unwrapSetupPayload } = await import("./setup-crypto");
          const plain = await unwrapSetupPayload(json, opts?.passphrase);
          const pack = parseSetupPack(JSON.parse(plain));
          if (!pack) return { ok: false, detail: "Not a GrokHub setup pack" };
          const s = get();
          const merged = mergeSetupPack(pack, {
            agents: s.agents,
            skills: s.skills,
            automations: s.automations,
            connectors: s.connectors,
            mode: s.mode,
            desktop: s.desktop as unknown as Record<string, unknown>,
          });
          set((st) => ({
            mode: (merged.mode as typeof st.mode) || st.mode,
            desktop: merged.desktop
              ? { ...st.desktop, ...(merged.desktop as object) }
              : st.desktop,
            agents: merged.agents || st.agents,
            skills: merged.skills || st.skills,
            automations: merged.automations || st.automations,
            connectors: merged.connectors || st.connectors,
            setupSyncMeta: {
              ...st.setupSyncMeta,
              lastPullAt: Date.now(),
              lastDetail: `Imported pack (${merged.applied.join(", ")})`,
              lastAccount: pack.account.email || pack.account.sub,
            },
          }));
          get().pushActivity({
            kind: "system",
            title: "Setup imported",
            detail: merged.applied.join(", ") || "empty",
            status: "success",
          });
          return { ok: true, detail: `Applied: ${merged.applied.join(", ") || "nothing"}` };
        } catch (e) {
          return { ok: false, detail: e instanceof Error ? e.message : "import failed" };
        }
      },

      pushSetupSync: async (opts) => {
        const s = get();
        if (!s.oauth?.accessToken) {
          return { ok: false, detail: "Sign in with Grok OAuth first" };
        }
        const pack = buildSetupPack({
          oauth: s.oauth,
          mode: s.mode,
          desktop: s.desktop as unknown as Record<string, unknown>,
          agents: s.agents,
          skills: s.skills,
          automations: s.automations,
          connectors: s.connectors,
          openClawWorkspace: s.openClawWorkspace,
        });
        const plain = JSON.stringify(pack);
        let storeBody = plain;
        const effectivePass = opts?.passphrase?.trim() || "";
        if (effectivePass) {
          const { encryptSetupJson } = await import("./setup-crypto");
          storeBody = JSON.stringify(await encryptSetupJson(plain, effectivePass));
        }
        try {
          const key = `setup-pack:${setupAccountKey(s.oauth)}`;
          if (typeof window !== "undefined" && window.grokhubDesktop?.state?.set) {
            await window.grokhubDesktop.state.set(key, storeBody);
          }
        } catch {
          /* ignore */
        }
        const gh = s.githubToken?.trim();
        if (!gh) {
          set((st) => ({
            setupSyncMeta: {
              ...st.setupSyncMeta,
              lastPushAt: Date.now(),
              lastAccount: setupAccountKey(s.oauth!),
              lastDetail: effectivePass
                ? "Saved encrypted local vault (add GitHub token for cloud)"
                : "Saved local account vault (add GitHub token for cloud sync)",
            },
          }));
          return {
            ok: true,
            detail: effectivePass
              ? "Encrypted setup saved locally. Link a GitHub token for cross-device Gist sync."
              : "Setup saved for this Grok account locally. Link a GitHub token in Settings to sync across machines via private Gist.",
          };
        }
        const r = await pushSetupToGist(
          gh,
          pack,
          s.setupSyncMeta?.lastGistId,
          effectivePass ? storeBody : undefined,
        );
        set((st) => ({
          setupSyncMeta: {
            ...st.setupSyncMeta,
            lastPushAt: Date.now(),
            lastGistId: r.gistId || st.setupSyncMeta?.lastGistId,
            lastAccount: setupAccountKey(s.oauth!),
            lastDetail: r.ok
              ? effectivePass
                ? "Pushed encrypted pack to GitHub Gist"
                : "Pushed to GitHub Gist"
              : r.error,
          },
        }));
        get().pushActivity({
          kind: "auth",
          title: r.ok ? "Setup synced (push)" : "Setup push failed",
          detail: r.ok ? r.htmlUrl || r.gistId || "gist" : r.error || "failed",
          status: r.ok ? "success" : "failed",
        });
        return {
          ok: Boolean(r.ok),
          detail: r.ok
            ? `Pushed setup for ${pack.account.email || pack.account.sub}`
            : r.error || "push failed",
        };
      },

      pullSetupSync: async (opts) => {
        const s = get();
        if (!s.oauth?.accessToken) {
          return { ok: false, detail: "Sign in with Grok OAuth first" };
        }
        const acct = setupAccountKey(s.oauth);
        let pack: SetupPack | null = null;
        let detail = "";

        const gh = s.githubToken?.trim();
        if (gh) {
          const r = await pullSetupFromGist(gh, acct, s.setupSyncMeta?.lastGistId);
          if (r.ok && (r.pack || r.raw)) {
            if (r.pack) {
              pack = r.pack;
            } else if (r.raw) {
              try {
                const { unwrapSetupPayload } = await import("./setup-crypto");
                const plain = await unwrapSetupPayload(r.raw, opts?.passphrase);
                pack = parseSetupPack(JSON.parse(plain));
              } catch (e) {
                detail =
                  e instanceof Error
                    ? e.message
                    : "Could not decrypt Gist pack";
              }
            }
            if (pack) detail = "Pulled from GitHub Gist";
            if (r.gistId) {
              set((st) => ({
                setupSyncMeta: { ...st.setupSyncMeta, lastGistId: r.gistId },
              }));
            }
          } else {
            detail = r.error || "No gist";
          }
        }

        if (!pack && typeof window !== "undefined" && window.grokhubDesktop?.state?.get) {
          try {
            const got = await window.grokhubDesktop.state.get(`setup-pack:${acct}`);
            if (got?.value) {
              try {
                const { unwrapSetupPayload } = await import("./setup-crypto");
                const plain = await unwrapSetupPayload(got.value, opts?.passphrase);
                pack = parseSetupPack(JSON.parse(plain));
              } catch (e) {
                detail =
                  e instanceof Error
                    ? e.message
                    : "Could not open local vault (wrong passphrase?)";
              }
              if (pack) {
                detail = detail ? `${detail}; local vault` : "Loaded local account vault";
              }
            }
          } catch {
            /* ignore */
          }
        }

        if (!pack) {
          return {
            ok: false,
            detail:
              detail ||
              "No setup found for this Grok account yet. Push from a configured machine, or import a pack file.",
          };
        }

        const merged = mergeSetupPack(pack, {
          agents: s.agents,
          skills: s.skills,
          automations: s.automations,
          connectors: s.connectors,
          mode: s.mode,
          desktop: s.desktop as unknown as Record<string, unknown>,
        });
        set((st) => ({
          mode: (merged.mode as typeof st.mode) || st.mode,
          desktop: merged.desktop
            ? { ...st.desktop, ...(merged.desktop as object) }
            : st.desktop,
          agents: merged.agents || st.agents,
          skills: merged.skills || st.skills,
          automations: merged.automations || st.automations,
          connectors: merged.connectors || st.connectors,
          setupSyncMeta: {
            ...st.setupSyncMeta,
            lastPullAt: Date.now(),
            lastAccount: acct,
            lastDetail: `${detail} · ${merged.applied.join(", ")}`,
          },
        }));
        get().pushActivity({
          kind: "auth",
          title: "Setup synced (pull)",
          detail: merged.applied.join(", ") || detail,
          status: "success",
        });
        return {
          ok: true,
          detail: `${detail}: ${merged.applied.join(", ") || "ok"}`,
        };
      },

      syncSetupWithGrokAccount: async (opts) => {
        const s = get();
        if (!s.oauth?.accessToken && !s.apiKey) {
          return { ok: false, detail: "Connect Grok OAuth first" };
        }
        const parts: string[] = [];
        try {
          await get().syncFromGrok({
            displayName: s.oauth?.name ?? null,
            email: s.oauth?.email ?? null,
            imageUrl: s.oauth?.picture ?? null,
          });
          parts.push("profile/models");
        } catch {
          /* ignore */
        }
        try {
          await get().refreshModels();
          parts.push("models");
        } catch {
          /* ignore */
        }
        try {
          const conn = await get().syncWebsiteConnectors();
          if (conn.ok) parts.push(`${conn.count} connectors`);
          else parts.push("connectors skipped");
        } catch {
          parts.push("connectors failed");
        }
        try {
          await get().refreshUsage();
          parts.push("usage");
        } catch {
          /* ignore */
        }
        if (s.setupSyncMeta?.autoPullOnLogin !== false && s.oauth?.accessToken) {
          const pull = await get().pullSetupSync(opts);
          parts.push(pull.ok ? `pack: ${pull.detail}` : `pack: ${pull.detail}`);
        }
        const detail = parts.join(" · ") || "done";
        set((st) => ({
          setupSyncMeta: {
            ...st.setupSyncMeta,
            lastPullAt: Date.now(),
            lastAccount: s.oauth?.email || st.setupSyncMeta?.lastAccount,
            lastDetail: detail,
          },
        }));
        get().pushActivity({
          kind: "auth",
          title: "Grok account setup sync",
          detail,
          status: "success",
        });
        return { ok: true, detail };
      },

      clearGrokOAuth: () => {
        set({
          oauth: null,
          oauthPending: null,
          ssoCookie: "",
          grokConnected: get().apiKey ? null : false,
          grokStatusDetail: "Grok OAuth cleared",
        });
        set((s) => ({
          connectors: s.connectors.map((c) =>
            c.id === "grok-xai" ? { ...c, status: "disconnected" as const } : c,
          ),
        }));
        get().pushActivity({
          kind: "auth",
          title: "Grok OAuth signed out",
          detail: "Session removed from this device",
          status: "success",
        });
      },

            linkGrokWebsiteSession: async () => {
        try {
          if (typeof window !== "undefined" && window.grokhubDesktop?.grok?.linkWebsiteSession) {
            const r = await window.grokhubDesktop.grok.linkWebsiteSession();
            if (r?.cookie) {
              // Persist via setSsoCookie (secrets + inject + usage)
              get().setSsoCookie(r.cookie);
              void get().syncWebsiteConnectors();
              get().pushActivity({
                kind: "auth",
                title: "Grok website linked",
                detail: "Session saved — usage & connectors will sync from grok.com",
                status: "success",
              });
              return { ok: true, detail: "Grok website session linked" };
            }
            return {
              ok: false,
              detail:
                r?.error ||
                "No session captured. Sign in until Grok chat loads, click “Use this session” in the bar, or paste sso= from browser cookies.",
            };
          }
          // Browser preview: open grok.com for manual cookie copy (desktop uses Electron window)
          if (typeof window !== "undefined") {
            window.open("https://grok.com/", "_blank", "noopener,noreferrer");
          }
          return {
            ok: false,
            detail:
              "Opened grok.com — copy the sso cookie (DevTools → Application → Cookies) and paste it below. Full auto-link works in the Arch desktop app.",
          };
        } catch (e) {
          return {
            ok: false,
            detail: e instanceof Error ? e.message : "link failed",
          };
        }
      },

      importOpenClawWorkspace: async (path) => {
        try {
          const { hostReadOpenClawWorkspace } = await import("./host-client");
          const { mapOpenClawWorkspace } = await import("./openclaw-import");
          const raw = await hostReadOpenClawWorkspace(path);
          if (!raw?.ok) {
            return {
              ok: false,
              detail: raw?.error || "Could not read OpenClaw workspace",
            };
          }
          const mapped = mapOpenClawWorkspace(raw);
          set((s) => {
            const bySlash = new Map(s.skills.map((sk) => [sk.slash, sk]));
            for (const sk of mapped.skills) {
              bySlash.set(sk.slash, sk);
            }
            const mergedSkills = Array.from(bySlash.values());
            const others = s.agents.filter((a) => !a.id.startsWith("openclaw-"));
            const mergedAgents = [...mapped.agents, ...others];
            const autoNames = new Set(s.automations.map((a) => a.name));
            const newAutos = mapped.automations.filter((a) => !autoNames.has(a.name));
            return {
              skills: mergedSkills,
              agents: mergedAgents,
              automations: [...newAutos, ...s.automations],
              openClawWorkspace: {
                root: mapped.root,
                importedAt: Date.now(),
                filesImported: mapped.filesImported,
                contextBundle: mapped.contextBundle,
                identityName: mapped.identityName,
              },
            };
          });
          get().pushActivity({
            kind: "system",
            title: "OpenClaw workspace imported",
            detail: `${mapped.root} · ${mapped.skills.length} skills · ${mapped.filesImported.length} files`,
            status: "success",
          });
          const warn = mapped.warnings.length ? ` · ${mapped.warnings[0]}` : "";
          return {
            ok: true,
            detail: `Imported ${mapped.skills.length} skills, ${mapped.automations.length} automations from ${mapped.root}${warn}`,
            skills: mapped.skills.length,
            automations: mapped.automations.length,
          };
        } catch (e) {
          return {
            ok: false,
            detail: e instanceof Error ? e.message : "import failed",
          };
        }
      },

      clearOpenClawWorkspace: () => {
        set((s) => ({
          openClawWorkspace: null,
          agents: s.agents.filter((a) => !a.id.startsWith("openclaw-")),
          skills: s.skills.filter((sk) => !sk.id.startsWith("ocskill")),
          automations: s.automations.filter((a) => !a.name.startsWith("OpenClaw ")),
        }));
        get().pushActivity({
          kind: "system",
          title: "OpenClaw workspace cleared",
          detail: "Imported skills/agents/context removed",
          status: "success",
        });
      },

      probeGrok: async () => {
        try {
          const { grokProbe, oauthEnsure } = await import("./grok-client");
          let accessToken = get().oauth?.accessToken;
          if (get().oauth) {
            try {
              const ensured = await oauthEnsure(get().oauth!);
              if (ensured.tokens) {
                set({ oauth: ensured.tokens });
                if (ensured.refreshed) {
                  void import("./secrets-client").then((m) =>
                    m.secretsSet("oauth", JSON.stringify(ensured.tokens)),
                  );
                }
              }
              accessToken = ensured.tokens?.accessToken || accessToken;
              if (ensured.ok) {
                set({
                  grokConnected: true,
                  grokStatusDetail: ensured.detail || "Grok OAuth live",
                });
                return true;
              }
            } catch (e) {
              // fall through to api key
              const msg = e instanceof Error ? e.message : "oauth ensure failed";
              if (!get().apiKey) {
                set({ grokConnected: false, grokStatusDetail: msg });
                return false;
              }
            }
          }
          const r = await grokProbe({
            apiKey: get().apiKey || undefined,
            accessToken,
          });
          set({
            grokConnected: r.ok,
            grokStatusDetail:
              r.detail +
              (r.authMode === "oauth"
                ? " · OAuth"
                : r.envConfigured && !get().apiKey && !accessToken
                  ? " (env key)"
                  : r.authMode === "apiKey"
                    ? " · API key"
                    : ""),
          });
          return r.ok;
        } catch (e) {
          const msg = e instanceof Error ? e.message : "probe failed";
          set({ grokConnected: false, grokStatusDetail: msg });
          return false;
        }
      },

      refreshOAuthSession: async (opts) => {
        const cur = get().oauth;
        if (!cur?.accessToken) {
          return { ok: false, refreshed: false, detail: "No OAuth session" };
        }
        try {
          const { tokenNeedsRefresh } = await import("./xai-oauth");
          if (!opts?.force && !tokenNeedsRefresh(cur)) {
            return {
              ok: true,
              refreshed: false,
              detail: "OAuth token still valid",
            };
          }
          if (!cur.refreshToken) {
            set({
              grokConnected: false,
              grokStatusDetail: "Grok OAuth expired — sign in again",
            });
            return {
              ok: false,
              refreshed: false,
              detail: "No refresh token — reconnect OAuth",
            };
          }
          const { oauthEnsure } = await import("./grok-client");
          const ensured = await oauthEnsure(cur);
          if (ensured.tokens) {
            set({
              oauth: ensured.tokens,
              grokConnected: ensured.ok !== false,
              grokStatusDetail: ensured.refreshed
                ? "Grok OAuth refreshed"
                : ensured.detail || "Grok OAuth live",
            });
            if (ensured.refreshed || opts?.force) {
              void import("./secrets-client").then((m) =>
                m.secretsSet("oauth", JSON.stringify(ensured.tokens)),
              );
            }
          }
          if (ensured.ok === false && ensured.refreshed === false) {
            return {
              ok: false,
              refreshed: false,
              detail: ensured.detail || "OAuth probe failed",
            };
          }
          return {
            ok: true,
            refreshed: Boolean(ensured.refreshed),
            detail: ensured.refreshed
              ? "Access token refreshed"
              : ensured.detail || "ok",
          };
        } catch (e) {
          const msg = e instanceof Error ? e.message : "OAuth refresh failed";
          // Invalid grant / hard fail — clear so UI prompts reconnect
          if (/invalid_grant|expired|revoked|sign in again/i.test(msg)) {
            set({
              grokConnected: false,
              grokStatusDetail: msg,
            });
          } else {
            set({ grokStatusDetail: msg });
          }
          return { ok: false, refreshed: false, detail: msg };
        }
      },

      syncFromGrok: async (opts) => {
        const models: string[] = [];
        try {
          const key = get().apiKey || "";
          const accessToken = get().oauth?.accessToken || "";
          const res = await fetch("/api/grok", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              action: "models",
              apiKey: key,
              accessToken,
            }),
          });
          if (res.ok) {
            const data = (await res.json()) as { models?: string[] };
            if (Array.isArray(data.models)) models.push(...data.models.filter(Boolean));
          }
        } catch {
          /* optional when offline */
        }
        const now = Date.now();
        const catalog = models.length ? buildCatalog(models, get().modelCatalog) : get().modelCatalog || emptyCatalog();
        set((st) => ({
          profile: {
            displayName: opts?.displayName ?? st.profile.displayName,
            email: opts?.email ?? st.profile.email,
            imageUrl: opts?.imageUrl ?? st.profile.imageUrl,
            models: catalog.essential.length ? catalog.essential : st.profile.models,
            connectedAt: st.profile.connectedAt ?? (st.grokConnected ? now : null),
          },
          modelCatalog: catalog,
          lastModelsFetchAt: models.length ? now : st.lastModelsFetchAt,
          agents:
            st.agents.length > 0
              ? st.agents
              : [
                  {
                    id: "primary",
                    name: (opts?.displayName || "Primary").split(/\s+/)[0] || "Primary",
                    role: "Primary co-pilot",
                    model: "Grok · Auto",
                    status: "idle" as const,
                    tasks: 0,
                    color: "#d4d4d8",
                  },
                  {
                    id: "builder",
                    name: "Build",
                    role: "Build mode",
                    model: "Grok · Build",
                    status: "idle" as const,
                    tasks: 0,
                    color: "#7dd3fc",
                  },
                ],
        }));
        if (opts?.displayName || opts?.email || models.length) {
          get().pushActivity({
            kind: "auth",
            title: "Grok profile synced",
            detail:
              opts?.displayName ||
              opts?.email ||
              `${catalog.essential.length} essential models (${catalog.source})`,
            status: "success",
          });
        }
        // Reclassify slots with Grok when the live list is new / stale
        if (models.length) void get().refreshModels();
      },

      refreshModels: async (opts) => {
        try {
          const st = get();
          const res = await fetch("/api/grok", {
            method: "POST",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              action: "models",
              apiKey: st.apiKey || "",
              accessToken: st.oauth?.accessToken || "",
            }),
          });
          let models: string[] = [];
          if (res.ok) {
            const data = (await res.json()) as { models?: string[] };
            if (Array.isArray(data.models)) models = data.models.filter(Boolean);
          }
          if (!models.length) {
            set({ lastModelsFetchAt: Date.now() });
            return;
          }

          let catalog = buildCatalog(models, st.modelCatalog);

          const shouldClassify =
            Boolean(st.oauth?.accessToken || st.apiKey || st.grokConnected) &&
            (Boolean(opts?.force) || needsGrokClassification(catalog));

          if (shouldClassify) {
            try {
              const cRes = await fetch("/api/grok", {
                method: "POST",
                headers: { "content-type": "application/json" },
                body: JSON.stringify({
                  action: "classifyModels",
                  models,
                  apiKey: st.apiKey || "",
                  accessToken: st.oauth?.accessToken || "",
                  tokens: st.oauth || undefined,
                }),
              });
              if (cRes.ok) {
                const cData = (await cRes.json()) as {
                  ok?: boolean;
                  plan?: GrokSlotPlan;
                };
                if (cData.ok && cData.plan) {
                  catalog = applyGrokPlan(catalog, cData.plan);
                }
              }
            } catch {
              /* keep heuristic */
            }
          }

          set((s) => ({
            modelCatalog: catalog,
            lastModelsFetchAt: Date.now(),
            profile: {
              ...s.profile,
              models: catalog.essential,
            },
            grokStatusDetail: s.grokConnected
              ? `Live · ${catalog.essential.length} models · slots by ${catalog.classifiedBy}`
              : s.grokStatusDetail,
          }));

          if (catalog.classifiedBy === "grok" && shouldClassify) {
            get().pushActivity({
              kind: "system",
              title: "Model slots updated by Grok",
              detail:
                catalog.classifyNotes ||
                `Fast ${catalog.slots.fast} · Smart ${catalog.slots.smart} · Build ${catalog.slots.build}`,
              status: "success",
            });
          }
        } catch {
          set({ lastModelsFetchAt: Date.now() });
        }
      },

      newThread: () => {
        if (get().running || get().streamingMessageId) {
          try { get().stopChat(); } catch { /* ignore */ }
        }
        const now = Date.now();
        // Empty thread — adaptive welcome is UI-only (not a chat bubble)
        const thread: ChatThread = {
          id: uid("thread"),
          title: "New chat",
          createdAt: now,
          updatedAt: now,
          messages: [],
        };
        set((s) => ({
          threads: [thread, ...s.threads],
          activeThreadId: thread.id,
          chat: [],
          nav: "chat",
          running: false,
          streamStatus: null,
          streamingMessageId: null,
        }));
        void get().refreshWelcomeMessage({ force: true });
        try {
          if (typeof window !== "undefined") {
            window.dispatchEvent(new CustomEvent("grokhub:focus-composer"));
            window.dispatchEvent(new CustomEvent("grokhub:new-chat"));
          }
        } catch {
          /* ignore */
        }
      },

      selectThread: (id) => {
        const t = get().threads.find((x) => x.id === id);
        if (!t) return;
        // Switching threads mid-run leaves a zombie stream — stop first
        if (get().running || get().streamingMessageId) {
          try {
            get().stopChat();
          } catch {
            /* ignore */
          }
        }
        const msgs = (t.messages || []).map((m) =>
          m.streaming ? { ...m, streaming: false } : m,
        );
        set({
          activeThreadId: id,
          chat: msgs,
          nav: "chat",
          mode: t.mode || get().mode,
          running: false,
          streamStatus: null,
          streamingMessageId: null,
        });
      },

      deleteThread: (id) => {
        if (get().activeThreadId === id && (get().running || get().streamingMessageId)) {
          try { get().stopChat(); } catch { /* ignore */ }
        }
        const victim = get().threads.find((t) => t.id === id);
        const remaining = get().threads.filter((t) => t.id !== id);
        if (remaining.length === 0) {
          if (victim) {
            set({
              undoBuffer: {
                kind: "thread",
                label: `Deleted “${victim.title || "chat"}”`,
                expiresAt: Date.now() + 12_000,
                thread: victim,
                wasActive: true,
              },
            });
          }
          get().newThread();
          // newThread replaces threads — re-apply undo without the deleted one already gone
          return;
        }
        const wasActive = get().activeThreadId === id;
        const nextActive =
          wasActive
            ? remaining[0]!
            : remaining.find((t) => t.id === get().activeThreadId) || remaining[0]!;
        set({
          threads: remaining,
          activeThreadId: nextActive.id,
          chat: nextActive.messages,
          undoBuffer: victim
            ? {
                kind: "thread",
                label: `Deleted “${victim.title || "chat"}”`,
                expiresAt: Date.now() + 12_000,
                thread: victim,
                wasActive,
              }
            : get().undoBuffer,
        });
      },

      renameThread: (id, title) => {
        const next = title.trim().slice(0, 80);
        if (!next) return;
        set((s) => ({
          threads: s.threads.map((t) =>
            t.id === id
              ? {
                  ...t,
                  title: next,
                  titleLocked: true, // manual rename freezes auto titles
                  updatedAt: Date.now(),
                }
              : t,
          ),
        }));
      },

      autoRenameThread: async (id: string) => {
        const th = get().threads.find((t) => t.id === id);
        if (!th) return;
        const msgs = th.id === get().activeThreadId ? get().chat : th.messages || [];
        if (!msgs.some((m) => m.role === "user")) {
          get().pushActivity({
            kind: "system",
            title: "Nothing to name yet",
            detail: "Send a message first, then try Auto name",
            status: "queued",
          });
          return;
        }
        get().pushActivity({
          kind: "system",
          title: "Naming chat…",
          detail: "Fast mode summary title",
          status: "running",
        });
        const before = th.title;
        await autoRenameThreadWithFast(get, set, id, { force: true });
        const after = get().threads.find((t) => t.id === id)?.title;
        get().pushActivity({
          kind: "system",
          title: after && after !== before ? `Named “${after}”` : "Auto name finished",
          detail:
            after && after !== before
              ? "Updated from chat summary"
              : "Could not generate a better title — try again when connected",
          status: after && after !== before ? "success" : "queued",
        });
      },

      pinThread: (id, pinned) => {
        set((s) => ({
          threads: s.threads.map((t) =>
            t.id === id
              ? { ...t, pinned: typeof pinned === "boolean" ? pinned : !t.pinned, updatedAt: Date.now() }
              : t,
          ),
        }));
      },

      setThreadFolder: (id, folder) => {
        const f = folder?.trim() ? folder.trim().slice(0, 40) : null;
        set((s) => ({
          threads: s.threads.map((t) =>
            t.id === id ? { ...t, folder: f, updatedAt: Date.now() } : t,
          ),
        }));
      },

      dismissSessionResume: () => set({ sessionResume: null }),
      setUpdateBanner: (v) => set({ updateBanner: v }),
      checkUpdateQuiet: async () => {
        try {
          const { checkUpdate } = await import("./grok-client");
          const st = await checkUpdate(get().githubToken || undefined);
          if (st?.updateAvailable && (st.remoteSha || st.detail)) {
            set({
              updateBanner: {
                available: true,
                version: st.currentVersion,
                detail:
                  st.remoteMessage ||
                  st.remoteSha?.slice(0, 8) ||
                  st.detail ||
                  "Update available",
              },
            });
          } else {
            set({ updateBanner: null });
          }
        } catch {
          /* quiet */
        }
      },

      setAgentPrefs: (patch) => {
        set((s) => ({
          agentPrefs: {
            ...s.agentPrefs,
            ...patch,
            temperature: Math.min(
              1,
              Math.max(0, Number(patch.temperature ?? s.agentPrefs.temperature) || 0.7),
            ),
          },
        }));
      
        scheduleSettingsPersist();
      },
      compactThread: (threadId) => {
        const s = get();
        const tid = threadId || s.activeThreadId;
        if (!tid) return { ok: false, detail: "No active thread" };
        const th = s.threads.find((x) => x.id === tid);
        const messages =
          tid === s.activeThreadId ? s.chat : th?.messages || [];
        const result = compactMessages(messages, {
          keepRecent: 10,
          title: th?.title || "Chat",
        });
        if (!result) {
          return {
            ok: false,
            detail: "Not enough messages to compact (need more than ~12 turns)",
          };
        }
        const nextNotes = mergeFlushIntoMemory(
          s.agentPrefs.memoryNotes || "",
          result.flushFacts,
        );
        // M1: durable file memory (best-effort async)
        if (result.flushFacts.length) {
          void memoryAppendFacts(result.flushFacts, { target: "MEMORY.md" });
          void memoryAppend(
            "today",
            `Compacted ${result.messageCount} msgs · ${result.flushFacts.length} facts`,
          );
        }
        set((state) => ({
          agentPrefs: {
            ...state.agentPrefs,
            memoryNotes: nextNotes,
          },
          threads: state.threads.map((thread) =>
            thread.id === tid
              ? {
                  ...thread,
                  summary: result.summary,
                  summaryUpToId: result.summaryUpToId,
                  compactedAt: result.compactedAt,
                  compactedMessageCount: result.messageCount,
                  updatedAt: Date.now(),
                }
              : thread,
          ),
        }));
        get().pushActivity({
          kind: "chat",
          title: "Context compacted",
          detail: `Folded ${result.messageCount} msgs · ~${result.tokensEst} tok summary`,
          status: "success",
        });
        return {
          ok: true,
          detail: `Compacted ${result.messageCount} older messages into a summary (~${result.tokensEst} tokens). Full chat still visible. ${result.flushFacts.length ? `Flushed ${result.flushFacts.length} facts to memory.` : ""}`,
        };
      },

      getContextStats: () => {
        const s = get();
        const th = s.threads.find((x) => x.id === s.activeThreadId);
        const built = buildContext({
          messages: s.chat,
          thread: th || null,
          memoryNotes: s.agentPrefs.memoryNotes,
          openClawBundle: s.openClawWorkspace?.contextBundle,
          trimTools: true,
        });
        return {
          percent: built.percent,
          tokensEst: built.tokensEst,
          budget: built.budget,
          shouldCompact: built.shouldCompact,
          report: formatContextReport(built),
        };
      },


      clearChat: () => {
        if (get().running || get().streamingMessageId) {
          try { get().stopChat(); } catch { /* ignore */ }
        }
        const tid = get().activeThreadId;
        set((s) => ({
          chat: [],
          threads: s.threads.map((th) =>
            th.id === tid
              ? {
                  ...th,
                  messages: [],
                  // Empty thread: unlock auto title again
                  title: "New chat",
                  titleLocked: false,
                  updatedAt: Date.now(),
                }
              : th,
          ),
          running: false,
          streamStatus: null,
          streamingMessageId: null,
        }));
      },

      exportThreadMarkdown: (id) => {
        const tid = id || get().activeThreadId;
        const th = get().threads.find((x) => x.id === tid);
        const messages = th?.messages || get().chat;
        const title = th?.title || "GrokHub chat";
        const lines = [
          `# ${title}`,
          "",
          `_Exported from GrokHub · ${new Date().toISOString()}_`,
          "",
        ];
        for (const m of messages) {
          const who = m.role === "user" ? "You" : m.role === "assistant" ? "Grok" : "System";
          lines.push(`## ${who}`, "", m.content || "", "");
        }
        return lines.join("\n");
      },

      editChatMessage: async (id, content, resend) => {
        const next = content.trim();
        if (!next) return;
        const idx = get().chat.findIndex((m) => m.id === id);
        if (idx < 0) return;
        const msg = get().chat[idx]!;
        if (msg.role !== "user") return;
        set((s) => {
          const chat = s.chat.slice(0, idx + 1).map((m) =>
            m.id === id ? { ...m, content: next, ts: Date.now(), edited: true } : m,
          );
          const tid = s.activeThreadId;
          return {
            chat,
            threads: s.threads.map((th) =>
              th.id === tid ? threadWithMessages(th, chat) : th,
            ),
          };
        });
        if (resend) {
          if (get().running || get().streamingMessageId) {
            try {
              get().stopChat();
            } catch {
              /* ignore */
            }
          }
          set((s) => {
            const chat = s.chat.slice(0, idx);
            const tid = s.activeThreadId;
            return {
              chat,
              threads: s.threads.map((th) =>
                th.id === tid ? { ...th, messages: chat, updatedAt: Date.now() } : th,
              ),
            };
          });
          await get().sendChat(next);
        }
      },

      setReplyTo: (msg) => {
        if (!msg) {
          set({ replyTo: null });
          return;
        }
        const preview = String(msg.content || "")
          .replace(/\s+/g, " ")
          .trim()
          .slice(0, 160);
        set({
          replyTo: {
            id: msg.id,
            preview: preview || "(empty)",
            role: msg.role,
          },
          nav: "chat",
        });
      },

      deleteChatMessages: (ids) => {
        const idSet = new Set(Array.isArray(ids) ? ids : [ids]);
        if (!idSet.size) return;
        const removed = get().chat.filter((m) => idSet.has(m.id));
        const tid = get().activeThreadId;
        set((s) => {
          const chat = s.chat.filter((m) => !idSet.has(m.id));
          const replyTo =
            s.replyTo && idSet.has(s.replyTo.id) ? null : s.replyTo;
          return {
            chat,
            replyTo,
            threads: s.threads.map((th) =>
              th.id === tid ? threadWithMessages(th, chat) : th,
            ),
            undoBuffer: {
              kind: "messages" as const,
              label: `Deleted ${idSet.size} message${idSet.size === 1 ? "" : "s"}`,
              expiresAt: Date.now() + 12_000,
              messages: removed,
              threadId: tid || undefined,
            },
          };
        });
        get().pushActivity({
          kind: "chat",
          title: "Message deleted",
          detail: `${idSet.size} message${idSet.size === 1 ? "" : "s"} removed — Undo available`,
          status: "success",
        });
      },

      resumeLastSession: () => {
        const r = get().sessionResume;
        if (!r || r.kind !== "interrupted") {
          // Drop legacy non-interrupt cards
          set({ sessionResume: null, nav: "chat" });
          return;
        }

        const threads = get().threads;
        let t =
          (r.threadId && threads.find((x) => x.id === r.threadId)) || null;

        if (!t && r.title) {
          const title = r.title.trim().toLowerCase();
          t =
            threads.find(
              (x) =>
                x.title.trim().toLowerCase() === title &&
                (x.messages?.length || 0) > 0,
            ) ||
            threads.find((x) => x.title.trim().toLowerCase() === title) ||
            null;
        }
        if (!t) {
          t =
            [...threads]
              .filter((x) => (x.messages?.length || 0) > 0)
              .sort((a, b) => b.updatedAt - a.updatedAt)[0] || null;
        }

        if (!t) {
          set({ nav: "chat" });
          get().pushActivity({
            kind: "system",
            title: "Resume unavailable",
            detail: "Could not find the interrupted chat.",
            status: "failed",
          });
          return;
        }

        // Prefer longer of persisted thread vs live chat
        let messages = Array.isArray(t.messages) ? [...t.messages] : [];
        if (get().activeThreadId === t.id && get().chat.length > messages.length) {
          messages = [...get().chat];
        }

        set((s) => ({
          activeThreadId: t!.id,
          chat: messages,
          nav: "chat" as const,
          // Prefer thread's saved user mode; never force a one-shot routed mode
          mode: t!.mode || s.mode,
          threads: s.threads.map((th) =>
            th.id === t!.id
              ? {
                  ...th,
                  messages,
                  updatedAt: Date.now(),
                }
              : th,
          ),
          running: false,
          streamStatus: null,
          streamingMessageId: null,
          // Keep banner until Continue succeeds or user Dismisses
        }));

        get().pushActivity({
          kind: "system",
          title: "Opened interrupted chat",
          detail: r.title || "Previous session",
          status: "success",
        });

        if (typeof window !== "undefined") {
          try {
            window.dispatchEvent(
              new CustomEvent("grokhub:resume-session", {
                detail: {
                  threadId: t.id,
                  title: r.title,
                  preview: r.preview,
                  pendingPrompt: r.pendingPrompt || "",
                  focusOnly: true,
                },
              }),
            );
          } catch {
            /* ignore */
          }
        }
      },

      continueInterruptedSession: async () => {
        const r = get().sessionResume;
        if (!r || r.kind !== "interrupted") {
          set({ sessionResume: null });
          return;
        }
        if (get().running || get().streamingMessageId) {
          try {
            get().stopChat();
          } catch {
            /* ignore */
          }
        }

        // Ensure we're on the right thread first
        get().resumeLastSession();
        const prompt =
          r.pendingPrompt ||
          [...get().chat].reverse().find((m) => m.role === "user")?.content ||
          "";
        if (!prompt.trim()) {
          get().pushActivity({
            kind: "system",
            title: "Nothing to continue",
            detail: "No user prompt found for this interrupt.",
            status: "failed",
          });
          set({ sessionResume: null });
          return;
        }

        // Drop trailing stopped/incomplete assistant turn so resend is clean
        set((s) => {
          let chat = [...s.chat];
          // Remove stopped assistant at end
          while (chat.length) {
            const last = chat[chat.length - 1]!;
            if (
              last.role === "assistant" &&
              (last.stopped ||
                last.streaming ||
                /_Stopped\._\s*$/m.test(last.content || "") ||
                (r.stoppedMessageId && last.id === r.stoppedMessageId))
            ) {
              chat.pop();
              continue;
            }
            break;
          }
          // Also drop last user — sendChat will re-add it
          if (chat.length && chat[chat.length - 1]!.role === "user") {
            const lastU = chat[chat.length - 1]!;
            if (
              lastU.content.trim() === prompt.trim() ||
              (r.pendingPrompt && lastU.content.trim() === r.pendingPrompt.trim())
            ) {
              chat.pop();
            }
          }
          const tid = s.activeThreadId;
          return {
            chat,
            threads: s.threads.map((th) =>
              th.id === tid ? threadWithMessages(th, chat) : th,
            ),
            sessionResume: null,
            running: false,
            streamStatus: null,
            streamingMessageId: null,
          };
        });

        await get().sendChat(prompt);
      },
      keepGoingChat: async () => {
        if (get().running) return;
        const chat = get().chat;
        const lastAsst = [...chat].reverse().find((m) => m.role === "assistant");
        const lastUser = [...chat].reverse().find((m) => m.role === "user");
        const { looksLikeIncompleteAgentTurn, buildKeepGoingUserPrompt } = await import(
          "./agent-finish"
        );
        const incomplete = looksLikeIncompleteAgentTurn(lastAsst?.content || "", {
          userPrompt: lastUser?.content,
        });
        // Always allow keep-going after any assistant reply; soft nudge if looks complete
        const prompt = buildKeepGoingUserPrompt(lastUser?.content);
        get().pushActivity({
          kind: "chat",
          title: incomplete ? "Keep going" : "Continue anyway",
          detail: incomplete
            ? "Finishing stalled agent turn"
            : "User asked to continue",
          status: "running",
        });
        await get().sendChat(prompt);
      },

      setPreferFreeGrok: (_v) => {
        set({ preferFreeGrok: false });
        scheduleSettingsPersist();
      },
      setUiTheme: (t) => {
        set({ uiTheme: t });
        scheduleSettingsPersist();
      },
      setToolsNavCollapsed: (v) => {
        set({ toolsNavCollapsed: Boolean(v) });
        scheduleSettingsPersist();
      },

      setPlan: (plan) => {
        const prev = get().usage;
        const next = createUsage(plan);
        set({ usage: next });
        scheduleSettingsPersist();
        get().pushActivity({
          kind: "usage",
          title: `Plan → ${PLAN_LIMITS[plan].label}`,
          detail: `Limit ${PLAN_LIMITS[plan].units} units / month (was ${PLAN_LIMITS[prev.plan].label}) · meter reset for plan change`,
          status: "success",
        });
      },

      recordUsage: (bucket, mode) => {
        const cost = costFor(bucket, mode);
        let ok = true;
        set((s) => {
          const base = ensurePeriod(s.usage);
          const lim = PLAN_LIMITS[base.plan];
          // Local quota UI removed — never block the agent on synthetic limits
          if (false && base.usedUnits + cost > lim.units * 1.02) {
            ok = false;
            return { usage: base };
          }
          const byMode = { ...base.byMode };
          if ((bucket === "message" || bucket === "skill") && mode) {
            byMode[mode] = (byMode[mode] ?? 0) + 1;
          }
          return {
            usage: {
              ...base,
              usedUnits: Math.round((base.usedUnits + cost) * 100) / 100,
              messages: base.messages + (bucket === "message" ? 1 : 0),
              imagine: base.imagine + (bucket === "imagine" ? 1 : 0),
              automations: base.automations + (bucket === "automation" ? 1 : 0),
              host: base.host + (bucket === "host" ? 1 : 0),
              byMode,
              lastPolledAt: Date.now(),
              source: base.source === "website" ? "website" : "local",
            },
          };
        });
        if (!ok) {
          get().pushActivity({
            kind: "usage",
            title: "Quota exceeded",
            detail: `${PLAN_LIMITS[get().usage.plan].label} period limit reached`,
            status: "failed",
          });
        }
        return { ok, cost };
      },

      recordTokenUsage: (tokens, mode, rateLimit) => {
        const cost = unitsFromTokens(tokens, mode);
        let ok = true;
        set((s) => {
          const base = ensurePeriod(s.usage);
          const lim = PLAN_LIMITS[base.plan];
          if (false && base.usedUnits + cost > lim.units * 1.05) {
            ok = false;
            return { usage: base };
          }
          const prompt = tokens.prompt_tokens ?? 0;
          const completion = tokens.completion_tokens ?? Math.max(0, (tokens.total_tokens ?? 0) - prompt);
          const total = tokens.total_tokens ?? prompt + completion;
          const byMode = { ...base.byMode };
          if (mode) byMode[mode] = (byMode[mode] ?? 0) + 1;
          return {
            usage: {
              ...base,
              usedUnits: Math.round((base.usedUnits + cost) * 100) / 100,
              messages: base.messages + 1,
              byMode,
              promptTokens: base.promptTokens + prompt,
              completionTokens: base.completionTokens + completion,
              totalTokens: base.totalTokens + total,
              lastPolledAt: Date.now(),
              source: base.source === "website" ? "website" : "live",
              rateLimitRemaining: rateLimit?.remaining ?? base.rateLimitRemaining ?? null,
              rateLimitLimit: rateLimit?.limit ?? base.rateLimitLimit ?? null,
              rateLimitResetAt: rateLimit?.resetAt ?? base.rateLimitResetAt ?? null,
            },
          };
        });
        return { ok, cost };
      },

      refreshUsage: async () => {
        // Usage meter removed from product UI (may return later with a real API).
      },

      resetUsagePeriod: () => {
        const plan = get().usage.plan;
        const u = createUsage(plan);
        set({ usage: u });
        get().pushActivity({
          kind: "usage",
          title: "Billing period reset",
          detail: `${PLAN_LIMITS[plan].label} counters cleared`,
          status: "success",
        });
      },

      toggleConnector: (id) => {
        void get().connectConnector(id);
      },

      connectConnector: async (id) => {
        const c = get().connectors.find((x) => x.id === id);
        if (!c) return;

        // Disconnect path
        if (c.status === "connected") {
          if (id === "grok-xai") {
            get().clearGrokOAuth();
          }
          set((s) => ({
            connectors: s.connectors.map((row) =>
              row.id === id
                ? { ...row, status: "disconnected" as const, lastUsed: row.lastUsed }
                : row,
            ),
          }));
          get().pushActivity({
            kind: "connector",
            title: `Disconnected ${c.name}`,
            detail: "Connector turned off",
            status: "success",
          });
          return;
        }

        // Connect paths
        if (id === "grok-xai") {
          if (get().oauth?.accessToken || get().apiKey) {
            set((s) => ({
              connectors: s.connectors.map((row) =>
                row.id === id
                  ? { ...row, status: "connected" as const, lastUsed: Date.now() }
                  : row,
              ),
              grokConnected: true,
            }));
            get().pushActivity({
              kind: "connector",
              title: "Grok connected",
              detail: get().oauth?.email || "Session active",
              status: "success",
            });
            return;
          }
          set({ nav: "settings" });
          get().pushActivity({
            kind: "connector",
            title: "Connect Grok first",
            detail: "Settings → Connect with Grok OAuth",
            status: "failed",
          });
          return;
        }

        if (id === "desktop-host") {
          try {
            const { hostInfo } = await import("./host-client");
            const info = await hostInfo();
            if (info.bridge === "none" || !info.unsandboxed) {
              get().pushActivity({
                kind: "connector",
                title: "Desktop host offline",
                detail: "Relaunch the Electron desktop app for unsandboxed access",
                status: "failed",
              });
              set((s) => ({
                connectors: s.connectors.map((row) =>
                  row.id === id ? { ...row, status: "error" as const } : row,
                ),
              }));
              return;
            }
            set((s) => ({
              connectors: s.connectors.map((row) =>
                row.id === id
                  ? { ...row, status: "connected" as const, lastUsed: Date.now() }
                  : row,
              ),
            }));
            get().pushActivity({
              kind: "connector",
              title: "Desktop host connected",
              detail: `${info.user}@${info.hostname} · ${info.bridge}`,
              status: "success",
            });
          } catch (e) {
            get().pushActivity({
              kind: "connector",
              title: "Desktop host failed",
              detail: e instanceof Error ? e.message : "error",
              status: "failed",
            });
          }
          return;
        }

        if (id === "github") {
          const token = get().githubToken?.trim();
          if (!token) {
            set({ nav: "settings" });
            get().pushActivity({
              kind: "connector",
              title: "GitHub token required",
              detail: "Settings → Updates → paste a GitHub token (repo scope)",
              status: "failed",
            });
            return;
          }
          try {
            const res = await fetch("https://api.github.com/user", {
              headers: {
                authorization: `Bearer ${token}`,
                accept: "application/vnd.github+json",
                "user-agent": "GrokHub",
              },
            });
            if (!res.ok) throw new Error(`GitHub ${res.status}`);
            const user = (await res.json()) as { login?: string };
            set((s) => ({
              connectors: s.connectors.map((row) =>
                row.id === id
                  ? {
                      ...row,
                      status: "connected" as const,
                      lastUsed: Date.now(),
                      liveTools: true,
                      source: "token" as const,
                      accountLabel: user.login || row.accountLabel,
                    }
                  : row,
              ),
            }));
            get().pushActivity({
              kind: "connector",
              title: "GitHub connected",
              detail: user.login || "token ok",
              status: "success",
            });
          } catch (e) {
            get().pushActivity({
              kind: "connector",
              title: "GitHub connect failed",
              detail: e instanceof Error ? e.message : "error",
              status: "failed",
            });
          }
          return;
        }

        // Website-backed connectors — sync from linked Grok session (no fake connected)
        const websiteIds = new Set([
          "gmail",
          "gdrive",
          "google-calendar",
          "notion",
          "outlook",
          "outlook-calendar",
          "teams",
          "linear",
          "box",
          "canva",
          "stripe",
          "vercel",
        ]);
        if (websiteIds.has(id) || (c.source === "website" && id !== "github")) {
          if (!get().ssoCookie) {
            set({ nav: "settings" });
            get().pushActivity({
              kind: "connector",
              title: `Link Grok website for ${c.name}`,
              detail: "Settings → Link Grok website, then Connect again to sync Installed status",
              status: "failed",
            });
            return;
          }
          const synced = await get().syncWebsiteConnectors();
          const row = get().connectors.find((x) => x.id === id);
          if (row?.status === "connected") {
            get().pushActivity({
              kind: "connector",
              title: `${c.name} synced from website`,
              detail: row.accountLabel || synced.detail,
              status: "success",
            });
          } else {
            get().pushActivity({
              kind: "connector",
              title: `${c.name} not found on website`,
              detail:
                "Open grok.com → Skills and Connectors, connect it there, then re-sync (Connect again).",
              status: "failed",
            });
          }
          return;
        }

        // Planned / custom — open vendor home only (not marked connected)
        const homes: Record<string, string> = {
          "custom-mcp": "",
        };
        const url = homes[id];
        if (url && typeof window !== "undefined") {
          window.open(url, "_blank", "noopener,noreferrer");
        }
        get().pushActivity({
          kind: "connector",
          title: `${c.name}`,
          detail: "No local connector wiring for this id yet.",
          status: "queued",
        });
      },

      toggleSkill: (id) => {
        set((s) => ({
          skills: s.skills.map((sk) =>
            sk.id === id ? { ...sk, enabled: !sk.enabled } : sk,
          ),
        }));
      },

      addSkill: (input) => {
        const skill: Skill = {
          id: uid("skill"),
          name: input.name,
          description: input.description,
          kind: "custom",
          enabled: true,
          slash: input.slash.startsWith("/") ? input.slash : `/${input.slash}`,
          instructions: input.instructions,
          runs: 0,
        };
        set((s) => ({ skills: [skill, ...s.skills] }));
        get().pushActivity({
          kind: "skill",
          title: `Created skill ${skill.name}`,
          detail: skill.slash,
          status: "success",
        });
      },

      runSkill: async (id) => {
        const skill = get().skills.find((s) => s.id === id);
        if (!skill) return;
        if (get().running || get().streamingMessageId) {
          get().pushActivity({
            kind: "skill",
            title: `Skill busy: ${skill.name}`,
            detail: "Wait for the current agent turn to finish",
            status: "queued",
          });
          return;
        }
        set((s) => ({
          skills: s.skills.map((sk) =>
            sk.id === id ? { ...sk, runs: sk.runs + 1 } : sk,
          ),
          nav: "chat" as const,
        }));
        get().pushActivity({
          kind: "skill",
          title: `Running ${skill.name}`,
          detail: `${skill.slash} · live agent`,
          status: "running",
        });
        const prompt = [
          `[Skill: ${skill.name} ${skill.slash}]`,
          skill.instructions.trim(),
          "",
          "Execute this skill fully. Use HOST_CMD / CONNECTOR_CMD when real data is needed. Do not invent connector data.",
        ].join("\n");
        try {
          await get().sendChat(prompt);
          get().pushActivity({
            kind: "skill",
            title: `${skill.name} finished`,
            detail: skill.instructions.slice(0, 120),
            status: "success",
          });
        } catch (e) {
          get().pushActivity({
            kind: "skill",
            title: `${skill.name} failed`,
            detail: e instanceof Error ? e.message : "skill failed",
            status: "failed",
          });
        }
      },


      startWorkItem: async (id) => {
        const item = get().workboard.items.find((w) => w.id === id);
        if (!item) return;
        const project = get().projectWorkspace?.path;
        const prompt = [
          `[Workboard task: ${item.title}]`,
          item.detail ? `Detail: ${item.detail}` : "",
          project ? `Project cwd: ${project}` : "",
          "",
          "Work this task end-to-end. Prefer HOST_CMD for real files/shell.",
          "When finished, emit WORK_UPDATE: " + item.id + " status=done (or leave staged notes).",
          "If more work remains, say so clearly. Emit GOAL_COMPLETE when fully done.",
          "Do not invent data — use tools.",
        ]
          .filter(Boolean)
          .join("\n");
        get().setWorkItemStatus(id, "in_progress");
        // Always enqueue so busy agent doesn't drop work
        get().enqueueAgentJob({
          type: "workboard",
          priority: 8,
          title: item.title,
          prompt,
          workItemId: id,
          goalId: id,
          stepIndex: 0,
          maxSteps: get().autonomy.maxStepsPerGoal,
          maxRounds: 8,
        });
        set({ nav: "chat" });
        get().pushActivity({
          kind: "system",
          title: `Queued: ${item.title}`,
          detail: "Workboard → agent queue",
          status: "queued",
        });
        void get().processAgentQueue();
      },

      setAutonomy: (patch) => {
        set((s) => {
          let autonomy = rollBudgetDay({ ...s.autonomy, ...patch });
          const out = { autonomy };
          scheduleSettingsPersist();
          return out;
        });
        const a = get().autonomy;
        void agentCoreSync({ paused: a.paused, level: a.level, jobs: get().agentQueue.jobs });
        void agentCoreSetPaused(a.paused);
      },

      pauseAutonomy: (paused) => {
        get().setAutonomy({ paused });
        get().pushActivity({
          kind: "system",
          title: paused ? "Autonomy paused" : "Autonomy resumed",
          detail: paused ? "Job queue will not drain" : "Queue can run",
          status: paused ? "queued" : "success",
        });
      },

      enqueueAgentJob: (input) => {
        const id = input.id || uidJob();
        const cfg = get().autonomy;
        set((s) => ({
          agentQueue: enqueueJob(
            s.agentQueue,
            { ...input, id },
            cfg.maxQueue,
          ),
        }));
        const job = get().agentQueue.jobs.find((j) => j.id === id);
        if (job) void agentCoreEnqueue(job);
        return id;
      },

      cancelAgentJob: (id) => {
        set((s) => ({
          agentQueue: updateJob(s.agentQueue, id, { status: "cancelled" }),
        }));
        void agentCoreSync({ jobs: get().agentQueue.jobs });
      },

      approveAgentJob: (id, grant) => {
        set((s) => ({
          agentQueue: approveJob(s.agentQueue, id, grant),
        }));
        void agentCoreSync({ jobs: get().agentQueue.jobs });
        if (grant) void get().processAgentQueue();
      },

      claimWorkboardJobs: () => {
        const cfg = get().autonomy;
        if (!shouldAutoClaimWorkboard(cfg)) return 0;
        // Only pull new board cards — in_progress is owned by the active/goal loop
        // so we don't re-queue the same item forever after a completed step.
        const open = get().workboard.items.filter((w) =>
          ["approved", "staged"].includes(w.status),
        );
        let n = 0;
        for (const item of open.slice(0, 5)) {
          const already = get().agentQueue.jobs.some(
            (j) =>
              j.workItemId === item.id &&
              ["queued", "running", "waiting_user"].includes(j.status),
          );
          if (already) continue;
          // Skip if we already completed a job for this item recently without a new approval
          const recentlyDone = get().agentQueue.jobs.some(
            (j) =>
              j.workItemId === item.id &&
              j.status === "done" &&
              Date.now() - (j.updatedAt || 0) < 60_000,
          );
          if (recentlyDone) continue;
          get().enqueueAgentJob({
            type: "workboard",
            priority: item.priority === "high" ? 9 : 6,
            title: item.title,
            prompt: buildGoalStepPrompt({
              workItem: item,
              stepIndex: 0,
              maxSteps: cfg.maxStepsPerGoal,
            }),
            workItemId: item.id,
            goalId: item.id,
            stepIndex: 0,
            maxSteps: cfg.maxStepsPerGoal,
          });
          // Mark claimed so we don't re-claim until user re-approves
          get().setWorkItemStatus(item.id, "in_progress");
          n++;
        }
        return n;
      },

      _runAgentJob: async (job) => {
        set((s) => ({
          agentQueue: updateJob(s.agentQueue, job.id, { status: "running" }),
        }));
        try {
          if (job.threadId) {
            try {
              get().selectThread(job.threadId);
            } catch {
              /* ignore */
            }
          } else {
            set({ nav: "chat" });
          }
          await get().sendChat(job.prompt);
          const last = [...get().chat].reverse().find((m) => m.role === "assistant");
          const text = last?.content || "";
          const outcome = parseGoalOutcome(text);
          set((s) => ({
            agentQueue: updateJob(s.agentQueue, job.id, {
              status: outcome === "blocked" ? "waiting_user" : "done",
              resultSummary: text.slice(0, 240),
              needsApproval: outcome === "blocked",
              approval: outcome === "blocked" ? "pending" : undefined,
            }),
          }));
          if (job.workItemId) {
            if (outcome === "complete") {
              get().setWorkItemStatus(job.workItemId, "done");
            } else if (outcome === "blocked") {
              /* leave in progress */
            }
          }
          // Goal resume level 4
          if (
            outcome === "continue" &&
            shouldAutoGoalResume(get().autonomy) &&
            (job.type === "workboard" || job.type === "goal_step")
          ) {
            const step = (job.stepIndex || 0) + 1;
            const maxSteps = job.maxSteps || get().autonomy.maxStepsPerGoal;
            if (step < maxSteps) {
              get().enqueueAgentJob({
                type: "goal_step",
                priority: (job.priority || 5) + 1,
                title: `${job.title} · step ${step + 1}`,
                prompt: buildGoalStepPrompt({
                  workItem: job.workItemId
                    ? get().workboard.items.find((w) => w.id === job.workItemId) || null
                    : null,
                  priorSummary: text,
                  stepIndex: step,
                  maxSteps,
                }),
                workItemId: job.workItemId,
                goalId: job.goalId || job.workItemId,
                parentId: job.id,
                stepIndex: step,
                maxSteps,
                threadId: get().activeThreadId,
              });
            }
          }
        } catch (e) {
          const msg = e instanceof Error ? e.message : "job failed";
          const fails = (job.failCount || 0) + 1;
          const breakCircuit = fails >= get().autonomy.circuitBreakerFails;
          set((s) => ({
            agentQueue: updateJob(s.agentQueue, job.id, {
              status: breakCircuit ? "failed" : "queued",
              failCount: fails,
              lastError: msg,
              notBefore: breakCircuit ? undefined : Date.now() + fails * 30_000,
            }),
          }));
        } finally {
          set((s) => ({
            agentQueue: {
              ...s.agentQueue,
              runningId: s.agentQueue.runningId === job.id ? null : s.agentQueue.runningId,
            },
          }));
          void agentCoreSync({ jobs: get().agentQueue.jobs });
        }
      },

      processAgentQueue: async () => {
        if (agentQueueDraining) return;
        const cfg0 = rollBudgetDay(get().autonomy);
        if (cfg0 !== get().autonomy) set({ autonomy: cfg0 });
        if (get().autonomy.paused) return;
        if (get().running || get().streamingMessageId) return;
        if (get().agentQueue.runningId) return;
        agentQueueDraining = true;
        try {
          // Level 3+ auto claim (approved/staged only)
          if (shouldAutoClaimWorkboard(get().autonomy)) {
            get().claimWorkboardJobs();
          }
          const next = pickNextJob(get().agentQueue, get().autonomy);
          if (!next) return;
          await get()._runAgentJob(next);
        } finally {
          agentQueueDraining = false;
        }
        // chain next if idle
        if (!get().running && !get().streamingMessageId && !get().autonomy.paused) {
          queueMicrotask(() => {
            void get().processAgentQueue();
          });
        }
      },


      toggleAutomation: (id) => {
        set((s) => ({
          automations: s.automations.map((a) => {
            if (a.id !== id) return a;
            const enabled = !a.enabled;
            if (!enabled) return { ...a, enabled, nextRun: undefined };
            return {
              ...a,
              enabled,
              nextRun: computeNextRun(
                a.schedule,
                a.time,
                Date.now(),
                a.lastRun,
                a.times,
                a.heartbeatEveryMin,
              ),
            };
          }),
        }));
      },

      runAutomation: async (id) => {
        const auto = get().automations.find((a) => a.id === id);
        if (!auto) return;
        if (get().running || get().streamingMessageId || get().agentQueue.runningId) {
          if (shouldQueueWhenBusy(get().autonomy.level)) {
            get().enqueueAgentJob({
              type: "automation",
              priority: 7,
              title: auto.name,
              prompt: `[Automation: ${auto.name}]\n${auto.instructions}`,
              automationId: auto.id,
            });
            get().pushActivity({
              kind: "automation",
              title: `Queued: ${auto.name}`,
              detail: "Agent busy — enqueued",
              status: "queued",
            });
            return;
          }
          get().pushActivity({
            kind: "automation",
            title: `Skipped: ${auto.name}`,
            detail: "Agent is busy — will retry on next schedule tick",
            status: "queued",
          });
          return;
        }
        const routed = resolveMode(get().mode, auto.instructions);
        const m = getMode(routed);
        const bill = get().recordUsage("automation", routed);
        if (!bill.ok) return;
        // IMPORTANT: do not set running:true here — sendChat owns the run flag.
        // Setting it first caused sendChat to no-op (deadlock) while reporting success.
        get().setAgentStatus("ops", "working", 1);
        get().pushActivity({
          kind: "automation",
          title: `Automation started: ${auto.name}`,
          detail: `${auto.instructions.slice(0, 100)} · ${m.label} · ${bill.cost}u`,
          status: "running",
        });
        let summary = "";
        let ok = true;
        try {
          const canChat = Boolean(
            get().oauth?.accessToken ||
              get().apiKey ||
              get().ssoCookie ||
              false, /* free grok off */
          );
          if (canChat) {
            await get().sendChat(
              `[Automation: ${auto.name}]\n${auto.instructions}`,
            );
            summary = "Ran via agent chat";
          } else {
            summary = "Not connected to Grok — automation recorded only";
            ok = false;
          }
        } catch (e) {
          ok = false;
          summary = e instanceof Error ? e.message : "automation failed";
        }
        const { computeNextRun } = await import("./automation-schedule");
        const threadId = get().activeThreadId;
        const prevFail = auto.failCount || 0;
        const failCount = ok ? 0 : prevFail + 1;
        // Pause after 3 consecutive failures (non-once schedules)
        const pauseFails = !ok && failCount >= 3 && auto.schedule !== "once";
        set((s) => ({
          automations: s.automations.map((a) =>
            a.id === id
              ? {
                  ...a,
                  lastRun: Date.now(),
                  runCount: a.runCount + 1,
                  failCount,
                  lastThreadId: threadId || a.lastThreadId || null,
                  nextRun:
                    a.schedule === "once" || pauseFails
                      ? undefined
                      : computeNextRun(
                          a.schedule,
                          a.time,
                          Date.now(),
                          Date.now(),
                          a.times,
                          a.heartbeatEveryMin,
                        ),
                  enabled:
                    a.schedule === "once" || pauseFails ? false : a.enabled,
                }
              : a,
          ),
        }));
        if (pauseFails) {
          get().pushActivity({
            kind: "automation",
            title: `Paused: ${auto.name}`,
            detail: `${failCount} consecutive failures — re-enable in Automations`,
            status: "failed",
          });
        }
        get().setAgentStatus("ops", "idle", 0);
        get().pushActivity({
          kind: "automation",
          title: ok
            ? `Automation completed: ${auto.name}`
            : `Automation failed: ${auto.name}`,
          detail: summary,
          status: ok ? "success" : "failed",
        });
      },

      tickAutomations: async (opts) => {
        const {
          dueAutomations,
          dueHeartbeatAutomations,
          ensureAutomationSchedule,
        } = await import("./automation-schedule");
        const now = Date.now();
        set((s) => ({
          automations: s.automations.map((a) => ensureAutomationSchedule(a, now)),
          heartbeatAt: opts?.heartbeatOnly ? s.heartbeatAt : s.heartbeatAt,
        }));
        const list = get().automations;
        const due = opts?.heartbeatOnly
          ? dueHeartbeatAutomations(list, now)
          : dueAutomations(list, now);
        for (const a of due.slice(0, 3)) {
          // enqueue-friendly; processAgentQueue drains
          if (get().running && !shouldQueueWhenBusy(get().autonomy.level)) break;
          await get().runAutomation(a.id);
        }
        void get().processAgentQueue();
      },

      addAutomation: (input) => {
        const times = (input.times && input.times.length
          ? input.times
          : [input.time || "09:00"]
        )
          .map((x) => String(x).trim())
          .filter(Boolean);
        const unique = Array.from(new Set(times));
        const primary = unique[0] || "09:00";
        const auto: Automation = {
          id: uid("auto"),
          name: input.name,
          instructions: input.instructions,
          schedule: input.schedule,
          time: primary,
          times: unique,
          heartbeatEveryMin: input.heartbeatEveryMin ?? 5,
          enabled: true,
          connectorIds: get()
            .connectors.filter((c) => c.status === "connected")
            .slice(0, 2)
            .map((c) => c.id),
          skillIds: [],
          runCount: 0,
          nextRun: computeNextRun(
            input.schedule,
            primary,
            Date.now(),
            undefined,
            unique,
            input.heartbeatEveryMin ?? 5,
          ),
        };
        set((s) => ({ automations: [auto, ...s.automations] }));
        get().scheduleSetupAutoPush();
        get().pushActivity({
          kind: "automation",
          title: `Created automation ${auto.name}`,
          detail:
            auto.schedule === "heartbeat"
              ? `heartbeat every ${auto.heartbeatEveryMin || 5}m`
              : `${auto.schedule} @ ${unique.join(", ")}`,
          status: "success",
        });
      },

      stopChat: () => {
        const gen = ++chatGeneration;
        try {
          if (hostConfirmWaiter) {
            hostConfirmWaiter(false);
            hostConfirmWaiter = null;
          }
        } catch {
          /* ignore */
        }
        try {
          activeChatAbort?.abort();
        } catch {
          /* ignore */
        }
        activeChatAbort = null;
        const killIds = [...activeHostJobIds];
        if (activeHostJobId) killIds.push(activeHostJobId);
        activeHostJobIds.clear();
        activeHostJobId = null;
        for (const killId of [...new Set(killIds)]) {
          void import("./host-client").then(({ hostKillExec }) => hostKillExec(killId)).catch(() => {});
        }
        try {
          void window.grokhubDesktop?.grok?.stopChatStream?.();
        } catch {
          /* ignore */
        }
        void import("./persistent-storage").then(({ setPersistPaused }) => setPersistPaused(false));
        const sid = get().streamingMessageId;
        const tid = get().activeThreadId;
        let partial = "";
        let pendingPrompt = "";
        set((s) => {
          const chat = s.chat.map((m) => {
            if (m.id === sid) {
              const base = (m.content || "").trim();
              return {
                ...m,
                streaming: false,
                stopped: true,
                content: base ? `${base}\n_Stopped._` : "_Stopped._",
              };
            }
            if (m.streaming) return { ...m, streaming: false };
            return m;
          });
          const stopped = chat.find((m) => m.id === sid);
          partial = (stopped?.content || "").replace(/\n*_Stopped\._\s*$/m, "").trim();
          const lastUser = [...chat].reverse().find((m) => m.role === "user");
          pendingPrompt = lastUser?.content || "";
          const threads = s.threads.map((th) =>
            th.id === tid ? threadWithMessages(th, chat) : th,
          );
          const th = threads.find((x) => x.id === tid);
          return {
            chat,
            threads,
            running: false,
            streamStatus: null,
            streamingMessageId: null,
            pendingHostConfirm: null,
            // Only interrupt creates the continue banner
            sessionResume:
              tid && pendingPrompt
                ? {
                    kind: "interrupted" as const,
                    threadId: tid,
                    title: th?.title || "Interrupted chat",
                    preview: (partial || pendingPrompt).slice(0, 160),
                    mode: s.mode,
                    ts: Date.now(),
                    pendingPrompt,
                    partialContent: partial || undefined,
                    stoppedMessageId: sid || undefined,
                  }
                : s.sessionResume,
          };
        });
        get().setAgentStatus("primary", "idle", 0);
        get().setAgentStatus("builder", "idle", 0);
        get().setAgentStatus("research", "idle", 0);
        get().setAgentStatus("ops", "idle", 0);
        get().pushActivity({
          kind: "chat",
          title: "Stopped — continue when ready",
          detail: pendingPrompt
            ? pendingPrompt.slice(0, 100)
            : "User interrupted the agent",
          status: "failed",
        });
        void gen;
      },

      sendChat: async (text) => {
        let trimmed = text.trim();
        if (!trimmed) return;
        if (get().running) {
          // Already running — ignore new sends (use Stop first)
          return;
        }

        // Slash commands (local, instant)
        const slash = trimmed.match(/^\/([a-zA-Z_-]+)(?:\s+([\s\S]*))?$/);
        if (slash) {
          const cmd = slash[1]!.toLowerCase();
          const arg = (slash[2] || "").trim();
          if (cmd === "help") {
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: slashHelpMarkdown(),
                  ts: Date.now(),
                },
              ],
              nav: "chat",
            }));
            return;
          }
          if (cmd === "new") {
            get().newThread();
            return;
          }
          if (cmd === "clear") {
            get().clearChat();
            return;
          }
          if (cmd === "imagine") {
            set({ nav: "imagine", imaginePrompt: arg || get().imaginePrompt || "" });
            return;
          }
          if (cmd === "export") {
            const md = get().exportThreadMarkdown();
            if (typeof window !== "undefined") {
              const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
              const a = document.createElement("a");
              a.href = URL.createObjectURL(blob);
              a.download = `grokhub-chat-${Date.now()}.md`;
              a.click();
              URL.revokeObjectURL(a.href);
            }
            get().pushActivity({
              kind: "chat",
              title: "Exported Markdown",
              detail: "Downloaded current chat",
              status: "success",
            });
            return;
          }
          if (cmd === "rename") {
            const title = arg.trim();
            if (!title) {
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: "Usage: `/rename Short title` (locks auto-rename for this chat)",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            const tid = get().activeThreadId;
            if (tid) {
              get().renameThread(tid, title);
            }
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Chat renamed to **${title.slice(0, 80)}** (auto-rename off for this chat).`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
          if (cmd === "remember") {
            const note = arg.trim();
            if (!note) {
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: "Usage: `/remember durable fact or preference`",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            // reuse memory append path
            const line = `- ${new Date().toISOString().slice(0, 10)}: ${note}`;
            const prev = get().agentPrefs.memoryNotes || "";
            get().setAgentPrefs({ memoryNotes: prev ? `${prev}\n${line}` : line });
            await memoryAppend("memory", note);
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Saved to **MEMORY.md**:\n${line}`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
          if (cmd === "project") {
            const sub = (arg || "").trim();
            if (/^clear$/i.test(sub)) {
              get().clearProjectWorkspace();
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: "Project workspace unbound.",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            if (/^bind(\s|$)/i.test(sub) || sub.startsWith("/") || sub.startsWith("~")) {
              let pathArg = sub.replace(/^bind\s*/i, "").trim();
              if (!pathArg && typeof window !== "undefined" && window.grokhubDesktop?.pickFolder) {
                const picked = await window.grokhubDesktop.pickFolder();
                if (picked?.ok && picked.path) pathArg = picked.path;
                else if (picked?.canceled) return;
              }
              if (!pathArg) {
                set((s) => ({
                  chat: [
                    ...s.chat,
                    { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                    {
                      id: uid("msg"),
                      role: "system",
                      content: "Usage: `/project bind` (picker) or `/project bind /path/to/folder`",
                      ts: Date.now(),
                    },
                  ],
                }));
                return;
              }
              const r = await get().bindProjectWorkspace(pathArg);
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: r.ok
                      ? `**Project bound:** \`${pathArg}\`\n\n${r.detail}`
                      : `**Bind failed:** ${r.detail}`,
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            const ws = get().projectWorkspace;
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: ws?.path
                    ? `**Project:** ${ws.name}\n\`${ws.path}\`\n\n_/project bind · /project clear_`
                    : "No project bound. `/project bind` to pick a folder.",
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
          if (cmd === "host") {
            const sub = (arg || "").trim().toLowerCase();
            if (sub === "on" || sub === "off") {
              const on = sub === "on";
              get().setAgentPrefs({ hostToolsEnabled: on });
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: on ? "Host tools **enabled**." : "Host tools **disabled**.",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            let info = "Desktop host status unavailable (browser-only?).";
            try {
              const { hostInfo } = await import("./host-client");
              const h = await hostInfo();
              const live = h?.bridge && h.bridge !== "none";
              const proj = get().projectWorkspace;
              info = [
                "**Desktop host**",
                "",
                `- Live: **${live ? "yes" : "no"}** (${h?.bridge || "unknown"})`,
                h?.platform ? `- Platform: ${h.platform}` : "",
                h?.homedir ? `- Home: \`${h.homedir}\`` : "",
                proj?.path ? `- Project: \`${proj.path}\`` : "- Project: _(none)_",
                `- Tools: ${get().agentPrefs.hostToolsEnabled ? "on" : "off"}`,
                `- Confirm: ${
                  !get().desktop.confirmHostCommands
                    ? "off"
                    : get().desktop.confirmDestructiveOnly
                      ? "risky only"
                      : "all commands"
                }`,
                `- Safe mode: ${get().desktop.hostSafeMode ? "on" : "off"}`,
              ]
                .filter(Boolean)
                .join("\n");
            } catch (e) {
              info = `Host probe failed: ${e instanceof Error ? e.message : e}`;
            }
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                { id: uid("msg"), role: "system", content: info, ts: Date.now() },
              ],
            }));
            return;
          }
          if (cmd === "approve") {
            const sub = (arg || "").trim().toLowerCase();
            if (sub === "off" || sub === "none" || sub === "auto") {
              get().setDesktop({ confirmHostCommands: false, confirmDestructiveOnly: true });
            } else if (sub === "risky" || sub === "safe" || sub === "destructive") {
              get().setDesktop({ confirmHostCommands: true, confirmDestructiveOnly: true });
            } else if (sub === "all" || sub === "always") {
              get().setDesktop({ confirmHostCommands: true, confirmDestructiveOnly: false });
            } else {
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: "Usage: `/approve off` | `/approve risky` | `/approve all`",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            const mode = !get().desktop.confirmHostCommands
              ? "off (auto-run)"
              : get().desktop.confirmDestructiveOnly
                ? "risky only"
                : "all commands";
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Host approval mode → **${mode}**`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
                    if (cmd === "mode" && arg) {
            const resolved = resolveModeArg(arg);
            if (resolved) {
              get().setMode(resolved as import("./types").GrokModeId);
              get().pushActivity({
                kind: "system",
                title: `Mode → ${resolved}`,
                detail: arg !== resolved ? `alias ${arg}` : "slash",
                status: "success",
              });
              return;
            }
          }
          if (cmd === "health" || cmd === "fix") {
            const r = await get().runHealthCheck();
            if (cmd === "fix") {
              const h = await get().runProactiveHousekeeping();
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: `**Self-heal**\n\n${r.detail}\n\n_Housekeeping: ${h.detail}_`,
                    ts: Date.now(),
                  },
                ],
                nav: "chat",
              }));
            } else {
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: r.detail,
                    ts: Date.now(),
                  },
                ],
                nav: "chat",
              }));
            }
            return;
          }
          if (cmd === "stop") {
            try {
              get().stopChat();
            } catch {
              /* ignore */
            }
            return;
          }
          if (cmd === "memory") {
            const sub = arg.match(/^(show|user|today|list)\s*$/i)
              ? arg.toLowerCase()
              : arg.match(/^(user|today|memory)\s+([\s\S]+)$/i)
                ? null
                : null;
            // /memory show | /memory user | /memory today | /memory list
            // /memory user <note> | /memory today <note> | /memory <note>
            const showMatch = arg.match(/^(show|list|user|today)$/i);
            const targetMatch = arg.match(/^(user|today|memory)\s+([\s\S]+)$/i);
            if (showMatch || !arg) {
              const kind = (showMatch?.[1] || "show").toLowerCase();
              let body = "";
              if (kind === "list" || kind === "show") {
                const mem = await memoryRead("MEMORY.md");
                const user = await memoryRead("USER.md");
                const today = await memoryRead("today");
                const legacy = get().agentPrefs.memoryNotes || "";
                body = [
                  "**File memory** (survives updates)",
                  "",
                  "### USER.md",
                  (user.content || "_empty_").slice(0, 2000),
                  "",
                  "### MEMORY.md",
                  (mem.content || "_empty_").slice(0, 3000),
                  "",
                  "### Today",
                  (today.content || "_empty_").slice(0, 1500),
                  legacy
                    ? "\n### Legacy app notes\n" + legacy.slice(0, 1000)
                    : "",
                  "",
                  "_Write: `/memory note` · `/memory user …` · `/memory today …` · Settings → Memory_",
                ].join("\n");
              } else if (kind === "user") {
                const user = await memoryRead("USER.md");
                body = "**USER.md**\n\n" + (user.content || "_empty_");
              } else if (kind === "today") {
                const today = await memoryRead("today");
                body = "**Today**\n\n" + (today.content || "_empty_");
              }
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: body,
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            const dest = targetMatch
              ? targetMatch[1]!.toLowerCase()
              : "memory";
            const note = targetMatch ? targetMatch[2]!.trim() : arg;
            const rel =
              dest === "user" ? "user" : dest === "today" ? "today" : "memory";
            const line = `- ${new Date().toISOString().slice(0, 10)}: ${note}`;
            const prev = get().agentPrefs.memoryNotes || "";
            get().setAgentPrefs({
              memoryNotes: prev ? `${prev}\n${line}` : line,
            });
            await memoryAppend(rel, note);
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Saved to file memory (**${rel === "memory" ? "MEMORY.md" : rel}**):\n${line}`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
if (cmd === "context") {
            const stats = get().getContextStats();
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: stats.report,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
          if (cmd === "compact") {
            const r = get().compactThread();
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: r.ok
                    ? `**Context compacted**\n\n${r.detail}\n\n${get().getContextStats().report}`
                    : `**Compact skipped**\n\n${r.detail}`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
          if (cmd === "learn") {
            const sub = (arg || "").trim();
            if (/^reflect$/i.test(sub) || /^improve$/i.test(sub)) {
              const r = await get().runSelfImprove();
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: `**Self-improve**\n\n${r.detail}`,
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            if (/^clear$/i.test(sub)) {
              get().clearLearning();
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: "Learning history cleared.",
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            if (/^(note|pref)\s+/i.test(sub) || (sub && !/^(show|stats)?$/i.test(sub))) {
              const note = sub.replace(/^(note|pref)\s+/i, "").trim() || sub;
              set({
                learning: pushLearningEvent(get().learning, {
                  kind: "pref",
                  summary: note,
                  polarity: 1,
                  tags: ["manual"],
                }),
              });
              await memoryAppend("memory", note);
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: `Learned preference:\n- ${note}`,
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            const L = get().learning;
            const lines = [
              "**Learning & self-improvement**",
              "",
              learningSummaryLine(L),
              L.lastReflectionAt
                ? `Last reflect: ${new Date(L.lastReflectionAt).toLocaleString()}`
                : "No reflection yet — run `/learn reflect`",
              "",
              "### Top insights",
              ...(L.insights.slice(0, 8).map((i) => `- (${Math.round(i.confidence * 100)}%) ${i.text}`) ||
                ["_None yet_"]),
              "",
              "### Recent events",
              ...L.events.slice(-6).reverse().map((e) => `- ${e.summary}`),
              "",
              "_`/learn reflect` · `/learn note …` · `/learn clear` · rate replies with 👍/👎_",
            ];
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: lines.join("\n"),
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }

          
          if (cmd === "board" || cmd === "work" || cmd === "workboard") {
            const sub = (arg || "").trim();
            if (/^add\s+/i.test(sub) || (!sub.startsWith("list") && sub.includes("|")) || (/^[^+]/.test(sub) && sub && !/^(list|open)?$/i.test(sub) && !/^dismiss/i.test(sub))) {
              const body = sub.replace(/^add\s+/i, "");
              const parts = body.split("|").map((s) => s.trim());
              get().pinWorkItem({
                title: parts[0] || body,
                detail: parts[1] || "",
                priority: /high/i.test(parts[2] || "") ? "high" : "normal",
                source: "user",
              });
              set((s) => ({
                chat: [
                  ...s.chat,
                  { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                  {
                    id: uid("msg"),
                    role: "system",
                    content: `Pinned to workboard: **${parts[0] || body}**`,
                    ts: Date.now(),
                  },
                ],
              }));
              return;
            }
            set({ nav: "workboard" });
            const open = get().workboard.items.filter((i) =>
              ["proposed", "approved", "staged", "in_progress"].includes(i.status),
            );
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: [
                    "**Workboard**",
                    "",
                    open.length
                      ? open
                          .slice(0, 12)
                          .map((i) => `- [${i.status}] ${i.title}`)
                          .join("\n")
                      : "_No open items — agent can WORK_PIN: tasks or use /board add …_",
                    "",
                    "Opened Workboard view.",
                  ].join("\n"),
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
if (cmd === "tools") {
            const on = !/off|false|0|no/i.test(arg || "on");
            get().setAgentPrefs({ hostToolsEnabled: on, connectorToolsEnabled: on });
            set((s) => ({
              chat: [
                ...s.chat,
                { id: uid("msg"), role: "user", content: trimmed, ts: Date.now() },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Agent tools ${on ? "**enabled**" : "**disabled**"} (host + connectors).`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
        }

        // Instant feedback BEFORE routing / network (kills perceived latency)
        const mode = get().mode;
        const replyTarget = get().replyTo;
        const userMsg: ChatMessage = {
          id: uid("msg"),
          role: "user",
          content: trimmed,
          ts: Date.now(),
          mode,
          ...(replyTarget
            ? {
                replyToId: replyTarget.id,
                replyToPreview: replyTarget.preview,
                replyToRole: replyTarget.role,
              }
            : {}),
        };
        // Clear composer reply chip once we commit the message
        if (replyTarget) set({ replyTo: null });
        const botId = uid("msg");
        const botPlaceholder: ChatMessage = {
          id: botId,
          role: "assistant",
          content: "",
          ts: Date.now(),
          mode,
          streaming: true,
        };
        try {
          activeChatAbort?.abort();
        } catch {
          /* ignore */
        }
        const abort = new AbortController();
        activeChatAbort = abort;
        const gen = ++chatGeneration;
        const pendingMemoryNotes: string[] = [];

        void import("./persistent-storage").then(({ setPersistPaused }) => setPersistPaused(true));
        streamStartedAt = Date.now();
        set((s) => {
          const chat = [
            ...s.chat.map((m) =>
              m.streaming ? { ...m, streaming: false } : m,
            ),
            userMsg,
            botPlaceholder,
          ];
          const tid = s.activeThreadId;
          return {
            chat,
            running: true,
            streamStatus: "Thinking…",
            streamingMessageId: botId,
            nav: "chat" as const,
            threads: s.threads.map((th) =>
              th.id === tid ? threadWithMessages(th, chat, { mode: s.mode }) : th,
            ),
          };
        });
        // Offload large embedded images from stored user message (desktop)
        if (trimmed.length > 32_000 && /data:image\//.test(trimmed)) {
          void import("./chat-media").then(async ({ compactMessageMedia }) => {
            try {
              const compact = await compactMessageMedia(trimmed);
              if (compact !== trimmed) {
                set((s) => {
                  const chat = s.chat.map((m) =>
                    m.id === userMsg.id ? { ...m, content: compact } : m,
                  );
                  const tid = s.activeThreadId;
                  return {
                    chat,
                    threads: s.threads.map((th) =>
                      th.id === tid ? threadWithMessages(th, chat) : th,
                    ),
                  };
                });
              }
            } catch {
              /* ignore */
            }
          });
        }
        // Yield a frame so the indicator paints before heavier work
        await new Promise<void>((r) => {
          if (typeof requestAnimationFrame === "function") {
            requestAnimationFrame(() => r());
          } else {
            setTimeout(() => r(), 0);
          }
        });

        const catalog = get().modelCatalog || emptyCatalog();
        const recentChat = get().chat.filter((c) => c.id !== botId);
        const lastAsst = [...recentChat].reverse().find((c) => c.role === "assistant");
        const usageSnap = get().usage;
        const websitePct =
          usageSnap?.website && typeof usageSnap.website.creditUsagePercent === "number"
            ? Math.min(1, Math.max(0, usageSnap.website.creditUsagePercent / 100))
            : 0;
        const localPct = Math.min(1, Number(usageSnap?.usedUnits || 0) / 400);
        const usagePressure = Math.max(websitePct, localPct);
        const recentHost = recentChat
          .slice(-6)
          .some((c) =>
            /HOST_CMD|Desktop host|host ok|host failed|### 🖥️/i.test(c.content || ""),
          );
        const lastFailed =
          /could not reach|connection error|timed out|request failed|multi agent|not allowed on chat/i.test(
            lastAsst?.content || "",
          );
        // Prefer last assistant that actually has a routeTier (skips system/tool noise)
        const lastRoutedAsst = [...recentChat]
          .reverse()
          .find((c) => c.role === "assistant" && c.routeTier);
        const routeCtx = {
          historyTurns: recentChat.length,
          learningBias: routeLearningBias(get().learning),
          recentUserText: recentChat
            .filter((c) => c.role === "user")
            .slice(-3)
            .map((c) => c.content)
            .join("\n"),
          recentAssistantText: recentChat
            .filter((c) => c.role === "assistant")
            .slice(-2)
            .map((c) => c.content)
            .join("\n")
            .slice(0, 4000),
          hasAttachments: /\[attachment:|data:image\//i.test(trimmed),
          lastRouteTier: lastRoutedAsst?.routeTier || lastAsst?.routeTier,
          lastRoutedMode:
            lastRoutedAsst?.mode === "fast" ||
            lastRoutedAsst?.mode === "balanced" ||
            lastRoutedAsst?.mode === "expert" ||
            lastRoutedAsst?.mode === "heavy" ||
            lastRoutedAsst?.mode === "max" ||
            lastRoutedAsst?.mode === "build"
              ? lastRoutedAsst.mode
              : lastRoutedAsst?.routeTier === "fast"
                ? ("fast" as const)
                : lastRoutedAsst?.routeTier === "balanced"
                  ? ("balanced" as const)
                  : lastRoutedAsst?.routeTier === "build"
                    ? ("build" as const)
                    : lastRoutedAsst?.routeTier === "deep"
                      ? ("heavy" as const)
                      : lastRoutedAsst?.routeTier === "think"
                        ? ("expert" as const)
                        : lastAsst?.mode === "fast" ||
                            lastAsst?.mode === "balanced" ||
                            lastAsst?.mode === "expert" ||
                            lastAsst?.mode === "heavy" ||
                            lastAsst?.mode === "max" ||
                            lastAsst?.mode === "build"
                          ? lastAsst.mode
                          : undefined,
          usagePressure,
          preferFree: false,
          lastRouteFailed: lastFailed,
          usedHostRecently: recentHost,
        };
        const overrides = cleanModelOverrides(get().modelOverrides);
        const auto = autoRouteFor(trimmed, catalog, routeCtx, overrides);
        if (mode === "auto" && auto.openImagine) {
          set((s) => ({
            chat: s.chat.filter((m) => m.id !== botId && m.id !== userMsg.id),
            running: false,
            streamStatus: null,
            streamingMessageId: null,
            nav: "imagine",
            imaginePrompt: trimmed,
          }));
          if (activeChatAbort === abort) activeChatAbort = null;
          endChatTurnPersist();
          return;
        }
        const routed = resolveModeWithCatalog(mode, trimmed, catalog, routeCtx);
        const m = getMode(routed);
        // Soft quota check (real token units settled after live reply)
        {
          const u = ensurePeriod(get().usage);
          const est = costFor("message", routed);
          if (u.usedUnits + est > PLAN_LIMITS[u.plan].units * 1.02) {
            set((s) => ({
              chat: s.chat.map((row) =>
                row.id === botId
                  ? {
                      ...row,
                      streaming: false,
                      role: "system" as const,
                      content: `Quota exceeded on ${PLAN_LIMITS[u.plan].label}. Wait for period reset or switch plan in Settings.`,
                    }
                  : row,
              ),
              running: false,
              streamStatus: null,
              streamingMessageId: null,
            }));
            if (activeChatAbort === abort) activeChatAbort = null;
            endChatTurnPersist();
            return;
          }
        }
        let bill = { ok: true, cost: costFor("message", routed) };

        const routeStamp =
          mode === "auto"
            ? {
                mode: routed,
                routeTier: auto.tier,
                routeReason: auto.reasonDetail,
                routeModel: auto.modelId,
              }
            : {
                mode: routed,
                routeTier:
                  routed === "fast"
                    ? ("fast" as const)
                    : routed === "balanced"
                      ? ("balanced" as const)
                      : routed === "build"
                        ? ("build" as const)
                        : routed === "heavy" || routed === "max"
                          ? ("deep" as const)
                          : ("think" as const),
                routeReason: `Manual ${m.label} mode`,
                routeModel: modelIdForMode(mode, trimmed, catalog, routeCtx, overrides),
              };

        set((s) => ({
          chat: s.chat.map((row) =>
            row.id === botId
              ? { ...row, ...routeStamp }
              : row.id === userMsg.id
                ? { ...row, mode }
                : row,
          ),
          streamStatus:
            mode === "auto"
              ? `Adaptive → ${auto.tierLabel} · ${auto.reasonDetail}`
              : `Thinking · ${m.label}…`,
        }));

        if (get().agents.length === 0) {
          // Non-blocking profile sync — don't delay first token
          void get().syncFromGrok();
        }
        // Keep OAuth access token fresh (proactive ~30m before 6h expiry)
        if (get().oauth?.refreshToken) {
          try {
            await get().refreshOAuthSession();
          } catch {
            /* non-fatal — chat may still use current token / free fallback */
          }
        }
        get().setAgentStatus(
          routed === "build"
            ? "builder"
            : routed === "heavy" || routed === "max"
              ? "research"
              : "primary",
          "working",
          1,
        );
        // UI-only status dots — never emit multi-agent API requests

        const patchBot = (content: string, extra?: Partial<ChatMessage>) => {
          if (gen !== chatGeneration) return;
          set((s) => ({
            chat: s.chat.map((row) =>
              row.id === botId ? { ...row, content, ...extra } : row,
            ),
          }));
        };

        // Expand enabled skill slash into full instructions for the live agent
        {
          const slashMatch = trimmed.match(/^\/([a-zA-Z0-9_-]+)(?:\s+([\s\S]*))?$/);
          if (slashMatch) {
            const cmd = slashMatch[1]!.toLowerCase();
            const arg = (slashMatch[2] || "").trim();
            const skill = get().skills.find(
              (sk) =>
                sk.enabled &&
                sk.slash.replace(/^\//, "").toLowerCase() === cmd,
            );
            if (skill) {
              trimmed = [
                `[Skill: ${skill.name} /${cmd}]`,
                skill.instructions.trim(),
                arg ? `\nUser notes: ${arg}` : "",
                "",
                "Execute this skill. Use real tools; never invent connector/inbox data.",
              ]
                .filter(Boolean)
                .join("\n");
              // Keep user bubble readable as the slash; agent sees expanded prompt
              set((s) => ({
                chat: s.chat.map((row) =>
                  row.id === userMsg.id
                    ? {
                        ...row,
                        content: arg
                          ? `${skill.slash} ${arg}`
                          : skill.slash,
                      }
                    : row,
                ),
                skills: s.skills.map((sk) =>
                  sk.id === skill.id ? { ...sk, runs: sk.runs + 1 } : sk,
                ),
              }));
            }
          }
        }

        let usedLive = false;
        let finalAnswer = "";
        let aborted = false;
        try {
        // Host helpers always available for final sanitization
        const {
          extractHostCommands,
          stripHostCommands,
          inferHostCommandsFromUser,
          looksLikeDeferredHostWork,
          userWantsHostInvestigation,
        } = await import("./grok");
        const {
          looksLikeIncompleteAgentTurn,
          looksLikePlanningStall,
          buildAutoFinishNudge,
          buildKeepGoingUserPrompt,
        } = await import("./agent-finish");
        const {
          extractConnectorCommands,
          stripConnectorCommands,
          runConnectorTool,
        } = await import("./connector-tools");
        const {
          extractSelfModCommands,
          stripSelfModCommands,
          selfModList,
          selfModRead,
          selfModWrite,
          selfModPatch,
          selfModSnapshot,
        } = await import("./self-mod-client");
        const scrubAssistant = (s: string) =>
          stripSelfModCommands(stripConnectorCommands(stripHostCommands(s)));

        try {
          
            const { grokChatStream } = await import("./grok-client");

            // Multi-turn host tool loop (model can emit HOST_CMD: lines)
            const { expandMessageMedia } = await import("./chat-media");
            // Context manager: auto-compact if over budget, then budgeted history
            {
              const th0 = get().threads.find((x) => x.id === get().activeThreadId);
              const probe = buildContext({
                messages: get().chat.filter((c) => c.id !== botId),
                thread: th0 || null,
                memoryNotes: get().agentPrefs.memoryNotes,
                openClawBundle: get().openClawWorkspace?.contextBundle,
                trimTools: true,
              });
              if (probe.shouldCompact) {
                const r = get().compactThread(get().activeThreadId);
                if (r.ok) {
                  set({ streamStatus: "Context compacted…" });
                  await wait(120);
                }
              }
            }

            const thCtx = get().threads.find((x) => x.id === get().activeThreadId);
            const rawForCtx = get()
              .chat.filter((c) => c.role === "user" || c.role === "assistant")
              .filter((c) => c.id !== botId);

            // Expand media + reply tags on a working copy
            const expandedMsgs: ChatMessage[] = [];
            for (const c of rawForCtx) {
              let content =
                c.role === "assistant" ? stripAssistantChrome(c.content) : c.content;
              if (c.role === "user") {
                content = await expandMessageMedia(content);
              }
              if (c.role === "user" && c.replyToPreview) {
                const who =
                  c.replyToRole === "assistant"
                    ? "assistant"
                    : c.replyToRole === "user"
                      ? "user"
                      : "message";
                content =
                  "[Replying to " +
                  who +
                  ']: "' +
                  c.replyToPreview +
                  '"\n\n' +
                  content;
              }
              if (content.trim().length > 0) {
                expandedMsgs.push({ ...c, content });
              }
            }
            const expandedTrimmed = await expandMessageMedia(trimmed);
            if (
              !expandedMsgs.length ||
              expandedMsgs[expandedMsgs.length - 1]?.content !== expandedTrimmed
            ) {
              expandedMsgs.push({
                id: uid("msg"),
                role: "user",
                content: expandedTrimmed,
                ts: Date.now(),
              });
            }

            // M1 file memory pin (USER.md / MEMORY.md / daily)
            const notes = get().agentPrefs.memoryNotes || "";
            if (notes.trim()) {
              void migrateNotesToFileMemory(notes);
            }
            const fileMem = await loadMemoryPinBundle();
            const ctxBuilt = buildContext({
              messages: expandedMsgs,
              thread: thCtx || null,
              memoryNotes: notes,
              fileMemoryBundle: fileMem.bundle || "",
              learningBundle: learningPinBundle(get().learning),
              projectBundle: projectContextBlock(get().projectWorkspace),
              workboardBundle: workboardContextBlock(get().workboard),
              openClawBundle: get().openClawWorkspace?.contextBundle,
              connectorBlock: (await import("./grok")).connectorContextBlock(
                get().connectors,
              ),
              capabilityBlock: undefined, // filled below after we know tool flags
              trimTools: true,
            });
            const history: Array<{ role: "user" | "assistant"; content: string }> =
              ctxBuilt.messages;

const modelId = modelIdForMode(mode, trimmed, catalog, routeCtx, overrides);
            // Hold Adaptive decision so it feels intentional (not a flash)
            if (mode === "auto") {
              set({
                streamStatus: `Adaptive → ${auto.tierLabel} · ${auto.reasonDetail}`,
              });
              // Brief pause so the badge is readable; shorter on clear switches
              const holdMs =
                auto.tier === "fast" || (routeCtx.lastRouteTier && routeCtx.lastRouteTier !== auto.tier)
                  ? 160
                  : 240;
              await wait(holdMs);
              if (abort.signal.aborted || gen !== chatGeneration) {
                aborted = true;
              }
            }
            let rounds = 0;
            // Max/Heavy/Build get deeper tool loops for real debug/agent work
            const maxRounds =
              mode === "max" || routed === "max"
                ? 14
                : mode === "heavy" || routed === "heavy"
                  ? 12
                  : mode === "build" || routed === "build"
                    ? 12
                    : 8;
            let hostNudges = 0;
            const maxHostNudges = 5;
            let finishNudges = 0;
            const maxFinishNudges =
              mode === "max" || routed === "max" || mode === "heavy" || routed === "heavy"
                ? 5
                : mode === "build" || routed === "build" || mode === "auto"
                  ? 4
                  : 3;
            let usedAnyTools = false;
            // Sticky protocol reminder (reduces plan-only replies)
            if (get().agentPrefs.hostToolsEnabled) {
              history.push({
                role: "user",
                content:
                  "SYSTEM REMINDER: For any local install/process/file/log question, emit HOST_CMD on its own line in the same reply. Announcing “running checks” without HOST_CMD is invalid.",
              });
            }

            let accumulated = "";

            // Build workspace context once per user turn (budgeted pins + capabilities)
            const stTurn = get();
            const freeTier = false;
            const liveConn = stTurn.connectors.filter(
              (c) => c.status === "connected" && c.liveTools,
            );
            const statusConn = stTurn.connectors.filter(
              (c) => c.status === "connected" && !c.liveTools,
            );
            const capabilityBlock = [
              "## GrokHub session capabilities",
              "- Context manager: budgeted history + optional thread summary (see /context).",
              "- File memory on disk: ~/.config/GrokHub/memory/ (USER.md, MEMORY.md, LEARNINGS.md, STATUS.md) — NOT ~/.config/grokhub.",
              "- Learning is live: turns + 👍/👎 write STATUS.md; Reflect fills LEARNINGS.md.",
              "- Workboard: pin tasks with WORK_PIN: title | detail | priority=high; update WORK_UPDATE: id | status=…",
              "- Bound project: prefer HOST_CMD under the project path when set.",
              "- Tool registry (live):",
              formatToolRegistryForPrompt({ includeBlocked: false }),
              get().autonomy.level >= 3
                ? "- Autonomy (proactive): notice small problems and fix them without waiting; emit GOAL_COMPLETE / GOAL_BLOCKED when appropriate."
                : get().autonomy.level >= 2
                  ? "- Autonomy (helpful): if your last reply was incomplete, continue/fix without waiting for “please continue”."
                  : "- Autonomy: complete the user turn; wait for new asks before starting new work.",
              "- Persistent: chat history, settings, memory notes, Imagine media, connectors.",
              stTurn.agentPrefs?.memoryNotes?.trim()
                ? `- Memory notes loaded (${stTurn.agentPrefs.memoryNotes.trim().length} chars). Use them.`
                : "- No custom memory notes yet (user can set via Settings or /memory).",
              `- Threads in app: ${get().threads.length}; API window msgs: ${history.length}.`,
              thCtx?.summary
                ? "- This thread has a compacted summary of older turns."
                : "- No compaction summary yet (auto when over budget, or /compact).",
              stTurn.agentPrefs?.hostToolsEnabled === false
                ? "- Host shell tools: DISABLED."
                : "- Host shell tools: available when Desktop Host is LIVE (use HOST_CMD).",
              stTurn.agentPrefs?.connectorToolsEnabled === false
                ? "- Connector tools: DISABLED."
                : "- Connector tools: ONLY connectors marked liveTools (Grok, Desktop Host, GitHub with PAT).",
              liveConn.length
                ? `- LIVE connectors: ${liveConn.map((c) => c.name).join(", ")}.`
                : "- LIVE connectors: none connected.",
              statusConn.length
                ? `- Website status-only (DO NOT invent data or emit CONNECTOR_CMD for these): ${statusConn.map((c) => c.name).join(", ")}.`
                : "- No website-only connectors connected.",
              "- Never invent Gmail/Drive/Calendar/Notion/Linear contents. If a connector is not LIVE, say so and stop.",
            ].join("\n");
            const proactiveBlock = proactiveSystemAddon(get().autonomy);
            const turnWorkspaceContext =
              [
                ctxBuilt.workspaceContext,
                capabilityBlock,
                proactiveBlock,
                !stTurn.agentPrefs?.hostToolsEnabled
                  ? "NOTE: Host shell tools are DISABLED by user settings. Do not emit HOST_CMD."
                  : "",
                !stTurn.agentPrefs?.connectorToolsEnabled
                  ? "NOTE: Connector tools are DISABLED by user settings. Do not emit CONNECTOR_CMD."
                  : "",
                `## Context budget\n~${ctxBuilt.tokensEst} / ${ctxBuilt.budget} tokens (${ctxBuilt.percent}%).`,
              ]
                .filter(Boolean)
                .join("\n\n")
                .slice(0, 28_000) || undefined;

            set({
              streamStatus:
                ctxBuilt.percent >= 70
                  ? `Context ${ctxBuilt.percent}% · streaming…`
                  : get().streamStatus,
            });

while (rounds < maxRounds && !aborted) {
              rounds += 1;
              if (abort.signal.aborted || gen !== chatGeneration) {
                aborted = true;
                break;
              }
              set({
                streamStatus:
                  rounds === 1
                    ? mode === "auto"
                      ? `Streaming · ${auto.tierLabel}`
                      : "Streaming…"
                    : `Tool loop · round ${rounds} · calling model…`,
              });
              if (rounds > 1) {
                patchBot(
                  toolLoopWaitMarkdown(
                    scrubAssistant(accumulated) || "Working…",
                    rounds,
                  ),
                  { streaming: true },
                );
              }
              (globalThis as unknown as { __ghFirstTok?: boolean }).__ghFirstTok = false;
              let roundText = "";
              const stNow = get();
              const result = await grokChatStream(
                {
                  messages: history,
                  mode: routed,
                  model: freeTier ? undefined : modelId,
                  apiKey: get().apiKey || undefined,
                  accessToken: get().oauth?.accessToken,
                  tokens: get().oauth,
                  ssoCookie: get().ssoCookie || undefined,
                  freeTier,
                  allowWebsiteFallback: false,
                  temperature: stNow.agentPrefs?.temperature ?? 0.7,
                  workspaceContext: turnWorkspaceContext,
                },
                {
                  signal: abort.signal,
                  onStatus: (st) => {
                    if (gen !== chatGeneration) return;
                    const label =
                      st === "streaming"
                        ? rounds === 1
                          ? "Streaming…"
                          : `Streaming · round ${rounds}…`
                        : st === "fallback"
                          ? "Responding…"
                          : st === "connecting"
                            ? "Connecting to Grok…"
                            : st || "Working…";
                    set({ streamStatus: label });
                  },
                  onDelta: (piece) => {
                    if (gen !== chatGeneration) return;
                    roundText += piece;
                    accumulated = roundText;
                    const scrub = (s: string) => scrubAssistant(s) || "…";
                    // First token: paint immediately so stream never looks dead
                    const g = globalThis as unknown as {
                      __ghRaf?: number;
                      __ghFirstTok?: boolean;
                    };
                    if (!g.__ghFirstTok) {
                      g.__ghFirstTok = true;
                      patchBot(scrub(roundText), { streaming: true });
                      set({
                        streamStatus:
                          rounds === 1 ? "Streaming…" : `Streaming · round ${rounds}…`,
                      });
                      return;
                    }
                    // Batch subsequent patches to animation frames
                    if (!g.__ghRaf) {
                      g.__ghRaf = requestAnimationFrame(() => {
                        g.__ghRaf = 0;
                        if (gen !== chatGeneration) return;
                        patchBot(scrub(roundText), { streaming: true });
                      });
                    }
                  },
                },
              );

              if (result.tokens) {
                set({ oauth: result.tokens });
                void import("./secrets-client").then((m) =>
                  m.secretsSet("oauth", JSON.stringify(result.tokens)),
                );
              }
              if (result.aborted || abort.signal.aborted || gen !== chatGeneration) {
                aborted = true;
                break;
              }

              if (result.ok && (result.content || roundText)) {
                usedLive = true;
                if (
                  (result as { freeTier?: boolean }).freeTier ||
                  String((result as { accessPath?: string }).accessPath || "").includes("free")
                ) {
                  set((s) => ({
                    grokConnected: true,
                    grokStatusDetail:
                      (result as { accessPath?: string }).accessPath === "website_free"
                        ? "Website session"
                        : (result as { fallbackFrom?: string }).fallbackFrom
                          ? `Session fallback · ${(result as { model?: string }).model || "model"}`
                          : "Session fallback",
                    usage:
                      s.oauth || s.apiKey
                        ? s.usage
                        : { ...s.usage, plan: "free" as const },
                  }));
                }
                if (result.usage) {
                  bill = get().recordTokenUsage(
                    result.usage,
                    routed,
                    result.rateLimit,
                  );
                } else if (rounds === 1) {
                  // No token payload — fall back to mode estimate once
                  bill = get().recordUsage("message", routed);
                }
                const fullRaw = stripAssistantChrome(result.content || roundText);
                // Workboard pins/updates from model
                try {
                  const wc = extractWorkCommands(fullRaw);
                  if (wc.pins.length || wc.updates.length) {
                    let wb = get().workboard;
                    for (const pin of wc.pins) {
                      const r = pinWorkItemHelper(wb, {
                        title: pin.title,
                        detail: pin.detail,
                        priority: pin.priority,
                        source: "agent",
                        threadId: get().activeThreadId,
                        projectPath: get().projectWorkspace?.path || null,
                      });
                      wb = r.state;
                    }
                    for (const up of wc.updates) {
                      wb = setWorkStatusHelper(wb, up.ref, up.status);
                    }
                    set({ workboard: wb });
                    if (wc.pins.length) {
                      get().pushActivity({
                        kind: "agent",
                        title: `Workboard +${wc.pins.length}`,
                        detail: wc.pins.map((p) => p.title).join(", ").slice(0, 120),
                        status: "success",
                      });
                    }
                  }
                } catch {
                  /* ignore */
                }
                // Collect MEMORY_NOTE for post-stream write (never block stream)
                try {
                  const mn = extractMemoryNotes(fullRaw);
                  if (mn.notes.length) {
                    pendingMemoryNotes.push(...mn.notes);
                  }
                } catch {
                  /* ignore */
                }
                const full = fullRaw;
                const visible = scrubAssistant(full);
                accumulated = full;
                // Never show raw HOST_CMD lines to the user
                patchBot(
                  visible || "Working on your machine…",
                  { streaming: true },
                );
                set({
                  grokConnected: true,
                  grokStatusDetail: `Live · ${result.model || modelId}`,
                });

                let cmds = extractHostCommands(full);
                let connCmds = extractConnectorCommands(full);
                // Respect tool toggles
                if (!get().agentPrefs.hostToolsEnabled) cmds = [];
                if (!get().agentPrefs.connectorToolsEnabled) connCmds = [];
                // First round: if user asked about local files and model forgot HOST_CMD, infer
                if (!cmds.length && rounds === 1 && get().agentPrefs.hostToolsEnabled) {
                  cmds = inferHostCommandsFromUser(trimmed);
                }
                // Model planned host work but never emitted HOST_CMD — force a tool turn
                const needsHost =
                  get().agentPrefs.hostToolsEnabled &&
                  (looksLikeDeferredHostWork(full) ||
                    looksLikePlanningStall(full) ||
                    (!usedAnyTools &&
                      userWantsHostInvestigation(trimmed) &&
                      rounds <= 4));
                if (
                  !cmds.length &&
                  !connCmds.length &&
                  needsHost &&
                  hostNudges < maxHostNudges
                ) {
                  hostNudges += 1;
                  const inferred = inferHostCommandsFromUser(trimmed);
                  if (inferred.length) {
                    cmds = inferred;
                    set({
                      streamStatus: `Starting host investigation… (${hostNudges}/${maxHostNudges})`,
                    });
                    get().pushActivity({
                      kind: "desktop",
                      title: "Auto host nudge",
                      detail: "Model deferred tools — running safe diagnostics",
                      status: "running",
                    });
                  } else {
                    history.push({ role: "assistant", content: full });
                    history.push({
                      role: "user",
                      content: [
                        "SYSTEM: INVALID TURN — you announced work or planned diagnostics without HOST_CMD.",
                        "Do not apologize. Do not ask permission. Do not only describe what you will do.",
                        "Immediately emit one or more own-line HOST_CMD commands for safe read-only diagnostics.",
                        "Examples:",
                        'HOST_CMD: uname -a',
                        'HOST_CMD: ps -eo pid,pcpu,cmd --sort=-pcpu | head -20',
                        'HOST_CMD: ls -la "$HOME/.local/lib/grokhub" | head -40',
                        "No preamble-only replies. Commands first.",
                      ].join("\n"),
                    });
                    set({
                      streamStatus: `Nudging agent to use host tools… (${hostNudges}/${maxHostNudges})`,
                    });
                    patchBot(
                      (visible || full) + "\n\n_Switching to host tools…_",
                      { streaming: true },
                    );
                    continue;
                  }
                }
                if (!cmds.length && !connCmds.length) {
                  const candidate = visible || full;
                  const incomplete = looksLikeIncompleteAgentTurn(candidate, {
                    hadTools: usedAnyTools,
                    userPrompt: trimmed,
                  });
                  if (
                    incomplete &&
                    finishNudges < maxFinishNudges &&
                    !abort.signal.aborted &&
                    gen === chatGeneration
                  ) {
                    finishNudges += 1;
                    history.push({ role: "assistant", content: full });
                    history.push({
                      role: "user",
                      content: buildAutoFinishNudge({
                        round: finishNudges,
                        maxRounds: maxFinishNudges,
                        userPrompt: trimmed,
                        lastAssistant: candidate,
                        hostAvailable: Boolean(get().agentPrefs.hostToolsEnabled),
                      }),
                    });
                    set({
                      streamStatus: `Auto-finish ${finishNudges}/${maxFinishNudges} — completing goal…`,
                    });
                    patchBot(
                      (candidate || "Working…") +
                        `\n\n_Continuing automatically (${finishNudges}/${maxFinishNudges})…_`,
                      { streaming: true },
                    );
                    get().pushActivity({
                      kind: "system",
                      title: "Auto-finish nudge",
                      detail: `Round ${finishNudges}/${maxFinishNudges} — agent stalled on plan-only reply`,
                      status: "running",
                    });
                    continue;
                  }
                  finalAnswer = candidate;
                  break;
                }
                usedAnyTools = true;

                // Connector tools (GitHub live; website connectors status-aware)
                if (connCmds.length) {
                  set({ streamStatus: "Running connector tools…" });
                  const outputs: string[] = [];
                  for (const cc of connCmds.slice(0, 3)) {
                    if (abort.signal.aborted || gen !== chatGeneration) {
                      aborted = true;
                      break;
                    }
                    const label = `${cc.connectorId} ${cc.tool}${cc.args ? " " + cc.args : ""}`;
                    set({ streamStatus: `Connector: ${label}…` });
                    get().pushActivity({
                      kind: "connector",
                      title: `Running ${cc.connectorId}:${cc.tool}`,
                      detail: label.slice(0, 160),
                      status: "running",
                    });
                    const row = get().connectors.find((c) => c.id === cc.connectorId);
                    let connElapsed = 0;
                    const paintConn = () => {
                      set({ streamStatus: `Connector: ${label}… (${connElapsed}s)` });
                      patchBot(
                        toolRunningMarkdown({
                          kind: "connector",
                          command: label,
                          preface: visible || "Using connected services…",
                          elapsedSec: connElapsed,
                        }),
                        { streaming: true },
                      );
                    };
                    paintConn();
                    const connTick = setInterval(() => {
                      if (gen !== chatGeneration || abort.signal.aborted) return;
                      connElapsed += 1;
                      paintConn();
                    }, 1000);
                    try {
                      const r = await runConnectorTool({
                        connectorId: cc.connectorId,
                        tool: cc.tool,
                        args: cc.args,
                        githubToken: get().githubToken,
                        websiteConnected: row?.status === "connected",
                        accountLabel: row?.accountLabel,
                      });
                      clearInterval(connTick);
                      outputs.push(
                        [
                          `CONNECTOR ${cc.connectorId} ${cc.tool}`,
                          r.ok ? "ok" : "failed",
                          r.detail,
                        ].join("\n"),
                      );
                      get().pushActivity({
                        kind: "connector",
                        title: `${cc.connectorId}:${cc.tool}`,
                        detail: r.detail.slice(0, 160),
                        status: r.ok ? "success" : "failed",
                      });
                      if (r.ok) {
                        set((s) => ({
                          connectors: s.connectors.map((c) =>
                            c.id === cc.connectorId
                              ? { ...c, lastUsed: Date.now() }
                              : c,
                          ),
                        }));
                      }
                    } catch (e) {
                      clearInterval(connTick);
                      outputs.push(
                        `CONNECTOR ${cc.connectorId} ${cc.tool}\n[error] ${
                          e instanceof Error ? e.message : "failed"
                        }`,
                      );
                    }
                  }
                  if (aborted) break;
                  const toolBlock = [
                    "CONNECTOR_RESULT (authoritative — use this, do not invent data):",
                    outputs.join("\n\n---\n\n"),
                    "",
                    "Summarize for the user. Only emit another CONNECTOR_CMD if needed.",
                  ].join("\n");
                  history.push({ role: "assistant", content: full });
                  history.push({ role: "user", content: toolBlock });
                  set({ streamStatus: "Summarizing connector results…" });
                  patchBot(
                    toolResultMarkdown({
                      kind: "connector",
                      preface: visible || "Checked connectors.",
                      outputs,
                      summarizing: true,
                    }),
                    { streaming: true },
                  );
                  // If also host cmds, continue to host below after connector round
                  if (!cmds.length) continue;
                }

                
                // Self-modification (install tree) when enabled
                let selfCmds = extractSelfModCommands(full);
                if (selfCmds.length) {
                  if (!get().desktop.selfModifyEnabled) {
                    history.push({ role: "assistant", content: full });
                    history.push({
                      role: "user",
                      content:
                        "SELF_MOD_RESULT: blocked — self-modification is disabled. User must enable Settings → Desktop → Allow self-modification, or use Factory reinstall if the app is broken.",
                    });
                    patchBot(
                      (visible || "") +
                        "\n\n_Self-mod blocked (enable in Settings)._\n_Continuing…_",
                      { streaming: true },
                    );
                    if (!cmds.length) continue;
                  } else {
                    set({ streamStatus: "Self-modifying app…" });
                    const outputs: string[] = [];
                    const needsMutate = selfCmds.some(
                      (sc) => sc.kind === "write" || sc.kind === "patch",
                    );
                    if (needsMutate) {
                      try {
                        const snap = await selfModSnapshot(
                          `pre-agent-${Date.now()}`,
                        );
                        outputs.push(
                          snap.ok
                            ? `SNAPSHOT auto ${(snap as { id?: string }).id || "ok"} (before writes)`
                            : `SNAPSHOT auto failed — writes may still proceed`,
                        );
                      } catch (e) {
                        outputs.push(
                          `SNAPSHOT auto error: ${e instanceof Error ? e.message : "failed"}`,
                        );
                      }
                    }
                    for (const sc of selfCmds.slice(0, 4)) {
                      if (abort.signal.aborted || gen !== chatGeneration) {
                        aborted = true;
                        break;
                      }
                      try {
                        if (sc.kind === "list") {
                          const r = await selfModList(sc.path);
                          outputs.push(
                            `LIST ${sc.path}\n${JSON.stringify(r.entries || r, null, 2).slice(0, 4000)}`,
                          );
                        } else if (sc.kind === "read") {
                          const r = await selfModRead(sc.path);
                          outputs.push(
                            r.ok
                              ? `READ ${sc.path}\n${(r.content || "").slice(0, 8000)}`
                              : `READ ${sc.path} failed: ${(r as { error?: string }).error}`,
                          );
                        } else if (sc.kind === "write") {
                          const r = await selfModWrite(sc.path, sc.content, {
                            note: "agent SELF_MOD",
                          });
                          outputs.push(
                            r.ok
                              ? `WRITE ${sc.path} ok`
                              : `WRITE ${sc.path} failed: ${(r as { error?: string }).error}`,
                          );
                          get().pushActivity({
                            kind: "skill",
                            title: `Self-mod write ${sc.path}`,
                            detail: r.ok ? "ok" : String((r as { error?: string }).error || "fail"),
                            status: r.ok ? "success" : "failed",
                          });
                        } else if (sc.kind === "patch") {
                          const r = await selfModPatch(sc.path, sc.find, sc.replace, {
                            note: "agent SELF_MOD patch",
                          });
                          outputs.push(
                            r.ok
                              ? `PATCH ${sc.path} ok`
                              : `PATCH ${sc.path} failed: ${(r as { error?: string }).error}`,
                          );
                          get().pushActivity({
                            kind: "skill",
                            title: `Self-mod patch ${sc.path}`,
                            detail: r.ok ? "ok" : String((r as { error?: string }).error || "fail"),
                            status: r.ok ? "success" : "failed",
                          });
                        } else if (sc.kind === "snapshot") {
                          const r = await selfModSnapshot(sc.note);
                          outputs.push(
                            r.ok
                              ? `SNAPSHOT ${(r as { id?: string }).id} files=${(r as { fileCount?: number }).fileCount}`
                              : `SNAPSHOT failed`,
                          );
                        }
                      } catch (e) {
                        outputs.push(
                          `SELF_MOD error: ${e instanceof Error ? e.message : "failed"}`,
                        );
                      }
                    }
                    if (aborted) break;
                    history.push({ role: "assistant", content: full });
                    history.push({
                      role: "user",
                      content: [
                        "SELF_MOD_RESULT (authoritative):",
                        outputs.join("\n\n---\n\n"),
                        "",
                        "Summarize for the user. Remind them Factory reinstall is available in Settings if anything breaks.",
                      ].join("\n"),
                    });
                    patchBot(
                      [
                        visible || "Applied self-mod steps.",
                        "",
                        "```",
                        outputs.join("\n\n").slice(0, 3000),
                        "```",
                        "",
                        "_Summarizing…_",
                      ].join("\n"),
                      { streaming: true },
                    );
                    if (!cmds.length) continue;
                  }
                }

if (!cmds.length) {
                  finalAnswer = visible || full;
                  break;
                }

                // Execute host commands and feed results back
                const { classifyHostCommand, needsHostConfirm, riskLabel } = await import("./host-safety");
                const riskList = cmds.slice(0, 5).map((c) => riskLabel(classifyHostCommand(c)));
                const desk = get().desktop;
                const hostSlice = cmds.slice(0, 5);
                const allowlist = get().hostAllowlist || [];
                const { isHostAllowlisted } = await import("./host-safety");
                const needsAnyConfirm =
                  needsHostConfirm(hostSlice, {
                    confirmAll: Boolean(desk.confirmHostCommands) && !desk.confirmDestructiveOnly,
                    confirmDestructive: Boolean(desk.confirmHostCommands),
                  }) &&
                  hostSlice.some((c) => !isHostAllowlisted(c, allowlist));
                if (needsAnyConfirm) {
                  const allowed = await requestHostConfirm(
                    set,
                    hostSlice,
                    riskList,
                    botId,
                  );
                  if (!allowed) {
                    finalAnswer =
                      (visible || "") +
                      "\n\n_Host commands cancelled — not run on your machine._";
                    patchBot(finalAnswer, { streaming: false });
                    break;
                  }
                }
                set({ streamStatus: "Running on your desktop…" });
                const { hostExec } = await import("./host-client");
                const { boundHostScanCommand, hostTimeoutMs, clipHostOutput } = await import("./host-scan");
                const outputs: string[] = [];
                // Allow a few more cmds for multi-step scans; still cap to keep turns sane
                const hostCmdList = cmds.slice(0, 5);
                const isReadOnlyCmd = (c: string) =>
                  /^(ls|ll|pwd|whoami|uname|cat |head |tail |wc |file |stat |find |rg |grep |ps |df |du |free |id |env |printenv |hostname |date |which |type |realpath |readlink |test |\[|echo |journalctl |systemctl --user status|ip |ss |uptime)/i.test(
                    c.trim(),
                  ) &&
                  !/\b(rm|mv|cp|chmod|chown|mkfs|dd|sudo|tee|install|pacman|npm i|pip install|>|>>)\b/i.test(
                    c,
                  );
                const canParallel =
                  hostCmdList.length > 1 && hostCmdList.every((c) => isReadOnlyCmd(c));

                const runOneHost = async (rawCmd: string, stepIdx: number, stepTotal: number) => {
                  if (abort.signal.aborted || gen !== chatGeneration) {
                    return { aborted: true as const, out: "" };
                  }
                  const bounded = boundHostScanCommand(rawCmd);
                  const cmd = bounded.command;
                  const timeoutMs = hostTimeoutMs(cmd, 90_000);
                  get().pushActivity({
                    kind: "desktop",
                    title: "Host command",
                    detail: (bounded.note ? `[${bounded.note}] ` : "") + rawCmd.slice(0, 140),
                    status: "running",
                  });
                  let elapsedSec = 0;
                  const paintHost = () => {
                    set({
                      streamStatus: canParallel
                        ? `Host parallel ${stepIdx}/${stepTotal}: ${rawCmd.slice(0, 40)}…`
                        : `Host: ${rawCmd.slice(0, 56)}… (${elapsedSec}s)`,
                    });
                    if (!canParallel) {
                      patchBot(
                        toolRunningMarkdown({
                          kind: "host",
                          command: rawCmd,
                          preface: visible || "Working on your machine…",
                          elapsedSec,
                          step: { index: stepIdx, total: stepTotal },
                        }),
                        { streaming: true },
                      );
                    }
                  };
                  paintHost();
                  const hostTick = setInterval(() => {
                    if (gen !== chatGeneration || abort.signal.aborted) return;
                    elapsedSec += 1;
                    paintHost();
                  }, 1000);
                  try {
                    const jobId = `host-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
                    activeHostJobId = jobId; activeHostJobIds.add(jobId);
                    let r;
                    try {
                      r = await hostExec(cmd, undefined, timeoutMs, { jobId });
                    } finally {
                      clearInterval(hostTick);
                      activeHostJobIds.delete(jobId); if (activeHostJobId === jobId) activeHostJobId = null;
                    }
                    const out = clipHostOutput(
                      [
                        `$ ${rawCmd}`,
                        bounded.bounded && bounded.note
                          ? `# runtime bounds: ${bounded.note}`
                          : "",
                        `exit ${r.code ?? "?"} · ${r.ms}ms · ${r.cwd}`,
                        r.stdout || "(no stdout)",
                        r.stderr ? `[stderr]\n${r.stderr}` : "",
                        r.ok
                          ? ""
                          : r.stderr && /timed out/i.test(r.stderr)
                            ? "(scan timed out — partial output above; narrow the path or maxdepth)"
                            : "",
                      ]
                        .filter(Boolean)
                        .join("\n"),
                    );
                    get().pushActivity({
                      kind: "desktop",
                      title: r.ok
                        ? "Host ok"
                        : /timed out/i.test(r.stderr || "")
                          ? "Host timeout"
                          : "Host failed",
                      detail: rawCmd.slice(0, 120),
                      status: r.ok ? "success" : "failed",
                    });
                    return { aborted: false as const, out };
                  } catch (e) {
                    clearInterval(hostTick);
                    get().pushActivity({
                      kind: "desktop",
                      title: "Host error",
                      detail: e instanceof Error ? e.message : "failed",
                      status: "failed",
                    });
                    return {
                      aborted: false as const,
                      out: clipHostOutput(
                        `$ ${rawCmd}\n[host error] ${e instanceof Error ? e.message : "failed"}\n(continuing agent turn)`,
                      ),
                    };
                  }
                };

                if (canParallel) {
                  set({
                    streamStatus: `Host: running ${hostCmdList.length} read-only cmds in parallel…`,
                  });
                  patchBot(
                    (visible || "Working on your machine…") +
                      `\n\n_Running ${hostCmdList.length} safe host commands in parallel…_`,
                    { streaming: true },
                  );
                  const results = await Promise.all(
                    hostCmdList.map((c, i) => runOneHost(c, i + 1, hostCmdList.length)),
                  );
                  if (results.some((r) => r.aborted)) aborted = true;
                  for (const r of results) {
                    if (r.out) outputs.push(r.out);
                  }
                } else {
                  for (let hi = 0; hi < hostCmdList.length; hi++) {
                    const r = await runOneHost(
                      hostCmdList[hi]!,
                      hi + 1,
                      hostCmdList.length,
                    );
                    if (r.aborted) {
                      aborted = true;
                      break;
                    }
                    if (r.out) outputs.push(r.out);
                  }
                }
                if (aborted) break;

                const toolBlock = [
                  "HOST_RESULT (authoritative — use this, do not invent files):",
                  outputs.join("\n\n---\n\n"),
                  "",
                  "Summarize these results for the user in plain language.",
                  "If a scan timed out or was truncated, say so and suggest a narrower path.",
                  "Do not output HOST_CMD again unless you still need a different command.",
                ].join("\n");

                history.push({ role: "assistant", content: full });
                history.push({ role: "user", content: toolBlock });
                set({ streamStatus: "Summarizing host results…" });
                // Show intermediate host output (sanitized) while model continues
                const mid = toolResultMarkdown({
                  kind: "host",
                  preface: visible || "Checked your machine.",
                  outputs,
                  summarizing: true,
                });
                patchBot(mid, { streaming: true });
                accumulated = mid;
                // continue loop for next model turn
                continue;
              }

              // Failed live call
              const hasOauth = Boolean(get().oauth?.accessToken);
              const err = result.error || "Unknown error";
              finalAnswer = [
                friendlyAssistantError(err),
                "",
                hasOauth
                  ? "Your OAuth session is saved. Try reconnecting OAuth, paste an xAI API key, or Link Grok website for free-tier chat."
                  : "Fix: Link Grok website (free), Connect with Grok OAuth, or paste an xAI API key (console.x.ai free credits).",
              ].join("\n");
              set({
                grokConnected: false,
                grokStatusDetail: hasOauth
                  ? `OAuth session · chat failed: ${err}`
                  : err,
              });
              patchBot(finalAnswer, { streaming: false });
              break;
            }

            if (!finalAnswer && accumulated && !aborted) {
              finalAnswer = stripHostCommands(
                stripAssistantChrome(
                  accumulated.replace(/\n_Working…_\s*$/, "").replace(/\n_Summarizing…_\s*$/, ""),
                ),
              );
            }
          
        } catch (e) {
          if (abort.signal.aborted || gen !== chatGeneration) {
            aborted = true;
          } else {
            const msg = e instanceof Error ? e.message : "request failed";
            finalAnswer = friendlyAssistantError(msg);
            set({ grokConnected: false, grokStatusDetail: msg });
            patchBot(finalAnswer, { streaming: false });
          }
        }

        if (gen !== chatGeneration) {
          // Superseded — clear THIS bubble only; do not unpause persist if a newer turn owns it
          try {
            if (get().streamingMessageId === botId || get().chat.some((m) => m.id === botId && m.streaming)) {
              finalizeChatStream(set, botId);
            }
          } catch {
            /* ignore */
          }
          if (activeChatAbort === abort) {
            endChatTurnPersist();
          }
          return;
        }

        if (aborted) {
          // stopChat may already have set resume; if abort came from elsewhere, mark it
          const st = get();
          if (st.running) {
            const tid = st.activeThreadId;
            const lastUser = [...st.chat].reverse().find((m) => m.role === "user");
            const partial = (
              st.chat.find((m) => m.id === botId)?.content ||
              finalAnswer ||
              ""
            )
              .replace(/\n*_Stopped\._\s*$/m, "")
              .trim();
            set((s) => {
              const chat = s.chat.map((row) =>
                row.id === botId
                  ? {
                      ...row,
                      streaming: false,
                      stopped: true,
                      content: row.content?.trim()
                        ? `${row.content}${row.content.endsWith("\n") ? "" : "\n"}\n_Stopped._`
                        : "_Stopped._",
                    }
                  : row,
              );
              const threads = s.threads.map((th) =>
                th.id === tid ? threadWithMessages(th, chat) : th,
              );
              const th = threads.find((x) => x.id === tid);
              return {
                chat,
                threads,
                running: false,
                streamStatus: null,
                streamingMessageId: null,
                sessionResume:
                  tid && lastUser?.content
                    ? {
                        kind: "interrupted" as const,
                        threadId: tid,
                        title: th?.title || "Interrupted chat",
                        preview: (partial || lastUser.content).slice(0, 160),
                        mode: s.mode,
                        ts: Date.now(),
                        pendingPrompt: lastUser.content,
                        partialContent: partial || undefined,
                        stoppedMessageId: botId,
                      }
                    : s.sessionResume,
              };
            });
          }
          try {
            get().recordQuickAssistOutcome("failure");
          } catch {
            /* ignore */
          }
          void enqueueLearning(async () => {
            try {
              await wait(0);
              const turn = await applyTurnLearning(get().learning, {
                ok: false,
                mode: routed,
                routeTier: auto?.tier,
                userText: trimmed,
                assistantText: finalAnswer || "",
                threadId: get().activeThreadId || undefined,
                online: Boolean(
                  get().oauth?.accessToken || get().apiKey || get().ssoCookie,
                ),
              });
              set({ learning: turn.learning });
            } catch {
              /* ignore */
            }
          });
        } else {
          const answer = stripHostCommands(stripAssistantChrome(finalAnswer || ""));
          set((s) => {
            const chat = s.chat.map((row) =>
              row.id === botId
                ? {
                    ...row,
                    content: answer || row.content || "(empty)",
                    streaming: false,
                    stopped: false,
                    ts: Date.now(),
                    mode: routed,
                  }
                : row,
            );
            const tid = s.activeThreadId;
            const threads = s.threads.map((th) =>
              th.id === tid
                ? threadWithMessages(th, chat, { mode: s.mode })
                : th,
            );
            return {
              chat,
              threads,
              running: false,
              streamStatus: null,
              streamingMessageId: null,
              // Successful finish — clear any interrupt banner
              sessionResume: null,
            };
          });
          get().pushActivity({
            kind: "chat",
            title: usedLive ? `Grok · ${m.label}` : `Agent reply · ${m.label}`,
            detail: `${trimmed.slice(0, 80)} · ${bill.cost}u`,
            status: usedLive ? "success" : "failed",
          });
          try {
            get().recordQuickAssistOutcome(usedLive ? "success" : "failure");
          } catch {
            /* ignore */
          }
          // Learning is post-stream only — never await on the UI/stream path
          {
            const botRow = get().chat.find((m) => m.id === botId);
            const th = get().threads.find((x) => x.id === get().activeThreadId);
            const snap = {
              ok: Boolean(usedLive) || Boolean(answer?.trim()),
              mode: routed,
              routeTier: botRow?.routeTier || auto?.tier,
              modelId: botRow?.routeModel || auto?.modelId,
              userText: trimmed,
              assistantText: answer || "",
              threadId: get().activeThreadId || undefined,
              threadTitle: th?.title,
              usedHostTools: /host|HOST_CMD|Desktop host/i.test(answer || ""),
              usedConnectors: /connector|CONNECTOR_CMD/i.test(answer || ""),
              online: Boolean(
                get().oauth?.accessToken || get().apiKey || get().ssoCookie,
              ),
            };
            void enqueueLearning(async () => {
              try {
                await wait(0);
                let L = get().learning;
                if (pendingMemoryNotes.length) {
                  try {
                    L = await applyAgentMemoryNotes(L, pendingMemoryNotes);
                  } catch {
                    /* ignore */
                  }
                }
                const turn = await applyTurnLearning(L, snap);
                // Serial queue: always commit (don't drop when next turn is running)
                set({ learning: turn.learning });
                if (turn.didReflect) {
                  get().pushActivity({
                    kind: "system",
                    title: "Learning reflect",
                    detail: `Wrote insights · ${turn.diskRoot || "memory"}`,
                    status: "success",
                  });
                }
                if (turn.learning.totalTurns > 0 && turn.learning.totalTurns % 12 === 0) {
                  void get().runSelfImprove().catch(() => {});
                }
              } catch {
                /* ignore */
              }
            });
          }
        }

        } catch (fatal) {
          // Import or unexpected failure after running:true — always clear stream chrome
          try {
            const msg =
              fatal instanceof Error ? fatal.message : "Agent turn failed";
            set((s) => {
              const chat = s.chat.map((row) =>
                row.id === botId
                  ? {
                      ...row,
                      streaming: false,
                      content:
                        row.content?.trim() ||
                        `Something went wrong: ${msg}`,
                    }
                  : row.streaming
                    ? { ...row, streaming: false }
                    : row,
              );
              return {
                chat,
                threads: s.threads.map((th) =>
                  th.id === s.activeThreadId
                    ? { ...th, messages: chat, updatedAt: Date.now() }
                    : th,
                ),
                running: false,
                streamStatus: null,
                streamingMessageId: null,
              };
            });
          } catch {
            set({
              running: false,
              streamStatus: null,
              streamingMessageId: null,
            });
          }
        } finally {
        // Safety net: never leave bubbles in streaming state after turn ends
        try {
          const owns =
            get().streamingMessageId === botId ||
            get().chat.some((m) => m.id === botId && m.streaming);
          // Never tear down a newer turn (bare get().running is wrong here)
          if (owns) {
            finalizeChatStream(set, botId);
          }
        } catch {
          try {
            set({
              running: false,
              streamStatus: null,
              streamingMessageId: null,
            });
          } catch {
            /* ignore */
          }
        }

        const ours = activeChatAbort === abort;
        if (ours) activeChatAbort = null;
        if (ours || gen === chatGeneration) {
          endChatTurnPersist();
        }
        if (ours || gen === chatGeneration) {
          get().setAgentStatus("primary", "idle", 0);
          get().setAgentStatus("builder", "idle", 0);
          get().setAgentStatus("research", "idle", 0);
          get().setAgentStatus("ops", "idle", 0);
          // Fast-mode LLM title (super short summary) — respects titleLocked
          if (!aborted) {
            const tid = get().activeThreadId;
            void autoRenameThreadWithFast(get, set, tid);
            // LLM chip refresh runs only when suggestions are open (ChatView)
          }
          void get().processAgentQueue();
        }
        } // end finally
      },

      setImaginePrompt: (v) => set({ imaginePrompt: v }),
      setImagineAspect: (v) => set({ imagineAspect: v }),
      setImagineMediaKind: (v) => set({ imagineMediaKind: v }),
      setImagineQuality: (v) => set({ imagineQuality: v }),
      setImagineReference: (v) => set({ imagineReference: v }),

      removeImagineJob: (id) => {
        const job = get().imagineJobs.find((j) => j.id === id);
        set((s) => ({
          imagineJobs: s.imagineJobs.filter((j) => j.id !== id),
          imagineReference:
            s.imagineReference &&
            job &&
            (s.imagineReference === job.imageDataUrl ||
              s.imagineReference === job.videoDataUrl)
              ? null
              : s.imagineReference,
        }));
        void import("./imagine-media").then(({ deleteImagineMedia }) =>
          deleteImagineMedia(id),
        );
        get().pushActivity({
          kind: "imagine",
          title: "Deleted Imagine item",
          detail: job?.prompt?.slice(0, 80) || id,
          status: "success",
        });
      },

      clearImagineJobs: () => {
        const n = get().imagineJobs.length;
        if (!n) return;
        set({ imagineJobs: [], imagineError: null, imagineReference: null });
        void import("./imagine-media").then(({ clearImagineMedia }) => clearImagineMedia());
        get().pushActivity({
          kind: "imagine",
          title: "Cleared Imagine gallery",
          detail: `${n} item${n === 1 ? "" : "s"} removed`,
          status: "success",
        });
      },

      runImagine: async (prompt) => {
        const p = (prompt ?? get().imaginePrompt).trim();
        if (!p) return;
        const bill = get().recordUsage("imagine");
        if (!bill.ok) {
          get().pushActivity({
            kind: "imagine",
            title: "Imagine blocked",
            detail: "Usage quota exceeded — wait for period reset or switch plan in Settings",
            status: "failed",
          });
          return;
        }
        const aspect = get().imagineAspect;
        const mediaKind = get().imagineMediaKind;
        const quality = get().imagineQuality;
        const referenceDataUrl = get().imagineReference || undefined;
        const mode = get().mode;
        const id = uid("img");
        const job: ImagineJob = {
          id,
          prompt: p,
          aspect,
          ts: Date.now(),
          status: "rendering",
          mode,
          mediaKind,
          quality,
          referenceDataUrl,
        };
        set((s) => ({
          imagineJobs: [job, ...s.imagineJobs].slice(0, 24),
          imagineBusy: true,
          imaginePrompt: p,
          imagineError: null,
        }));
        get().pushActivity({
          kind: "imagine",
          title: mediaKind === "video" ? "Imagine video rendering" : "Imagine rendering",
          detail: `${p.slice(0, 80)} · ${aspect} · ${quality} · ${bill.cost}u`,
          status: "running",
        });

        let imageDataUrl: string | undefined;
        let videoDataUrl: string | undefined;
        let source: "xai" | "local" = "local";
        let model: string | undefined;
        let err: string | null = null;
        let outKind = mediaKind;

        try {
          const { grokImagine } = await import("./grok-client");
          const live = await grokImagine({
            prompt: p,
            apiKey: get().apiKey || undefined,
            accessToken: get().oauth?.accessToken,
            tokens: get().oauth,
            aspect,
            quality,
            mediaKind,
            referenceDataUrl,
          });
          if (live.ok && (live.imageDataUrl || live.videoDataUrl)) {
            imageDataUrl = live.imageDataUrl;
            videoDataUrl = live.videoDataUrl;
            source = "xai";
            model = live.model;
            if (live.mediaKind === "video" || live.mediaKind === "image") {
              outKind = live.mediaKind;
            }
            if (live.error) err = live.error;
            if (live.tokens) {
              set({ oauth: live.tokens });
              void import("./secrets-client").then((m) =>
                m.secretsSet("oauth", JSON.stringify(live.tokens)),
              );
            }
          } else {
            err = live.error || "live Imagine unavailable";
          }
        } catch (e) {
          err = e instanceof Error ? e.message : "Imagine request failed";
        }

        // Local SVG preview if live image path failed (not for successful video)
        if (!imageDataUrl && !videoDataUrl) {
          const localAspect = aspect === "auto" ? "1:1" : aspect;
          imageDataUrl = renderImaginePreview(p, localAspect);
          source = "local";
          outKind = "image";
        }

        // Persist bytes to userData so media survives restarts/updates
        let imageRelPath: string | undefined;
        let videoRelPath: string | undefined;
        try {
          const { persistImagineMedia } = await import("./imagine-media");
          if (imageDataUrl) {
            const saved = await persistImagineMedia(id, imageDataUrl, "image");
            if (saved?.relPath) imageRelPath = saved.relPath;
            if (saved?.dataUrl) imageDataUrl = saved.dataUrl;
          }
          if (videoDataUrl) {
            const saved = await persistImagineMedia(id, videoDataUrl, "video");
            if (saved?.relPath) videoRelPath = saved.relPath;
            if (saved?.dataUrl) videoDataUrl = saved.dataUrl;
          }
        } catch {
          /* still show in-session */
        }

        set((s) => ({
          imagineBusy: false,
          imagineError: source === "local" && err ? err : err && source === "xai" ? err : null,
          imagineJobs: s.imagineJobs.map((j) =>
            j.id === id
              ? {
                  ...j,
                  status: "ready" as const,
                  imageDataUrl,
                  videoDataUrl,
                  imageRelPath,
                  videoRelPath,
                  mediaKind: outKind,
                  quality,
                  model,
                  source,
                  error: err || undefined,
                }
              : j,
          ),
        }));
        get().pushActivity({
          kind: "imagine",
          title:
            source === "xai"
              ? outKind === "video"
                ? "Imagine video ready (Grok)"
                : "Imagine ready (Grok)"
              : err
                ? "Imagine local preview (live failed)"
                : "Imagine ready (local preview)",
          detail:
            source === "xai"
              ? `${p.slice(0, 80)} · ${model || "xAI"} · ${aspect}/${quality}`
              : `${p.slice(0, 80)}${err ? ` · live failed: ${err}` : " · offline SVG"}`,
          status: source === "xai" ? "success" : err ? "failed" : "success",
        });
      },

      pushActivity: (item) => {
        const row: ActivityItem = {
          id: uid("act"),
          ts: item.ts ?? Date.now(),
          kind: item.kind,
          title: redactSecrets(item.title),
          detail: item.detail != null ? redactSecrets(String(item.detail)) : item.detail,
          status: item.status,
        };
        set((s) => ({ activity: [row, ...s.activity].slice(0, 80) }));
      },


      dismissProactiveNotice: () => set({ proactiveNotice: null }),

      runHealthCheck: async () => {
        const st = get();
        const oauth = st.oauth as { expiresAt?: number } | null;
        const hostConnected = st.connectors.some(
          (c) => c.id === "desktop-host" && c.status === "connected",
        );
        const result = await runHealthPass({
          autonomy: st.autonomy,
          hasOauth: Boolean(st.oauth?.accessToken),
          oauthExpiresAt: oauth?.expiresAt ?? null,
          hasApiKey: Boolean(st.apiKey?.trim()),
          hostConnected,
          modelsAgeMs: st.lastModelsFetchAt ? Date.now() - st.lastModelsFetchAt : null,
          streamingStuck: Boolean(st.streamingMessageId) && !st.running,
          running: st.running,
        });
        // Apply safe auto-fixes when proactive allows
        for (const f of result.autoFixes) {
          try {
            if (f.fix === "refresh_oauth") await get().refreshOAuthSession();
            else if (f.fix === "refresh_models") await get().refreshModels();
            else if (f.fix === "probe_host") {
              const { hostInfo } = await import("./host-client");
              const info = await hostInfo();
              if (info.unsandboxed && info.bridge !== "none") {
                set((s) => ({
                  connectors: s.connectors.map((c) =>
                    c.id === "desktop-host"
                      ? { ...c, status: "connected" as const, lastUsed: Date.now() }
                      : c,
                  ),
                }));
              }
            } else if (f.fix === "ensure_memory") {
              await ensureFileMemory();
            } else if (f.fix === "clear_stream") {
              finalizeChatStream(set, st.streamingMessageId || "");
            }
          } catch {
            /* ignore */
          }
        }
        return { ok: result.ok, detail: formatHealthMarkdown(result) };
      },

      runProactiveHousekeeping: async () => {
        const st = get();
        if (!proactiveEnabled(st.autonomy)) {
          return { ok: true, detail: "Proactive mode off or paused", fixed: 0 };
        }
        // Don't interrupt an active turn with auto-continue
        const actions = scanProactiveIssues({
          autonomy: st.autonomy,
          running: st.running,
          streamingMessageId: st.streamingMessageId,
          streamStatus: st.streamStatus,
          chat: st.chat,
          threads: st.threads,
          activeThreadId: st.activeThreadId,
          streamStartedAt,
        });
        let fixed = 0;
        const notes: string[] = [];
        for (const a of actions) {
          if (!a.auto) continue;
          if (a.kind === "clear_orphan_stream" || a.kind === "finalize_stuck_stream") {
            const mid = a.messageId || st.streamingMessageId || "";
            if (mid) finalizeChatStream(set, mid);
            streamStartedAt = null;
            fixed += 1;
            notes.push(a.title);
          } else if (a.kind === "clear_empty_assistant" && a.messageId) {
            set((s) => {
              const chat = s.chat.filter((m) => m.id !== a.messageId);
              const threads = s.threads.map((th) =>
                th.id === s.activeThreadId
                  ? { ...th, messages: chat, updatedAt: Date.now() }
                  : th,
              );
              return { chat, threads };
            });
            fixed += 1;
            notes.push(a.title);
          } else if (a.kind === "auto_continue" && !st.running) {
            markAutoContinue(st.activeThreadId);
            fixed += 1;
            notes.push(a.title);
            get().pushActivity({
              kind: "agent",
              title: "Proactive continue",
              detail: a.detail,
              status: "success",
            });
            void get().keepGoingChat();
            break;
          }
        }

        // Free-roaming safe chores (level 3+) — invent small maintenance without being asked
        if (canFreeRoam(st.autonomy) && !st.running) {
          const oauth = st.oauth as { expiresAt?: number } | null;
          const expiring =
            Boolean(oauth?.expiresAt) &&
            Number(oauth?.expiresAt) - Date.now() < 45 * 60 * 1000;
          const hostDown = !st.connectors.some(
            (c) => c.id === "desktop-host" && c.status === "connected",
          );
          const modelsStale =
            !st.lastModelsFetchAt || Date.now() - st.lastModelsFetchAt > 6 * 60 * 60 * 1000;
          const chores = planFreeRoamChores(st.autonomy, {
            oauthExpiring: expiring,
            hostLikelyDown: hostDown,
            modelsStale,
          });
          for (const ch of chores) {
            try {
              if (ch.action === "refresh_oauth") {
                await get().refreshOAuthSession();
              } else if (ch.action === "refresh_models") {
                await get().refreshModels();
              } else if (ch.action === "probe_host") {
                const { hostInfo } = await import("./host-client");
                const info = await hostInfo();
                if (info.unsandboxed && info.bridge !== "none") {
                  set((s) => ({
                    connectors: s.connectors.map((c) =>
                      c.id === "desktop-host"
                        ? { ...c, status: "connected" as const, lastUsed: Date.now() }
                        : c,
                    ),
                  }));
                }
              } else if (ch.action === "prune_learning" || ch.action === "flush_memory") {
                await get().flushLearningToDisk();
              }
              fixed += 1;
              notes.push(ch.title);
            } catch {
              /* ignore single chore failures */
            }
          }
        }

        if (fixed) {
          try {
            scheduleSettingsPersist();
          } catch {
            /* ignore */
          }
          const msg = notes.slice(0, 3).join(" · ");
          get().pushActivity({
            kind: "system",
            title: "Self-check",
            detail: msg,
            status: "success",
          });
          set({
            proactiveNotice: {
              message: msg,
              at: Date.now(),
            },
          });
        }
        return {
          ok: true,
          detail: fixed
            ? `Fixed ${fixed}: ${notes.slice(0, 2).join(", ")}`
            : "Nothing auto-applied",
          fixed,
        };
      },

      tickHeartbeat: () => {
        set((s) => ({
          heartbeatAt: Date.now(),
          usage: ensurePeriod(s.usage),
        }));
        // Heartbeat-driven automations
        void get().tickAutomations({ heartbeatOnly: true });
        // Proactive self-heal ~every 18s
        const now = Date.now();
        if (now - lastProactiveAt > 18_000) {
          lastProactiveAt = now;
          void get().runProactiveHousekeeping();
        }
      },

      setAgentStatus: (id, status, tasks) => {
        set((s) => ({
          agents: s.agents.map((a) =>
            a.id === id
              ? { ...a, status, tasks: typeof tasks === "number" ? tasks : a.tasks }
              : a,
          ),
        }));
      },

      refreshStaleTimes: () => {
        // Never wipe user chat, threads, skills, or automations on age.
        // Only refresh heartbeat + roll usage period if needed.
        const now = Date.now();
        set((s) => ({
          heartbeatAt: now,
          usage: ensurePeriod(s.usage, now),
        }));
      },

      resetDemo: () => {
        const fresh = createSeeds();
        set({
          connectors: fresh.connectors,
          skills: fresh.skills,
          automations: fresh.automations,
          activity: fresh.activity,
          chat: fresh.chat,
          threads: fresh.threads,
          activeThreadId: fresh.activeThreadId,
          agents: fresh.agents,
          profile: emptyProfile(),
          imagineJobs: [],
          imaginePrompt: "",
          imagineAspect: "1:1",
          imagineBusy: false,
          imagineError: null,
          mode: "auto",
          heartbeatAt: fresh.heartbeatAt,
          running: false,
          streamStatus: null,
          streamingMessageId: null,
          modelCatalog: emptyCatalog(),
          modelOverrides: {},
          lastModelsFetchAt: 0,
          nav: "chat",
          modeMenuOpen: false,
          usage: createUsage("pro"),
          grokConnected: null,
          grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",
          oauth: null,
          oauthPending: null,
          ssoCookie: "",
          openClawWorkspace: null,
          quickAssistMemory: emptyQuickAssistMemory(),
      quickAssistDismissed: [],
      quickAssistRotation: 0,
          learning: emptyLearning(),
          workboard: emptyWorkboard(),
          projectWorkspace: null,
          autonomy: defaultAutonomyConfig(),
          agentQueue: emptyAgentQueue(),
          pendingHostConfirm: null,
        });
      },
    }),
    {
      name: "grokhub-memory-v1",
      storage: createJSONStorage(() => persistentStorage),
      partialize: (s) => ({
        connectors: s.connectors,
        skills: s.skills,
        automations: s.automations,
        // Cap thread list but keep full message history per active threads
        threads: s.threads.slice(0, 80).map((t) => ({
          ...t,
          messages: (t.messages || []).slice(-120),
        })),
        activeThreadId: s.activeThreadId,
        sessionResume: s.sessionResume,
        agents: s.agents,
        mode: s.mode,
        desktop: s.desktop,
        agentPrefs: s.agentPrefs,
        composerDrafts: Object.fromEntries(
          Object.entries(s.composerDrafts || {}).slice(-40),
        ),
        shellHistory: (s.shellHistory || []).slice(0, 80),
        hostAllowlist: (s.hostAllowlist || []).slice(0, 40),
        usage: s.usage,
        learning: s.learning,
        workboard: s.workboard,
        projectWorkspace: s.projectWorkspace,
        autonomy: s.autonomy,
        agentQueue: {
          jobs: (s.agentQueue?.jobs || []).slice(0, 40),
          runningId: null,
          lastTickAt: s.agentQueue?.lastTickAt || 0,
        },
        // Persist imagine metadata + disk paths (bytes live in userData/imagine-media)
        imagineJobs: s.imagineJobs.slice(0, 32).map((j) => {
          const {
            imageDataUrl,
            videoDataUrl,
            referenceDataUrl,
            imageRelPath,
            videoRelPath,
            ...rest
          } = j;
          // Keep tiny local SVG previews inline when no disk path
          const keepImg =
            !imageRelPath &&
            imageDataUrl &&
            imageDataUrl.startsWith("data:image/svg") &&
            imageDataUrl.length < 80_000
              ? imageDataUrl
              : undefined;
          return {
            ...rest,
            imageRelPath,
            videoRelPath,
            imageDataUrl: keepImg,
            // never persist huge base64 / remote refs in JSON
          };
        }),
        imagineAspect: s.imagineAspect,
        imagineMediaKind: s.imagineMediaKind,
        imagineQuality: s.imagineQuality,
        openClawWorkspace: s.openClawWorkspace
          ? {
              ...s.openClawWorkspace,
              contextBundle: s.openClawWorkspace.contextBundle.slice(0, 80_000),
            }
          : null,
        profile: s.profile,
        modelCatalog: s.modelCatalog,
        modelOverrides: cleanModelOverrides(s.modelOverrides),
        lastModelsFetchAt: s.lastModelsFetchAt,
        // chat is derived from active thread on hydrate — avoid dual storage bloat
        activity: s.activity.slice(0, 40).map((a) => ({
          ...a,
          title: redactSecrets(a.title),
          detail: a.detail != null ? redactSecrets(String(a.detail)) : a.detail,
        })),
        quickAssistMemory: s.quickAssistMemory,
        quickAssistDismissed: (s.quickAssistDismissed || []).slice(-40),
        // rotation is session-ish but persist lightly so reopen still varies
        quickAssistRotation: s.quickAssistRotation || 0,
        uiTheme: s.uiTheme || "dark",
        toolsNavCollapsed: Boolean(s.toolsNavCollapsed),
        preferFreeGrok: false,
        setupSyncMeta: s.setupSyncMeta || { autoPullOnLogin: true, autoPushOnChange: false },
        // Restore last tab (connectors removed — remapped on hydrate)
        nav:
          s.nav === "connectors" || s.nav === "agents" || s.nav === "queue" || s.nav === "desktop"
            ? "chat"
            : s.nav || "chat",
        // Secrets stay in safeStorage (userData), not here
      }),
      version: 1,
      migrate: (persisted: unknown) => {
        const s = (persisted || {}) as Record<string, unknown>;
        const cat = s.modelCatalog as Record<string, unknown> | undefined;
        if (cat && (!cat.classifiedBy || !cat.slots)) {
          s.modelCatalog = emptyCatalog();
        } else if (cat && !cat.classifiedBy) {
          s.modelCatalog = {
            ...emptyCatalog(),
            ...cat,
            classifiedBy: cat.classifiedBy || "heuristic",
            classifiedAt: cat.classifiedAt || 0,
            signature: cat.signature || "",
          };
        }
        s.modelOverrides = cleanModelOverrides(
          (s as { modelOverrides?: ModelModeOverrides }).modelOverrides,
        );
        s.learning = normalizeLearning((s as { learning?: unknown }).learning);
        s.workboard = normalizeWorkboard((s as { workboard?: unknown }).workboard);
        s.autonomy = normalizeAutonomy((s as { autonomy?: unknown }).autonomy);
        s.agentQueue = normalizeAgentQueue((s as { agentQueue?: unknown }).agentQueue);
        // Drop legacy generic seed welcome bubble so adaptive empty-state can show
        try {
          const threads = Array.isArray(s.threads) ? (s.threads as ChatThread[]) : [];
          s.threads = threads.map((th) => {
            const msgs = th.messages || [];
            if (
              msgs.length === 1 &&
              msgs[0]?.role === "system" &&
              /welcome — connect grok/i.test(String(msgs[0]?.content || ""))
            ) {
              return {
                ...th,
                messages: [],
                title: th.title === "Welcome" ? "New chat" : th.title,
              };
            }
            return th;
          });
          if (s.activeThreadId) {
            const active = (s.threads as ChatThread[]).find(
              (th) => th.id === s.activeThreadId,
            );
            if (active) s.chat = active.messages || [];
          }
        } catch {
          /* ignore */
        }
        if ((s as { projectWorkspace?: unknown }).projectWorkspace && typeof (s as { projectWorkspace?: { path?: string } }).projectWorkspace === "object") {
          /* keep as-is */
        } else {
          (s as { projectWorkspace: null }).projectWorkspace = null;
        }
        s.quickAssistMemory = normalizeMemory(s.quickAssistMemory);
        if (!Array.isArray(s.quickAssistDismissed)) s.quickAssistDismissed = [];
        if (typeof s.quickAssistRotation !== "number") s.quickAssistRotation = 0;
        // merge catalog connectors (new website ids without wiping status)
        try {
          const cat = createSeeds().connectors;
          const cur = Array.isArray(s.connectors) ? (s.connectors as import("./types").Connector[]) : [];
          const byId = new Map(cur.map((c) => [c.id, c]));
          for (const c of cat) {
            if (!byId.has(c.id)) byId.set(c.id, c);
          }
          s.connectors = Array.from(byId.values());
        } catch {
          /* ignore */
        }
        if (s.imagineMediaKind !== "image" && s.imagineMediaKind !== "video") s.imagineMediaKind = "image";
        if (s.imagineQuality !== "speed" && s.imagineQuality !== "quality") s.imagineQuality = "speed";
        if (!s.imagineAspect) s.imagineAspect = "auto";
        if (!Array.isArray(s.imagineJobs)) s.imagineJobs = [];
        // Never restore ephemeral run state (crashed mid-stream left sticky UI)
        s.running = false;
        s.streamStatus = null;
        s.streamingMessageId = null;
        s.running = false;
        s.pendingHostConfirm = null;
        if (!s.composerDrafts || typeof s.composerDrafts !== "object") s.composerDrafts = {};
        if (!Array.isArray(s.shellHistory)) s.shellHistory = [];
        if (!Array.isArray(s.hostAllowlist)) s.hostAllowlist = [];
        s.undoBuffer = null;
        // Never rehydrate mid-stream chrome
        try {
          if (Array.isArray(s.chat)) {
            s.chat = (s.chat as import("./types").ChatMessage[]).map((m) =>
              m.streaming ? { ...m, streaming: false } : m,
            );
          }
          if (Array.isArray(s.threads)) {
            s.threads = (s.threads as import("./types").ChatThread[]).map((th) => ({
              ...th,
              messages: (th.messages || []).map((m) =>
                m.streaming ? { ...m, streaming: false } : m,
              ),
            }));
          }
        } catch {
          /* ignore */
        }
        // Resume card only for real interrupts (drop legacy "last chat" cards)
        try {
          const r = s.sessionResume as Record<string, unknown> | null;
          if (!r || r.kind !== "interrupted" || !r.threadId || !r.pendingPrompt) {
            s.sessionResume = null;
          }
        } catch {
          s.sessionResume = null;
        }
        // Context continuity: re-bind chat from active thread when messages drifted
        try {
          const threads = Array.isArray(s.threads) ? (s.threads as import("./types").ChatThread[]) : [];
          const aid = s.activeThreadId as string | null;
          const active = threads.find((th) => th.id === aid) || threads[0];
          if (active) {
            s.activeThreadId = active.id;
            const chat = Array.isArray(s.chat) ? (s.chat as import("./types").ChatMessage[]) : [];
            if (!chat.length || chat.length < (active.messages?.length || 0)) {
              s.chat = active.messages || [];
            }
            if (
              active.mode &&
              ["auto", "fast", "balanced", "expert", "heavy", "max", "build"].includes(
                String(active.mode),
              )
            ) {
              // Do not force routed modes onto global selector on boot —
              // prefer the persisted global mode (partialize). Only fill if missing.
              if (!s.mode) s.mode = active.mode;
            }
            // Global mode must stay valid after version upgrades
            if (
              !["auto", "fast", "balanced", "expert", "heavy", "max", "build"].includes(
                String(s.mode || ""),
              )
            ) {
              s.mode = "auto";
            }
          }
        } catch {
          /* ignore */
        }
        // desktop defaults
        try {
          const d = (s.desktop || {}) as Record<string, unknown>;
          if (typeof d.hostSafeMode !== "boolean") d.hostSafeMode = false;
          if (typeof d.confirmHostCommands !== "boolean") d.confirmHostCommands = true;
          s.desktop = d;
        } catch {
          /* ignore */
        }
        try {
          const ap = (s.agentPrefs || {}) as Record<string, unknown>;
          s.agentPrefs = {
            temperature: typeof ap.temperature === "number" ? ap.temperature : 0.7,
            hostToolsEnabled: ap.hostToolsEnabled !== false,
            connectorToolsEnabled: ap.connectorToolsEnabled !== false,
            memoryNotes: typeof ap.memoryNotes === "string" ? ap.memoryNotes : "",
          };
        } catch {
          s.agentPrefs = {
            temperature: 0.7,
            hostToolsEnabled: true,
            connectorToolsEnabled: true,
            memoryNotes: "",
          };
        }
        // Host safety defaults for upgrades
        const desk = (s.desktop || {}) as Record<string, unknown>;
        if (typeof desk.hostSafeMode !== "boolean") desk.hostSafeMode = false;
        if (desk.confirmHostCommands === undefined) desk.confirmHostCommands = true;
        if (desk.globalHotkey === undefined) desk.globalHotkey = "Super+Space";
        if (desk.confirmDestructiveOnly === undefined) desk.confirmDestructiveOnly = true;
        if (desk.selfModifyEnabled === undefined) desk.selfModifyEnabled = false;
        s.desktop = desk;

        if (!s.setupSyncMeta) s.setupSyncMeta = { autoPullOnLogin: true, autoPushOnChange: false };
        s.preferFreeGrok = false; // free Grok product surface removed
        if (s.uiTheme !== "dark" && s.uiTheme !== "light" && s.uiTheme !== "system") s.uiTheme = "dark";
        if (s.toolsNavCollapsed === undefined) s.toolsNavCollapsed = false;
        // Drop website-only connector catalog (Gmail, Notion, …) — keep core three
        try {
          s.connectors = pruneToCoreConnectors(
            (s.connectors as import("./types").Connector[]) || [],
          );
        } catch {
          /* ignore */
        }
        // normalize automation times / heartbeat fields
        if (Array.isArray(s.automations)) {
          s.automations = (s.automations as import("./types").Automation[]).map((a) => {
            const times =
              Array.isArray(a.times) && a.times.length
                ? a.times
                : a.time
                  ? [a.time]
                  : ["09:00"];
            return {
              ...a,
              times,
              time: times[0] || a.time || "09:00",
              heartbeatEveryMin: a.heartbeatEveryMin || 5,
              connectorIds: (a.connectorIds || []).filter((id) =>
                ["grok-xai", "desktop-host", "github"].includes(id),
              ),
            };
          });
        }
        // Drop old demo-seeded usage (842 units SuperGrok Pro) so meter shows real usage
        const u = s.usage as Record<string, unknown> | undefined;
        if (u) {
          const tokens = Number(u.totalTokens ?? 0);
          const used = Number(u.usedUnits ?? 0);
          if (tokens === 0 && (used === 842 || used === 210 || used === 28 || !("totalTokens" in u))) {
            const plan = (u.plan as "free" | "super" | "pro") || "pro";
            s.usage = createUsage(plan);
          } else {
            s.usage = ensurePeriod(u as import("./types").UsageSnapshot);
          }
        }
        return s as typeof s;
      },
      skipHydration: true,
      onRehydrateStorage: () => (state, err) => {
        if (err || !state) return;
        // Create ~/.config/GrokHub/memory/* and mirror learning STATUS.md
        void ensureFileMemory().then(() => {
          try {
            state.flushLearningToDisk();
          } catch {
            /* ignore */
          }
        });
        // Reload Imagine media from disk after update/restart
        void import("./imagine-media").then(async ({ rehydrateImagineJobs }) => {
          try {
            const jobs = await rehydrateImagineJobs(state.imagineJobs || []);
            useGrokHub.setState({ imagineJobs: jobs });
          } catch {
            /* ignore */
          }
        });
      },
    },
  ),
);

function wait(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

/**
 * Always clear streaming chrome for a finished/aborted/superseded turn.
 * Prevents assistant bubbles stuck with streaming:true forever.
 */
function finalizeChatStream(
  set: (fn: (s: any) => any) => void,
  botId: string,
  opts?: { content?: string; markStopped?: boolean },
) {
  streamStartedAt = null;
  if (!botId) {
    set((s) => ({
      running: false,
      streamStatus: null,
      streamingMessageId: null,
    }));
    return;
  }
  set((s) => {
    const owns =
      s.streamingMessageId === botId ||
      s.chat.some((row: { id: string; streaming?: boolean }) => row.id === botId && row.streaming);
    const chat = s.chat.map((row: ChatMessage) => {
      if (row.id === botId) {
        return {
          ...row,
          streaming: false,
          ...(opts?.markStopped ? { stopped: true } : {}),
          ...(opts?.content != null && opts.content !== ""
            ? { content: opts.content }
            : {}),
        };
      }
      // Only clear other orphans if we still own the stream slot
      if (owns && row.role === "assistant" && row.streaming) {
        return { ...row, streaming: false };
      }
      return row;
    });
    const tid = s.activeThreadId;
    const threads = s.threads.map((th: ChatThread) => {
      if (th.id === tid) {
        return { ...th, messages: chat, updatedAt: Date.now() };
      }
      return th;
    });
    if (!owns) {
      // Another turn owns the stream — only patch this bot bubble in chat/threads
      return { chat, threads };
    }
    return {
      chat,
      threads,
      running: false,
      streamStatus: null,
      streamingMessageId: null,
    };
  });
}

function endChatTurnPersist() {
  void import("./persistent-storage").then(({ setPersistPaused }) => setPersistPaused(false));
  activeHostJobId = null;
  activeHostJobIds.clear();
}

/** Active chat stream abort (module-level so Stop works across re-renders) */
let activeChatAbort: AbortController | null = null;
let chatGeneration = 0;
let streamStartedAt: number | null = null;
let lastProactiveAt = 0;