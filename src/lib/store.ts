import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { persistentStorage } from "./persistent-storage";
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
  emptyQuickAssistMemory,
  normalizeMemory,
  rememberChipClick,
  rememberTypedPrompt,
  type QuickAssistMemory,
} from "./quick-assist-memory";
import type { QuickChip } from "./quick-assistant";

/** Waits for user approval of host commands (agent tool loop). */
let hostConfirmWaiter: ((allow: boolean) => void) | null = null;

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
  };
  /** Host commands awaiting user approval */
  pendingHostConfirm: {
    cmds: string[];
    risks: string[];
    botId: string;
  } | null;
  /** Adaptive quick-assist chip habits */
  quickAssistMemory: QuickAssistMemory;
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
  tickAutomations: () => Promise<void>;
  hydrateSecrets: () => Promise<void>;
  recordQuickAssistChip: (chip: QuickChip) => void;
  recordQuickAssistTyped: (text: string) => void;
  clearQuickAssistMemory: () => void;
  syncWebsiteConnectors: () => Promise<{ ok: boolean; detail: string; count: number }>;
  setApiKey: (key: string) => void;
  setGithubToken: (token: string) => void;
  startGrokOAuth: () => Promise<void>;
  pollGrokOAuth: () => Promise<"pending" | "ready" | "failed">;
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
  setPlan: (plan: SubscriptionPlanId) => void;

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
      "- Auto — Chooses Fast or Expert",
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
  const first = messages.find((m) => m.role === "user");
  if (!first) return "New chat";
  const t = first.content.replace(/\s+/g, " ").trim();
  return t.length > 48 ? t.slice(0, 48) + "…" : t || "New chat";
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
      },
      usage: createUsage("pro"),
      heartbeatAt: boot.heartbeatAt,
      running: false,
      streamStatus: null,
      streamingMessageId: null,
      pendingHostConfirm: null,
      quickAssistMemory: emptyQuickAssistMemory(),
      modelCatalog: emptyCatalog(),
      lastModelsFetchAt: 0,
      apiKey: "",
      githubToken: "",
      oauth: null,
      ssoCookie: "",
      openClawWorkspace: null,
      oauthPending: null,
      grokConnected: null,
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
      setDesktop: (patch) => set((s) => ({ desktop: { ...s.desktop, ...patch } })),

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
        }));
      },

      recordQuickAssistTyped: (text) => {
        const trimmed = text.trim();
        if (!trimmed) return;
        set((s) => ({
          quickAssistMemory: rememberTypedPrompt(s.quickAssistMemory, trimmed),
        }));
      },

      clearQuickAssistMemory: () => {
        set({ quickAssistMemory: emptyQuickAssistMemory() });
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
        set({ ssoCookie: normalized });
        void import("./secrets-client").then((m) =>
          m.secretsSet("ssoCookie", normalized),
        );
        // Inject into Electron partition so session.fetch works
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
        const thread: ChatThread = {
          id: uid("thread"),
          title: "New chat",
          createdAt: now,
          updatedAt: now,
          messages: [
            {
              id: uid("msg"),
              role: "system",
              content: "New chat. Ask Grok anything — modes apply from the picker.",
              ts: now,
            },
          ],
        };
        set((s) => ({
          threads: [thread, ...s.threads],
          activeThreadId: thread.id,
          chat: thread.messages,
          nav: "chat",
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
            t.id === id ? { ...t, title: next, updatedAt: Date.now() } : t,
          ),
        }));
      },

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

        // Prefer live Grok website weekly pool (same data as Settings → Usage)
        try {
          const { fetchGrokWebsiteUsage } = await import("./grok-website-usage");
          let sso = st.ssoCookie?.trim() || "";
          // Electron: try to pull SSO from linked session if empty
          if (!sso && typeof window !== "undefined" && window.grokhubDesktop?.grok?.getWebsiteSso) {
            try {
              const r = await window.grokhubDesktop.grok.getWebsiteSso();
              if (r?.cookie) {
                sso = r.cookie;
                set({ ssoCookie: sso });
              }
            } catch {
              /* ignore */
            }
          }
          const web = await fetchGrokWebsiteUsage({
            ssoCookie: sso || null,
            bearer: null, // website pool needs SSO; management keys are separate
          });
          if (web.ok) {
            const planMap =
              web.planId === "heavy" || web.planId === "pro"
                ? ("pro" as const)
                : web.planId === "free"
                  ? ("free" as const)
                  : ("super" as const);
            const unitCap = PLAN_LIMITS[planMap].units;
            usage = {
              ...usage,
              plan: planMap,
              periodStart: web.periodStart || usage.periodStart,
              periodEnd: web.periodEnd || usage.periodEnd,
              usedUnits: Math.round((web.creditUsagePercent / 100) * unitCap * 100) / 100,
              source: "website",
              lastPolledAt: Date.now(),
              website: {
                planLabel: web.planLabel,
                creditUsagePercent: web.creditUsagePercent,
                periodType: web.periodType,
                periodStart: web.periodStart,
                periodEnd: web.periodEnd,
                productUsage: web.productUsage,
                prepaidBalanceCents: web.prepaidBalanceCents,
                onDemandCapCents: web.onDemandCapCents,
                onDemandUsedCents: web.onDemandUsedCents,
                error: null,
              },
            };
            set({ usage });
            return;
          } else if (web.error) {
            usage = {
              ...usage,
              website: {
                planLabel: usage.website?.planLabel || "—",
                creditUsagePercent: usage.website?.creditUsagePercent ?? 0,
                periodType: usage.website?.periodType || "unknown",
                periodStart: usage.website?.periodStart ?? null,
                periodEnd: usage.website?.periodEnd ?? null,
                productUsage: usage.website?.productUsage || [],
                prepaidBalanceCents: usage.website?.prepaidBalanceCents ?? 0,
                onDemandCapCents: usage.website?.onDemandCapCents ?? 0,
                onDemandUsedCents: usage.website?.onDemandUsedCents ?? 0,
                error: web.error,
              },
            };
          }
        } catch {
          /* fall through to local */
        }

        usage = { ...usage, lastPolledAt: Date.now() };
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
          automations: s.automations.map((a) =>
            a.id === id ? { ...a, enabled: !a.enabled } : a,
          ),
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
                      : computeNextRun(a.schedule, a.time, Date.now(), Date.now()),
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

      tickAutomations: async () => {
        const { dueAutomations, ensureAutomationSchedule } = await import(
          "./automation-schedule"
        );
        const now = Date.now();
        set((s) => ({
          automations: s.automations.map((a) => ensureAutomationSchedule(a, now)),
        }));
        const due = dueAutomations(get().automations, now);
        for (const a of due.slice(0, 1)) {
          // one per tick to avoid pile-up
          await get().runAutomation(a.id);
        }
      },

      addAutomation: (input) => {
        const auto: Automation = {
          id: uid("auto"),
          name: input.name,
          instructions: input.instructions,
          schedule: input.schedule,
          time: input.time,
          enabled: true,
          connectorIds: get()
            .connectors.filter((c) => c.status === "connected")
            .slice(0, 2)
            .map((c) => c.id),
          skillIds: [],
          runCount: 0,
          nextRun: computeNextRun(input.schedule, input.time, Date.now()),
        };
        set((s) => ({ automations: [auto, ...s.automations] }));
        get().pushActivity({
          kind: "automation",
          title: `Created automation ${auto.name}`,
          detail: `${auto.schedule} @ ${auto.time}`,
          status: "success",
        });
      },

      stopChat: () => {
        const gen = ++chatGeneration;
        try {
          activeChatAbort?.abort();
        } catch {
          /* ignore */
        }
        activeChatAbort = null;
        set((s) => {
          const sid = s.streamingMessageId;
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
          return {
            chat,
            running: false,
            streamStatus: null,
            streamingMessageId: null,
          };
        });
        get().setAgentStatus("primary", "idle", 0);
        get().setAgentStatus("builder", "idle", 0);
        get().setAgentStatus("research", "idle", 0);
        get().setAgentStatus("ops", "idle", 0);
        get().pushActivity({
          kind: "chat",
          title: "Stopped",
          detail: "User interrupted the agent",
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

        const mode = get().mode;
        const catalog = get().modelCatalog || emptyCatalog();
        const auto = autoRouteFor(trimmed, catalog);
        if (mode === "auto" && auto.openImagine) {
          set({ nav: "imagine", imaginePrompt: trimmed });
          return;
        }
        const routed = resolveModeWithCatalog(mode, trimmed, catalog);
        const m = getMode(routed);
        // Soft quota check (real token units settled after live reply)
        {
          const u = ensurePeriod(get().usage);
          const est = costFor("message", routed);
          if (u.usedUnits + est > PLAN_LIMITS[u.plan].units * 1.02) {
            set((s) => ({
              chat: [
                ...s.chat,
                {
                  id: uid("msg"),
                  role: "user",
                  content: trimmed,
                  ts: Date.now(),
                  mode,
                },
                {
                  id: uid("msg"),
                  role: "system",
                  content: `Quota exceeded on ${PLAN_LIMITS[u.plan].label}. Wait for period reset or switch plan in Settings.`,
                  ts: Date.now(),
                },
              ],
            }));
            return;
          }
        }
        let bill = { ok: true, cost: costFor("message", routed) };

        const userMsg: ChatMessage = {
          id: uid("msg"),
          role: "user",
          content: trimmed,
          ts: Date.now(),
          mode,
        };
        const botId = uid("msg");
        const botPlaceholder: ChatMessage = {
          id: botId,
          role: "assistant",
          content: "",
          ts: Date.now(),
          mode: routed,
          streaming: true,
        };

        // Abort any previous stream
        try {
          activeChatAbort?.abort();
        } catch {
          /* ignore */
        }
        const abort = new AbortController();
        activeChatAbort = abort;
        const gen = ++chatGeneration;

        set((s) => ({
          chat: [...s.chat, userMsg, botPlaceholder],
          running: true,
          streamStatus:
            mode === "auto"
              ? `Auto → ${auto.reason}`
              : `Thinking · ${m.label}…`,
          streamingMessageId: botId,
        }));

        if (get().agents.length === 0) {
          await get().syncFromGrok();
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
            const history: Array<{ role: "user" | "assistant"; content: string }> = get()
              .chat.filter((c) => c.role === "user" || c.role === "assistant")
              .filter((c) => c.id !== botId)
              .slice(-16)
              .map((c) => ({
                role: c.role as "user" | "assistant",
                content:
                  c.role === "assistant" ? stripAssistantChrome(c.content) : c.content,
              }))
              .filter((c) => c.content.trim().length > 0);
            if (!history.length || history[history.length - 1]?.content !== trimmed) {
              history.push({ role: "user", content: trimmed });
            }

            const modelId = modelIdForMode(mode, trimmed, catalog);
            // Surface Auto routing decision while streaming
            if (mode === "auto") {
              set({ streamStatus: `Auto → ${auto.reason}` });
            }
            let rounds = 0;
            const maxRounds = 4;
            let accumulated = "";

            while (rounds < maxRounds) {
              rounds += 1;
              if (abort.signal.aborted || gen !== chatGeneration) {
                aborted = true;
                break;
              }
              set({
                streamStatus:
                  rounds === 1 ? "Streaming…" : `Host tool round ${rounds}…`,
              });
              let roundText = "";
              const oc = get().openClawWorkspace;
              const result = await grokChatStream(
                {
                  messages: history,
                  mode: routed,
                  model: modelId,
                  apiKey: get().apiKey || undefined,
                  accessToken: get().oauth?.accessToken,
                  tokens: get().oauth,
                  workspaceContext: [
                    oc?.contextBundle || "",
                    (await import("./grok")).connectorContextBlock(get().connectors),
                  ]
                    .filter(Boolean)
                    .join("\n")
                    .slice(0, 28_000) || undefined,
                },
                {
                  signal: abort.signal,
                  onStatus: (st) => {
                    if (gen !== chatGeneration) return;
                    set({
                      streamStatus:
                        st === "streaming"
                          ? "Streaming…"
                          : st === "fallback"
                            ? "Responding…"
                            : st === "connecting"
                              ? "Connecting…"
                              : st,
                    });
                  },
                  onDelta: (piece) => {
                    if (gen !== chatGeneration) return;
                    roundText += piece;
                    accumulated = roundText;
                    // Batch UI patches to animation frames (smoother streaming)
                    const scrub = (s: string) => scrubAssistant(s) || "…";
                    if (!(globalThis as unknown as { __ghRaf?: number }).__ghRaf) {
                      (globalThis as unknown as { __ghRaf?: number }).__ghRaf =
                        requestAnimationFrame(() => {
                          (globalThis as unknown as { __ghRaf?: number }).__ghRaf = 0;
                          if (gen !== chatGeneration) return;
                          patchBot(scrub(roundText), { streaming: true });
                        });
                    }
                    void scrub;
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
                // First round: if user asked about local files and model forgot HOST_CMD, infer
                if (!cmds.length && rounds === 1) {
                  cmds = inferHostCommandsFromUser(trimmed);
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
                    set({ streamStatus: `Connector: ${cc.connectorId} ${cc.tool}…` });
                    const row = get().connectors.find((c) => c.id === cc.connectorId);
                    patchBot(
                      `${visible || "Using connector…"}\n\n_Running_\n\`CONNECTOR_CMD: ${cc.connectorId} ${cc.tool}\``,
                      { streaming: true },
                    );
                    try {
                      const r = await runConnectorTool({
                        connectorId: cc.connectorId,
                        tool: cc.tool,
                        args: cc.args,
                        githubToken: get().githubToken,
                        websiteConnected: row?.status === "connected",
                        accountLabel: row?.accountLabel,
                      });
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
                  patchBot(
                    [
                      visible || "Checked connectors.",
                      "",
                      "```",
                      outputs.join("\n\n"),
                      "```",
                      "",
                      "_Summarizing…_",
                    ].join("\n"),
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
                const riskList = cmds.slice(0, 3).map((c) => riskLabel(classifyHostCommand(c)));
                const desk = get().desktop;
                if (
                  needsHostConfirm(cmds.slice(0, 3), {
                    confirmAll: Boolean(desk.confirmHostCommands) && !desk.confirmDestructiveOnly,
                    confirmDestructive: Boolean(desk.confirmHostCommands),
                  })
                ) {
                  const allowed = await requestHostConfirm(
                    set,
                    cmds.slice(0, 3),
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
                const outputs: string[] = [];
                for (const cmd of cmds.slice(0, 3)) {
                  get().pushActivity({
                    kind: "desktop",
                    title: "Host command",
                    detail: cmd.slice(0, 160),
                    status: "running",
                  });
                  if (abort.signal.aborted || gen !== chatGeneration) {
                    aborted = true;
                    break;
                  }
                  set({ streamStatus: `Host: ${cmd.slice(0, 56)}…` });
                  patchBot(
                    `${visible || "Checking your machine…"}\n\n_Running_\n\`$ ${cmd}\``,
                    { streaming: true },
                  );
                  try {
                    const r = await hostExec(cmd, undefined, 45_000);
                    outputs.push(
                      [
                        `$ ${cmd}`,
                        `exit ${r.code ?? "?"} · ${r.ms}ms · ${r.cwd}`,
                        r.stdout || "(no stdout)",
                        r.stderr ? `[stderr]\n${r.stderr}` : "",
                      ]
                        .filter(Boolean)
                        .join("\n"),
                    );
                  } catch (e) {
                    outputs.push(
                      `$ ${cmd}\n[host error] ${e instanceof Error ? e.message : "failed"}`,
                    );
                  }
                }
                if (aborted) break;

                const toolBlock = [
                  "HOST_RESULT (authoritative — use this, do not invent files):",
                  outputs.join("\n\n---\n\n"),
                  "",
                  "Summarize these results for the user in plain language. Do not output HOST_CMD again unless you need another command.",
                ].join("\n");

                history.push({ role: "assistant", content: full });
                history.push({ role: "user", content: toolBlock });
                // Show intermediate host output (sanitized) while model continues
                const mid = [
                  visible || "Checked your machine.",
                  "",
                  "```",
                  outputs.join("\n\n"),
                  "```",
                  "",
                  "_Summarizing…_",
                ].join("\n");
                patchBot(mid, { streaming: true });
                accumulated = mid;
                // continue loop for next model turn
                continue;
              }

              // Failed live call
              const hasOauth = Boolean(get().oauth?.accessToken);
              const err = result.error || "Unknown error";
              finalAnswer = [
                "**Could not reach Grok.**",
                "",
                err,
                "",
                hasOauth
                  ? "Your OAuth session is saved. Try: Settings → Disconnect → Connect with Grok OAuth again, or paste an xAI API key as fallback."
                  : "Fix: Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
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
            finalAnswer = [
              `Grok connection error: ${msg}`,
              "",
              replyFor(trimmed, get(), routed),
            ].join("\n");
            set({ grokConnected: false, grokStatusDetail: msg });
            patchBot(finalAnswer, { streaming: false });
          }
        }

        if (gen !== chatGeneration) return;

        if (aborted) {
          // stopChat already cleaned UI
          if (get().running) {
            set((s) => ({
              running: false,
              streamStatus: null,
              streamingMessageId: null,
              chat: s.chat.map((row) =>
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
              ),
            }));
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
                ? {
                    ...th,
                    messages: chat,
                    updatedAt: Date.now(),
                    title: titleFromMessages(chat),
                    mode: routed,
                  }
                : th,
            );
            return {
              chat,
              threads,
              running: false,
              streamStatus: null,
              streamingMessageId: null,
            };
          });
          get().pushActivity({
            kind: "chat",
            title: usedLive ? `Grok · ${m.label}` : `Agent reply · ${m.label}`,
            detail: `${trimmed.slice(0, 80)} · ${bill.cost}u`,
            status: usedLive ? "success" : "failed",
          });
        }

        if (activeChatAbort === abort) activeChatAbort = null;
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
              : "Imagine ready (local preview)",
          detail:
            source === "xai"
              ? `${p.slice(0, 80)} · ${model || "xAI"} · ${aspect}/${quality}`
              : `${p.slice(0, 80)}${err ? ` · live failed: ${err}` : " · offline SVG"}`,
          status: "success",
        });
      },

      pushActivity: (item) => {
        const row: ActivityItem = {
          id: uid("act"),
          ts: item.ts ?? Date.now(),
          kind: item.kind,
          title: item.title,
          detail: item.detail,
          status: item.status,
        };
        set((s) => ({ activity: [row, ...s.activity].slice(0, 80) }));
      },

      tickHeartbeat: () =>
        set((s) => ({
          heartbeatAt: Date.now(),
          usage: ensurePeriod(s.usage),
        })),

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
        agents: s.agents,
        mode: s.mode,
        desktop: s.desktop,
        usage: s.usage,
        // Persist imagine job metadata; drop huge payloads (userData has more room than localStorage but still cap)
        imagineJobs: s.imagineJobs.slice(0, 16).map((j) => {
          const { imageDataUrl, videoDataUrl, referenceDataUrl, ...rest } = j;
          // keep tiny svg previews only
          const keepImg =
            imageDataUrl &&
            imageDataUrl.startsWith("data:image/svg") &&
            imageDataUrl.length < 80_000
              ? imageDataUrl
              : undefined;
          return { ...rest, imageDataUrl: keepImg };
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
        chat: s.chat.slice(-200),
        activity: s.activity.slice(0, 100),
        quickAssistMemory: s.quickAssistMemory,
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
        // Host safety defaults for upgrades
        const desk = s.desktop as Record<string, unknown> | undefined;
        if (desk) {
          if (desk.confirmHostCommands === undefined) desk.confirmHostCommands = true;
          if (desk.confirmDestructiveOnly === undefined) desk.confirmDestructiveOnly = true;
          if (desk.selfModifyEnabled === undefined) desk.selfModifyEnabled = false;
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
    },
  ),
);

function wait(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}

/** Active chat stream abort (module-level so Stop works across re-renders) */
let activeChatAbort: AbortController | null = null;
let chatGeneration = 0;
