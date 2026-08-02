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
  migrateNotesToFileMemory,
} from "./file-memory";
import { renderImaginePreview } from "./imagine";
import { getMode, resolveMode, resolveModeWithCatalog, stripAssistantChrome, modelIdForMode, autoRouteFor } from "./modes";
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
  type QuickAssistMemory,
} from "./quick-assist-memory";
import type { QuickChip } from "./quick-assistant";

/** Waits for user approval of host commands (agent tool loop). */
let hostConfirmWaiter: ((allow: boolean) => void) | null = null;
/** In-flight host exec job id (for Stop → killExec). */
let activeHostJobId: string | null = null;

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
  /** Adaptive quick-assist chip habits */
  quickAssistMemory: QuickAssistMemory;
  /** Chip values/ids the user dismissed */
  quickAssistDismissed: string[];
  /** Bumps to rotate alternate chip packs */
  quickAssistRotation: number;
  usage: UsageSnapshot;
  heartbeatAt: number;
  running: boolean;
  /** Live status line while agent is working (streaming / host / stopped) */
  streamStatus: string | null;
  /** Id of the assistant message currently streaming */
  streamingMessageId: string | null;
  /** Live essential models from xAI */
  modelCatalog: ResolvedCatalog;
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
  setDesktop: (patch: Partial<State["desktop"]>) => void;
  resolveHostConfirm: (allow: boolean) => void;
  tickAutomations: (opts?: { heartbeatOnly?: boolean }) => Promise<void>;
  hydrateSecrets: () => Promise<void>;
  recordQuickAssistChip: (chip: QuickChip) => void;
  recordQuickAssistTyped: (text: string) => void;
  recordQuickAssistOutcome: (outcome: "success" | "failure") => void;
  clearQuickAssistMemory: () => void;
  dismissQuickAssistChip: (chip: QuickChip) => void;
  rotateQuickAssist: () => void;
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
  syncFromGrok: (opts?: { displayName?: string | null; email?: string | null; imageUrl?: string | null }) => Promise<void>;
  newThread: () => void;
  selectThread: (id: string) => void;
  deleteThread: (id: string) => void;
  renameThread: (id: string, title: string) => void;
  pinThread: (id: string, pinned?: boolean) => void;
  setThreadFolder: (id: string, folder: string | null) => void;
  dismissSessionResume: () => void;
  resumeLastSession: () => void;
  /** After an interrupt: drop partial assistant reply and re-run last user prompt */
  continueInterruptedSession: () => Promise<void>;
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
  const lower = text.toLowerCase();
  const connected = s.connectors.filter((c) => c.status === "connected");
  const enabledSkills = s.skills.filter((sk) => sk.enabled);
  const depth = getMode(routed).depth;
  const plan = PLAN_LIMITS[s.usage.plan];
  const pct = Math.round(usagePercent(s.usage));

  if (lower.includes("usage") || lower.includes("quota") || lower.includes("limit") || lower.includes("subscription")) {
    return [
      "Subscription usage",
      "",
      `Plan: ${plan.label}`,
      `Units: ${s.usage.usedUnits.toFixed(1)} / ${plan.units} (${pct}%)`,
      `Messages ${s.usage.messages}/${plan.messages} · Imagine ${s.usage.imagine}/${plan.imagine}`,
      `Automations ${s.usage.automations}/${plan.automations} · Host ${s.usage.host}/${plan.host}`,
      "",
      "Heavy = 8u · Expert = 4u · Build = 2u · Fast = 1u · Imagine = 5u",
      "Open Settings for the full meter and plan switcher.",
    ].join("\n");
  }

  if (lower.startsWith("/morning") || lower.includes("morning brief")) {
    const core = [
      "",
      "Morning Brief",
      "",
      `- Connectors live: ${connected.map((c) => c.name).join(", ") || "none"}`,
      "- Calendar: 2 meetings after 13:00, free block 10:00–12:00",
      "- Inbox: 4 unread · 1 invoice reminder · 1 shipping notice",
      "- Linear: 2 P0s · GitHub: 3 review requests",
      `- Usage: ${pct}% of ${plan.label} period budget`,
    ];
    if (depth === "light") {
      return [...core, "", "Top move: clear P0 Linear, then PR reviews."].join("\n");
    }
    if (depth === "team") {
      return [
        ...core,
        "",
        "Team pass (Heavy):",
        "- Ops: confirm dependencies on the two P0s",
        "- Research: gather context from last related issue threads",
        "- Build: draft a short checklist skill if the workflow repeats",
        "- Primary: sequence deep work under 90 minutes",
      ].join("\n");
    }
    if (depth === "code") {
      return [
        ...core,
        "",
        "Build angle:",
        "- Ship GrokHub desktop install path first",
        "- Wire mode routing tests before new connectors",
        "- Package: Electron + Arch PKGBUILD ready",
      ].join("\n");
    }
    return [
      ...core,
      "",
      "Suggested order: P0 Linear → PR reviews → inbox drafts → lunch buffer.",
      depth === "deep"
        ? "Risk: context switching across tools — batch connector work."
        : "",
    ]
      .filter(Boolean)
      .join("\n");
  }

  if (lower.startsWith("/standup") || lower.includes("standup")) {
    return [
      "",
      "Standup",
      "",
      "- Yesterday: connector triage + mode routing polish",
      "- Today: desktop host checks and packaging notes",
      "- Blockers: none — usage meter and Imagine ready for demos",
      depth === "code"
        ? "- Build: keep /standup skill logging shipped items weekly"
        : "",
    ]
      .filter(Boolean)
      .join("\n");
  }

  if (lower.includes("imagine") || lower.startsWith("/imagine")) {
    return [
      "",
      "Imagine is available in the Imagine panel.",
      "Describe a scene there — GrokHub renders a local preview on this Arch desktop build.",
      `Imagine quota: ${s.usage.imagine}/${plan.imagine} this period (5 units each).`,
    ].join("\n");
  }

  if (lower.includes("mode") || lower.includes("fast") || lower.includes("expert") || lower.includes("heavy")) {
    return [
      "",
      "Baked-in Grok modes (same as web):",
      "- Adaptive — Real router: ⚡ Fast · 🧠 Think · 🔬 Deep · 🛠️ Build",
      "- Fast — Quick responses · grok-4-1-fast · 1 unit",
      "- Expert — Thinks hard · grok-4.3 · 4 units",
      "- Heavy — Team of Experts · grok-4.3 · 8 units",
      "- Build — Build apps and sites · grok-code-fast-1 · 2 units",
      "",
      `Active: ${getMode(s.mode).label}${s.mode === "auto" ? ` (this turn → ${getMode(routed).label})` : ""}`,
      "Change modes from the titlebar picker or Settings.",
    ].join("\n");
  }

  if (lower.includes("connector") || lower.includes("connect")) {
    return [
      "",
      "Connector status",
      "",
      ...s.connectors.map((c) => `- ${c.name}: ${c.status}`),
    ].join("\n");
  }

  if (lower.includes("automat") || lower.includes("schedule")) {
    return [
      "",
      "Automations",
      "",
      ...s.automations.map(
        (a) =>
          `- ${a.enabled ? "ON" : "OFF"} ${a.name} (${a.schedule} @ ${a.time}) · ${a.runCount} runs`,
      ),
    ].join("\n");
  }

  if (lower.includes("skill")) {
    return [
      "",
      "Skills",
      "",
      ...enabledSkills.map((sk) => `- ${sk.slash} — ${sk.name}`),
    ].join("\n");
  }

  if (depth === "code" || lower.includes("build") || lower.includes("arch") || lower.includes("desktop")) {
    return [
      "",
      "Build / desktop plan",
      "",
      "GrokHub ships as an Electron shell for Arch Linux:",
      "- `desktop/main.mjs` — native window, tray, Wayland-friendly flags",
      "- `packaging/PKGBUILD` — makepkg install",
      "- Unsandboxed host: CLI, files, apps via Desktop tab or `$ command`",
      "",
      `Plan ${plan.label}: host CLI ${s.usage.host}/${plan.host} this period.`,
    ].join("\n");
  }

  if (depth === "team") {
    return [
      "",
      "Heavy · Team of Experts",
      "",
      `Goal: ${text}`,
      "",
      "1) Planner — break into 3 workstreams",
      "2) Researcher — pull connector context (mail/code/issues)",
      "3) Critic — risk + cheapest path",
      "4) Builder — ship checklist",
      "",
      `Context: ${connected.length} connectors · ${enabledSkills.length} skills · ${pct}% usage.`,
    ].join("\n");
  }

  if (depth === "deep") {
    return [
      "",
      "Expert analysis",
      "",
      `Reading: ${text}`,
      "",
      "Constraints: local-first control plane, Grok modes (Fast/Expert/Heavy/Build), Arch desktop target.",
      "Approach: gather connector state → apply enabled skills → leave run log.",
      "Tradeoff: Fast is cheaper/latency; Expert/Heavy spend units for depth.",
      "",
      `Live tools: ${connected.map((c) => c.name).join(", ") || "none connected"}.`,
    ].join("\n");
  }

  if (depth === "light") {
    return [
      "",
      `Got it — ${text.slice(0, 120)}${text.length > 120 ? "…" : ""}`,
      `Using ${connected.length} connectors · ${enabledSkills.length} skills · ${pct}% quota.`,
      "Say /morning, open Imagine, or switch to Expert for deeper work.",
    ].join("\n");
  }

  return [
    "",
    "Primary co-pilot",
    "",
    `Goal: ${text}`,
    `Using ${connected.length} connectors and ${enabledSkills.length} enabled skills.`,
    "Next: break into steps → pull tools → run skills → log.",
    "Try /morning, /standup, Imagine, or Heavy mode for a team pass.",

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
      pendingHostConfirm: null,
      quickAssistMemory: emptyQuickAssistMemory(),
      quickAssistDismissed: [],
      quickAssistRotation: 0,
      modelCatalog: emptyCatalog(),
      lastModelsFetchAt: 0,
      apiKey: "",
      githubToken: "",
      oauth: null,
      ssoCookie: "",
      openClawWorkspace: null,
      oauthPending: null,
      setupSyncMeta: { autoPullOnLogin: true, autoPushOnChange: false },
      grokConnected: null,
      preferFreeGrok: true,
      uiTheme: "dark" as const,
      toolsNavCollapsed: false,
      grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",

      setNav: (nav) => set({ nav, modeMenuOpen: false }),
      setMode: (mode) => {
        set({ mode, modeMenuOpen: false });
        get().pushActivity({
          kind: "system",
          title: `Mode → ${getMode(mode).label}`,
          detail: getMode(mode).subtitle,
          status: "success",
        });
      },
      setModeMenuOpen: (open) => set({ modeMenuOpen: open }),
      setDesktop: (patch) => {
        set((s) => ({ desktop: { ...s.desktop, ...patch } }));
        if (typeof patch.hostSafeMode === "boolean" && typeof window !== "undefined") {
          try {
            void window.grokhubDesktop?.host?.setSafeMode?.(patch.hostSafeMode);
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
        set((s) => ({
          quickAssistMemory: rememberChipClick(s.quickAssistMemory, chip),
          // Rotate pack after a click so the next suggestions feel fresh
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
        });
      },

      dismissQuickAssistChip: (chip) => {
        const key = (chip.value || chip.id || "").trim();
        if (!key) return;
        set((s) => ({
          quickAssistDismissed: [...new Set([...(s.quickAssistDismissed || []), key, chip.id])].slice(
            -60,
          ),
          quickAssistRotation: (s.quickAssistRotation || 0) + 1,
        }));
      },

      rotateQuickAssist: () => {
        set((s) => ({
          quickAssistRotation: (s.quickAssistRotation || 0) + 1,
          // Clear a few oldest dismissals so Suggest can reintroduce useful chips
          quickAssistDismissed: (s.quickAssistDismissed || []).slice(-20),
        }));
      },

      syncWebsiteConnectors: async () => {
        try {
          const { fetchWebsiteConnectors } = await import("./website-connectors");
          const { createSeeds } = await import("./seed");
          const r = await fetchWebsiteConnectors({
            ssoCookie: get().ssoCookie || undefined,
            bearer: get().oauth?.accessToken,
          });
          // Ensure catalog has all known ids
          const catalog = createSeeds().connectors;
          set((s) => {
            const byId = new Map(s.connectors.map((c) => [c.id, c]));
            for (const c of catalog) {
              if (!byId.has(c.id)) byId.set(c.id, c);
            }
            // Apply website hits
            for (const hit of r.connectors) {
              const prev = byId.get(hit.id);
              if (!prev) {
                byId.set(hit.id, {
                  id: hit.id,
                  name: hit.name,
                  category: "Website",
                  description: "Linked on Grok website",
                  status: "connected",
                  tools: [],
                  accountLabel: hit.accountLabel,
                  source: "website",
                  liveTools: hit.id === "github",
                  lastUsed: Date.now(),
                });
                continue;
              }
              byId.set(hit.id, {
                ...prev,
                status: "connected",
                accountLabel: hit.accountLabel || prev.accountLabel,
                source: prev.source === "token" || prev.liveTools ? prev.source : "website",
                // GitHub stays live if token present
                liveTools:
                  prev.id === "github"
                    ? Boolean(get().githubToken) || prev.liveTools
                    : prev.id === "desktop-host" || prev.id === "grok-xai"
                      ? true
                      : false,
                lastUsed: Date.now(),
                description: hit.accountLabel
                  ? `${prev.description.split(" · ")[0]} · ${hit.accountLabel}`
                  : prev.description,
              });
            }
            // GitHub token implies connected + live
            if (get().githubToken) {
              const gh = byId.get("github");
              if (gh) {
                byId.set("github", {
                  ...gh,
                  status: "connected",
                  liveTools: true,
                  source: "token",
                  lastUsed: Date.now(),
                });
              }
            }
            return { connectors: Array.from(byId.values()) };
          });
          if (r.ok && r.connectors.length) {
            get().pushActivity({
              kind: "connector",
              title: "Website connectors synced",
              detail: r.detail,
              status: "success",
            });
          }
          return {
            ok: r.ok,
            detail: r.detail,
            count: r.connectors.length,
          };
        } catch (e) {
          const detail = e instanceof Error ? e.message : "sync failed";
          return { ok: false, detail, count: 0 };
        }
      },

      setApiKey: (key) => {
        set({ apiKey: key, grokConnected: null });
        void import("./secrets-client").then((m) => m.secretsSet("apiKey", key));
      },
      setGithubToken: (token) => {
        set({ githubToken: token });
        void import("./secrets-client").then((m) => m.secretsSet("githubToken", token));
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
              : "Free Grok · website session linked"
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
              if (ensured.tokens) set({ oauth: ensured.tokens });
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
        const now = Date.now();
        // Empty thread — no repeating welcome spam on every New chat / restart
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
      },

      selectThread: (id) => {
        const t = get().threads.find((x) => x.id === id);
        if (!t) return;
        set({
          activeThreadId: id,
          chat: t.messages,
          nav: "chat",
          mode: t.mode || get().mode,
        });
      },

      deleteThread: (id) => {
        const remaining = get().threads.filter((t) => t.id !== id);
        if (remaining.length === 0) {
          get().newThread();
          return;
        }
        const nextActive =
          get().activeThreadId === id
            ? remaining[0]!
            : remaining.find((t) => t.id === get().activeThreadId) || remaining[0]!;
        set({
          threads: remaining,
          activeThreadId: nextActive.id,
          chat: nextActive.messages,
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
        set((s) => {
          const chat = s.chat.filter((m) => !idSet.has(m.id));
          // Drop reply target if deleted
          const replyTo =
            s.replyTo && idSet.has(s.replyTo.id) ? null : s.replyTo;
          const tid = s.activeThreadId;
          return {
            chat,
            replyTo,
            threads: s.threads.map((th) =>
              th.id === tid ? threadWithMessages(th, chat) : th,
            ),
          };
        });
        get().pushActivity({
          kind: "chat",
          title: "Message deleted",
          detail: `${idSet.size} message${idSet.size === 1 ? "" : "s"} removed`,
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

      setPreferFreeGrok: (v) => set({ preferFreeGrok: Boolean(v) }),
      setUiTheme: (t) => set({ uiTheme: t }),
      setToolsNavCollapsed: (v) => set({ toolsNavCollapsed: Boolean(v) }),

      setPlan: (plan) => {
        const prev = get().usage;
        const next = createUsage(plan);
        set({ usage: next });
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
          if (base.usedUnits + cost > lim.units * 1.02) {
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
              source: "local",
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
          if (base.usedUnits + cost > lim.units * 1.05) {
            ok = false;
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
              source: "live",
              rateLimitRemaining: rateLimit?.remaining ?? base.rateLimitRemaining ?? null,
              rateLimitLimit: rateLimit?.limit ?? base.rateLimitLimit ?? null,
              rateLimitResetAt: rateLimit?.resetAt ?? base.rateLimitResetAt ?? null,
            },
          };
        });
        return { ok, cost };
      },

      refreshUsage: async () => {
        const st = get();
        let usage = ensurePeriod(st.usage);
        const inferred = inferPlanFromAuth({
          hasOauth: Boolean(st.oauth?.accessToken),
          hasApiKey: Boolean(st.apiKey?.trim()),
          email: st.oauth?.email || st.profile?.email,
          name: st.oauth?.name || st.profile?.displayName,
        });
        if (usage.plan === "free" && inferred !== "free") {
          usage = { ...usage, plan: inferred };
        }

        try {
          const { fetchGrokWebsiteUsage } = await import("./grok-website-usage");
          let sso = st.ssoCookie?.trim() || "";
          if (!sso && typeof window !== "undefined" && window.grokhubDesktop?.grok?.getWebsiteSso) {
            try {
              const r = await window.grokhubDesktop.grok.getWebsiteSso();
              if (r?.cookie) {
                sso = r.cookie;
                set({ ssoCookie: sso });
                try {
                  void window.grokhubDesktop?.secrets?.set?.("ssoCookie", sso);
                } catch {
                  /* ignore */
                }
              }
            } catch {
              /* ignore */
            }
          }
          const bearer =
            st.oauth?.accessToken?.trim() || st.apiKey?.trim() || null;
          const web = await fetchGrokWebsiteUsage({
            ssoCookie: sso || null,
            bearer: sso ? null : bearer,
          });

          if (web.ok) {
            let pct = Number(web.creditUsagePercent) || 0;
            if (pct > 0 && pct <= 1.0001) pct *= 100;
            const planMap =
              web.planId === "heavy" || web.planId === "pro"
                ? ("pro" as const)
                : web.planId === "free"
                  ? ("free" as const)
                  : ("super" as const);
            const unitCap = PLAN_LIMITS[planMap].units;
            const products = (web.productUsage || []).map((row) => {
              let up = Number(row.usagePercent) || 0;
              if (up > 0 && up <= 1.0001) up *= 100;
              return { ...row, usagePercent: up };
            });
            usage = {
              ...usage,
              plan: planMap,
              periodStart: web.periodStart || usage.periodStart,
              periodEnd: web.periodEnd || usage.periodEnd,
              usedUnits: Math.round((pct / 100) * unitCap * 100) / 100,
              source: "website",
              lastPolledAt: Date.now(),
              website: {
                planLabel: web.planLabel || PLAN_LIMITS[planMap].label,
                creditUsagePercent: pct,
                periodType: web.periodType || "weekly",
                periodStart: web.periodStart,
                periodEnd: web.periodEnd,
                productUsage: products,
                prepaidBalanceCents: web.prepaidBalanceCents || 0,
                onDemandCapCents: web.onDemandCapCents || 0,
                onDemandUsedCents: web.onDemandUsedCents || 0,
                error: null,
              },
            };
            set({ usage });
            return;
          }

          const prevWeb = usage.website;
          usage = {
            ...usage,
            lastPolledAt: Date.now(),
            source:
              prevWeb && prevWeb.error == null && prevWeb.creditUsagePercent != null
                ? usage.source
                : "local",
            website: {
              planLabel: prevWeb?.planLabel || PLAN_LIMITS[usage.plan].label,
              creditUsagePercent:
                prevWeb?.creditUsagePercent ??
                Math.round(usagePercent(usage) * 10) / 10,
              periodType: prevWeb?.periodType || "unknown",
              periodStart: prevWeb?.periodStart ?? usage.periodStart,
              periodEnd: prevWeb?.periodEnd ?? usage.periodEnd,
              productUsage: prevWeb?.productUsage || [],
              prepaidBalanceCents: prevWeb?.prepaidBalanceCents ?? 0,
              onDemandCapCents: prevWeb?.onDemandCapCents ?? 0,
              onDemandUsedCents: prevWeb?.onDemandUsedCents ?? 0,
              error:
                web.error ||
                "Could not load grok.com usage — link website session in Settings",
            },
          };
        } catch (e) {
          const prevWeb = usage.website;
          usage = {
            ...usage,
            lastPolledAt: Date.now(),
            website: {
              planLabel: prevWeb?.planLabel || PLAN_LIMITS[usage.plan].label,
              creditUsagePercent:
                prevWeb?.creditUsagePercent ??
                Math.round(usagePercent(usage) * 10) / 10,
              periodType: prevWeb?.periodType || "unknown",
              periodStart: prevWeb?.periodStart ?? usage.periodStart,
              periodEnd: prevWeb?.periodEnd ?? usage.periodEnd,
              productUsage: prevWeb?.productUsage || [],
              prepaidBalanceCents: prevWeb?.prepaidBalanceCents ?? 0,
              onDemandCapCents: prevWeb?.onDemandCapCents ?? 0,
              onDemandUsedCents: prevWeb?.onDemandUsedCents ?? 0,
              error: e instanceof Error ? e.message : "Usage poll failed",
            },
          };
        }
        set({ usage });
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
        const mode = get().mode;
        const routed = resolveMode(mode, skill.instructions);
        const m = getMode(routed);
        const bill = get().recordUsage("skill", routed);
        if (!bill.ok) return;
        set({ running: true });
        get().setAgentStatus("primary", "working", 1);
        get().pushActivity({
          kind: "skill",
          title: `Running ${skill.name}`,
          detail: `${skill.slash} · ${m.label} · ${bill.cost}u`,
          status: "running",
        });
        await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
        set((s) => ({
          running: false,
          skills: s.skills.map((sk) =>
            sk.id === id ? { ...sk, runs: sk.runs + 1 } : sk,
          ),
        }));
        get().setAgentStatus("primary", "idle", 0);
        get().pushActivity({
          kind: "skill",
          title: `${skill.name} finished`,
          detail: skill.instructions.slice(0, 120),
          status: "success",
        });
        set((s) => ({
          chat: [
            ...s.chat,
            {
              id: uid("msg"),
              role: "assistant",
              content: replyFor(skill.slash, get(), routed),
              ts: Date.now(),
              mode: routed,
            },
          ],
        }));
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
        if (get().running) {
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
        set({ running: true, streamStatus: `Automation: ${auto.name}` });
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
          const { grokChat } = await import("./grok-client");
          const prompt = [
            `You are running a scheduled automation named "${auto.name}".`,
            "Follow the instructions. Be concise. If host shell is needed, reply with HOST_CMD lines.",
            "",
            auto.instructions,
          ].join("\n");
          // Prefer sending through agent chat path for host tools when connected
          if (get().oauth?.accessToken || get().apiKey) {
            await get().sendChat(
              `[Automation: ${auto.name}]\n${auto.instructions}`,
            );
            summary = "Ran via agent chat";
          } else {
            summary = "Not connected to Grok — automation recorded only";
            ok = false;
          }
          void prompt;
          void grokChat;
        } catch (e) {
          ok = false;
          summary = e instanceof Error ? e.message : "automation failed";
        }
        const { computeNextRun } = await import("./automation-schedule");
        set((s) => ({
          running: false,
          streamStatus: null,
          automations: s.automations.map((a) =>
            a.id === id
              ? {
                  ...a,
                  lastRun: Date.now(),
                  runCount: a.runCount + 1,
                  nextRun:
                    a.schedule === "once"
                      ? undefined
                      : computeNextRun(
                          a.schedule,
                          a.time,
                          Date.now(),
                          Date.now(),
                          a.times,
                          a.heartbeatEveryMin,
                        ),
                  enabled: a.schedule === "once" ? false : a.enabled,
                }
              : a,
          ),
        }));
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
        for (const a of due.slice(0, 2)) {
          // small batch per tick; avoid pile-up
          if (get().running) break;
          await get().runAutomation(a.id);
        }
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
        const killId = activeHostJobId;
        activeHostJobId = null;
        if (killId) {
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
          const chat = s.chat.map((m) =>
            m.id === sid
              ? {
                  ...m,
                  streaming: false,
                  stopped: true,
                  content: m.content?.trim()
                    ? `${m.content}${m.content.endsWith("\n") ? "" : "\n"}\n_Stopped._`
                    : "_Stopped._",
                }
              : m,
          );
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
        const trimmed = text.trim();
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
                  content: [
                    "**Slash commands**",
                    "",
                    "- `/help` — this list",
                    "- `/new` — new chat",
                    "- `/clear` — clear current chat",
                    "- `/imagine [prompt]` — open Imagine",
                    "- `/export` — download chat as Markdown",
                    "- `/mode auto|fast|expert|heavy|build`",
                    "- `/memory [note]` — append to MEMORY.md (+ legacy notes)",
                    "- `/memory show|user|today` — view file memory",
                    "- `/context` — show context budget / layers",
                    "- `/compact` — summarize older turns (frees API window)",
                    "- `/tools on|off` — host + connector tools",
                  ].join("\n"),
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
          if (cmd === "mode" && arg) {
            const id = arg.toLowerCase() as import("./types").GrokModeId;
            if (["auto", "fast", "expert", "heavy", "build"].includes(id)) {
              get().setMode(id);
              return;
            }
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

        void import("./persistent-storage").then(({ setPersistPaused }) => setPersistPaused(true));
        set((s) => {
          const chat = [...s.chat, userMsg, botPlaceholder];
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
        const routeCtx = {
          historyTurns: recentChat.length,
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
          lastRouteTier: lastAsst?.routeTier,
          lastRoutedMode:
            lastAsst?.mode === "fast" ||
            lastAsst?.mode === "expert" ||
            lastAsst?.mode === "heavy" ||
            lastAsst?.mode === "build"
              ? lastAsst.mode
              : lastAsst?.routeTier === "fast"
                ? ("fast" as const)
                : lastAsst?.routeTier === "build"
                  ? ("build" as const)
                  : lastAsst?.routeTier === "deep"
                    ? ("heavy" as const)
                    : lastAsst?.routeTier === "think"
                      ? ("expert" as const)
                      : undefined,
        };
        const auto = autoRouteFor(trimmed, catalog, routeCtx);
        if (mode === "auto" && auto.openImagine) {
          set((s) => ({
            chat: s.chat.filter((m) => m.id !== botId && m.id !== userMsg.id),
            running: false,
            streamStatus: null,
            streamingMessageId: null,
            nav: "imagine",
            imaginePrompt: trimmed,
          }));
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
                    : routed === "build"
                      ? ("build" as const)
                      : routed === "heavy"
                        ? ("deep" as const)
                        : ("think" as const),
                routeReason: `Manual ${m.label} mode`,
                routeModel: modelIdForMode(mode, trimmed, catalog, routeCtx),
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
        get().setAgentStatus(
          routed === "build" ? "builder" : routed === "heavy" ? "research" : "primary",
          "working",
          1,
        );
        if (routed === "heavy") {
          get().setAgentStatus("ops", "working", 1);
          get().setAgentStatus("builder", "working", 1);
        }

        const patchBot = (content: string, extra?: Partial<ChatMessage>) => {
          if (gen !== chatGeneration) return;
          set((s) => ({
            chat: s.chat.map((row) =>
              row.id === botId ? { ...row, content, ...extra } : row,
            ),
          }));
        };

        const isLocalSlash =
          trimmed.startsWith("/morning") ||
          trimmed.startsWith("/standup") ||
          trimmed.startsWith("/docs") ||
          trimmed.startsWith("/prints");

        let usedLive = false;
        let finalAnswer = "";
        let aborted = false;
        // Host helpers always available for final sanitization
        const {
          extractHostCommands,
          stripHostCommands,
          inferHostCommandsFromUser,
          looksLikeDeferredHostWork,
          userWantsHostInvestigation,
        } = await import("./grok");
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
          if (isLocalSlash) {
            set({ streamStatus: "Running skill…" });
            await wait(280);
            if (abort.signal.aborted || gen !== chatGeneration) {
              aborted = true;
            } else {
              bill = get().recordUsage("message", routed);
              finalAnswer = replyFor(trimmed, get(), routed);
              patchBot(finalAnswer, { streaming: false });
            }
          } else {
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
              openClawBundle: get().openClawWorkspace?.contextBundle,
              connectorBlock: (await import("./grok")).connectorContextBlock(
                get().connectors,
              ),
              capabilityBlock: undefined, // filled below after we know tool flags
              trimTools: true,
            });
            const history: Array<{ role: "user" | "assistant"; content: string }> =
              ctxBuilt.messages;

const modelId = modelIdForMode(mode, trimmed, catalog, routeCtx);
            // Hold Adaptive decision so it feels intentional (not a flash)
            if (mode === "auto") {
              set({
                streamStatus: `Adaptive → ${auto.tierLabel} · ${auto.reasonDetail}`,
              });
              await wait(280);
              if (abort.signal.aborted || gen !== chatGeneration) {
                aborted = true;
              }
            }
            let rounds = 0;
            const maxRounds = 8;
            let hostNudges = 0;
            let accumulated = "";

            // Build workspace context once per user turn (budgeted pins + capabilities)
            const stTurn = get();
            const freeTier =
              stTurn.usage.plan === "free" ||
              (!stTurn.oauth?.accessToken && !stTurn.apiKey);
            const capabilityBlock = [
              "## GrokHub session capabilities",
              "- Context manager: budgeted history + optional thread summary (see /context).",
              "- File memory: USER.md, MEMORY.md, daily notes (see /memory show).",
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
                : "- Connector tools: use only LIVE tools via CONNECTOR_CMD.",
            ].join("\n");
            const turnWorkspaceContext =
              [
                ctxBuilt.workspaceContext,
                capabilityBlock,
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
                  allowWebsiteFallback: stNow.preferFreeGrok !== false,
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

              if (result.tokens) set({ oauth: result.tokens });
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
                        ? "Free Grok · website session"
                        : (result as { fallbackFrom?: string }).fallbackFrom
                          ? `Free Grok fallback · ${(result as { model?: string }).model || "mini"}`
                          : "Free Grok · free-tier models",
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
                const full = stripAssistantChrome(result.content || roundText);
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
                if (
                  !cmds.length &&
                  !connCmds.length &&
                  get().agentPrefs.hostToolsEnabled &&
                  hostNudges < 2 &&
                  (looksLikeDeferredHostWork(full) ||
                    (rounds === 1 && userWantsHostInvestigation(trimmed)))
                ) {
                  hostNudges += 1;
                  const inferred = inferHostCommandsFromUser(trimmed);
                  if (inferred.length) {
                    cmds = inferred;
                    set({ streamStatus: "Starting host investigation…" });
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
                        "SYSTEM: You described planned host work but did not emit HOST_CMD.",
                        "Do not ask permission. Immediately output one or more HOST_CMD lines",
                        "for safe read-only diagnostics (ps, ls, uname, find -maxdepth, journalctl --user -n).",
                        "No preamble-only replies.",
                      ].join(" "),
                    });
                    set({ streamStatus: "Nudging agent to use host tools…" });
                    patchBot(
                      (visible || full) + "\n\n_Switching to host tools…_",
                      { streaming: true },
                    );
                    continue;
                  }
                }
                if (!cmds.length && !connCmds.length) {
                  finalAnswer = visible || full;
                  break;
                }

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
                if (
                  needsHostConfirm(cmds.slice(0, 5), {
                    confirmAll: Boolean(desk.confirmHostCommands) && !desk.confirmDestructiveOnly,
                    confirmDestructive: Boolean(desk.confirmHostCommands),
                  })
                ) {
                  const allowed = await requestHostConfirm(
                    set,
                    cmds.slice(0, 5),
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
                for (let hi = 0; hi < hostCmdList.length; hi++) {
                  const rawCmd = hostCmdList[hi]!;
                  if (abort.signal.aborted || gen !== chatGeneration) {
                    aborted = true;
                    break;
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
                  const stepIdx = hi + 1;
                  const stepTotal = hostCmdList.length;
                  let elapsedSec = 0;
                  const paintHost = () => {
                    set({
                      streamStatus: `Host: ${rawCmd.slice(0, 56)}… (${elapsedSec}s)`,
                    });
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
                  };
                  paintHost();
                  const hostTick = setInterval(() => {
                    if (gen !== chatGeneration || abort.signal.aborted) return;
                    elapsedSec += 1;
                    paintHost();
                  }, 1000);
                  try {
                    // Long scans: do not abort the whole agent turn on timeout —
                    // hostExec returns ok:false with stderr; we feed that back and continue.
                    const jobId = `host-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
                    activeHostJobId = jobId;
                    let r;
                    try {
                      r = await hostExec(cmd, undefined, timeoutMs, { jobId });
                    } finally {
                      clearInterval(hostTick);
                      if (activeHostJobId === jobId) activeHostJobId = null;
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
                    outputs.push(out);
                    get().pushActivity({
                      kind: "desktop",
                      title: r.ok ? "Host ok" : /timed out/i.test(r.stderr || "") ? "Host timeout" : "Host failed",
                      detail: rawCmd.slice(0, 120),
                      status: r.ok ? "success" : "failed",
                    });
                  } catch (e) {
                    // Soft-fail: keep the tool loop alive so the model can summarize
                    outputs.push(
                      clipHostOutput(
                        `$ ${rawCmd}\n[host error] ${e instanceof Error ? e.message : "failed"}\n(continuing agent turn)`,
                      ),
                    );
                    get().pushActivity({
                      kind: "desktop",
                      title: "Host error",
                      detail: e instanceof Error ? e.message : "failed",
                      status: "failed",
                    });
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
          endChatTurnPersist();
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
        }

        if (activeChatAbort === abort) activeChatAbort = null;
        endChatTurnPersist();
        get().setAgentStatus("primary", "idle", 0);
        get().setAgentStatus("builder", "idle", 0);
        get().setAgentStatus("research", "idle", 0);
        get().setAgentStatus("ops", "idle", 0);
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
            if (live.tokens) set({ oauth: live.tokens });
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

      tickHeartbeat: () => {
        set((s) => ({
          heartbeatAt: Date.now(),
          usage: ensurePeriod(s.usage),
        }));
        // Heartbeat-driven automations
        void get().tickAutomations({ heartbeatOnly: true });
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
        usage: s.usage,
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
        preferFreeGrok: s.preferFreeGrok !== false,
        // nav not forced — restore last view except desktop
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
        s.pendingHostConfirm = null;
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
            if (active.mode && ["auto", "fast", "expert", "heavy", "build"].includes(String(active.mode))) {
              // Do not force routed modes onto global selector on boot —
              // prefer the persisted global mode (partialize). Only fill if missing.
              if (!s.mode) s.mode = active.mode;
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
        const desk = s.desktop as Record<string, unknown> | undefined;
        if (desk) {
          if (desk.confirmHostCommands === undefined) desk.confirmHostCommands = true;
          if (desk.confirmDestructiveOnly === undefined) desk.confirmDestructiveOnly = true;
          if (desk.selfModifyEnabled === undefined) desk.selfModifyEnabled = false;
        if (!s.setupSyncMeta) s.setupSyncMeta = { autoPullOnLogin: true, autoPushOnChange: false };
        if (s.preferFreeGrok === undefined) s.preferFreeGrok = true;
        if (s.uiTheme !== "dark" && s.uiTheme !== "light" && s.uiTheme !== "system") s.uiTheme = "dark";
        if (s.toolsNavCollapsed === undefined) s.toolsNavCollapsed = false;
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
            };
          });
        }
          s.desktop = desk;
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

function endChatTurnPersist() {
  void import("./persistent-storage").then(({ setPersistPaused }) => setPersistPaused(false));
  activeHostJobId = null;
}

/** Active chat stream abort (module-level so Stop works across re-renders) */
let activeChatAbort: AbortController | null = null;
let chatGeneration = 0;
