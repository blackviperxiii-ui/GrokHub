import { create } from "zustand";
import { persist } from "zustand/middleware";
import { renderImaginePreview } from "./imagine";
import { getMode, resolveMode, stripAssistantChrome, modelIdForMode } from "./modes";
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
  PLAN_LIMITS,
  usagePercent,
} from "./usage";
import { uid } from "./utils";

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
  desktop: {
    startMinimized: boolean;
    launchOnLogin: boolean;
    wayland: boolean;
    tray: boolean;
  };
  usage: UsageSnapshot;
  heartbeatAt: number;
  running: boolean;
  /** xAI API key (local only; never sent to third parties except api.x.ai) */
  apiKey: string;
  /** Optional GitHub token for private-repo updates */
  githubToken: string;
  /** xAI Grok OAuth tokens (SuperGrok / X Premium device-code) */
  oauth: import("./xai-oauth").XaiOAuthTokens | null;
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
  setApiKey: (key: string) => void;
  setGithubToken: (token: string) => void;
  startGrokOAuth: () => Promise<void>;
  pollGrokOAuth: () => Promise<"pending" | "ready" | "failed">;
  clearGrokOAuth: () => void;
  probeGrok: () => Promise<boolean>;
  syncFromGrok: (opts?: { displayName?: string | null; email?: string | null; imageUrl?: string | null }) => Promise<void>;
  newThread: () => void;
  selectThread: (id: string) => void;
  deleteThread: (id: string) => void;
  setPlan: (plan: SubscriptionPlanId) => void;

  recordUsage: (bucket: UsageBucket, mode?: GrokModeId) => { ok: boolean; cost: number };
  resetUsagePeriod: () => void;
  toggleConnector: (id: string) => void;
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
  setImaginePrompt: (v: string) => void;
  setImagineAspect: (v: ImagineAspect) => void;
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
      imagineAspect: "1:1",
      desktop: {
        startMinimized: false,
        launchOnLogin: false,
        wayland: true,
        tray: true,
      },
      usage: createUsage("pro"),
      heartbeatAt: boot.heartbeatAt,
      running: false,
      apiKey: "",
      githubToken: "",
      oauth: null,
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
      setApiKey: (key) => set({ apiKey: key, grokConnected: null }),
      setGithubToken: (token) => set({ githubToken: token }),

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
            if (Array.isArray(data.models)) models.push(...data.models);
          }
        } catch {
          /* optional when offline */
        }
        const now = Date.now();
        set((st) => ({
          profile: {
            displayName: opts?.displayName ?? st.profile.displayName,
            email: opts?.email ?? st.profile.email,
            imageUrl: opts?.imageUrl ?? st.profile.imageUrl,
            models: models.length ? models : st.profile.models,
            connectedAt: st.profile.connectedAt ?? (st.grokConnected ? now : null),
          },
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
            detail: opts?.displayName || opts?.email || `${models.length} models`,
            status: "success",
          });
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

      setPlan: (plan) => {
        const prev = get().usage;
        const next = ensurePeriod({
          ...createUsage(plan),
          usedUnits: 0,
          messages: 0,
          imagine: 0,
          automations: 0,
          host: 0,
          byMode: { auto: 0, fast: 0, expert: 0, heavy: 0, build: 0 },
        });
        const seeded = createUsage(plan);
        set({
          usage: {
            ...seeded,
            periodStart: next.periodStart,
            periodEnd: next.periodEnd,
            plan,
          },
        });
        get().pushActivity({
          kind: "usage",
          title: `Plan → ${PLAN_LIMITS[plan].label}`,
          detail: `Limit ${PLAN_LIMITS[plan].units} units / month (was ${PLAN_LIMITS[prev.plan].label})`,
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

      resetUsagePeriod: () => {
        const plan = get().usage.plan;
        const u = createUsage(plan);
        set({
          usage: {
            ...u,
            usedUnits: 0,
            messages: 0,
            imagine: 0,
            automations: 0,
            host: 0,
            byMode: { auto: 0, fast: 0, expert: 0, heavy: 0, build: 0 },
          },
        });
        get().pushActivity({
          kind: "usage",
          title: "Billing period reset",
          detail: `${PLAN_LIMITS[plan].label} counters cleared`,
          status: "success",
        });
      },

      toggleConnector: (id) => {
        set((s) => ({
          connectors: s.connectors.map((c) => {
            if (c.id !== id) return c;
            return {
              ...c,
              status: c.status === "connected" ? "disconnected" : "connected",
              lastUsed: c.status === "connected" ? c.lastUsed : Date.now(),
            } as Connector;
          }),
        }));
        const c = get().connectors.find((x) => x.id === id);
        if (c) {
          get().pushActivity({
            kind: "connector",
            title: c.status === "connected" ? `Connected ${c.name}` : `Disconnected ${c.name}`,
            detail:
              c.status === "connected"
                ? `Tools available: ${c.tools.join(", ")}`
                : "OAuth session cleared (demo)",
            status: "success",
          });
        }
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
        const m = getMode(resolveMode(get().mode, auto.instructions));
        const bill = get().recordUsage("automation");
        if (!bill.ok) return;
        set({ running: true });
        get().setAgentStatus("ops", "working", 1);
        get().pushActivity({
          kind: "automation",
          title: `Automation started: ${auto.name}`,
          detail: `${auto.instructions.slice(0, 100)} · ${m.label} · ${bill.cost}u`,
          status: "running",
        });
        await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
        set((s) => ({
          running: false,
          automations: s.automations.map((a) =>
            a.id === id
              ? { ...a, lastRun: Date.now(), runCount: a.runCount + 1 }
              : a,
          ),
        }));
        get().setAgentStatus("ops", "idle", 0);
        get().pushActivity({
          kind: "automation",
          title: `Automation completed: ${auto.name}`,
          detail: `Used connectors: ${auto.connectorIds.join(", ") || "none"}`,
          status: "success",
        });
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
          nextRun: Date.now() + 1000 * 60 * 60 * 24,
        };
        set((s) => ({ automations: [auto, ...s.automations] }));
        get().pushActivity({
          kind: "automation",
          title: `Created automation ${auto.name}`,
          detail: `${auto.schedule} @ ${auto.time}`,
          status: "success",
        });
      },

      sendChat: async (text) => {
        const trimmed = text.trim();
        if (!trimmed) return;
        const mode = get().mode;
        const routed = resolveMode(mode, trimmed);
        const m = getMode(routed);
        const bill = get().recordUsage("message", routed);
        if (!bill.ok) {
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
                content: `Quota exceeded on ${PLAN_LIMITS[get().usage.plan].label}. Wait for period reset or switch plan in Settings.`,
                ts: Date.now(),
              },
            ],
          }));
          return;
        }
        const userMsg: ChatMessage = {
          id: uid("msg"),
          role: "user",
          content: trimmed,
          ts: Date.now(),
          mode,
        };
        set((s) => ({ chat: [...s.chat, userMsg], running: true }));
        // Ensure agents exist before status updates
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

        // Local slash demos still work offline; everything else hits xAI Grok
        const isLocalSlash =
          trimmed.startsWith("/morning") ||
          trimmed.startsWith("/standup") ||
          trimmed.startsWith("/docs") ||
          trimmed.startsWith("/prints");

        let answer: string;
        let usedLive = false;
        try {
          if (isLocalSlash) {
            await wait(280);
            answer = replyFor(trimmed, get(), routed);
          } else {
            const { grokChat } = await import("./grok-client");
            const history = get()
              .chat.filter((c) => c.role === "user" || c.role === "assistant")
              .slice(-16)
              .map((c) => ({
                role: c.role as "user" | "assistant",
                content:
                  c.role === "assistant"
                    ? stripAssistantChrome(c.content)
                    : c.content,
              }))
              .filter((c) => c.content.trim().length > 0);
            // ensure last user message is present
            if (!history.length || history[history.length - 1]?.content !== trimmed) {
              history.push({ role: "user", content: trimmed });
            }
            const modelId = modelIdForMode(mode, trimmed);
            const result = await grokChat({
              messages: history,
              mode: routed,
              model: modelId,
              apiKey: get().apiKey || undefined,
              accessToken: get().oauth?.accessToken,
              tokens: get().oauth,
            });
            if (result.tokens) {
              set({ oauth: result.tokens });
            }
            if (result.ok && result.content) {
              usedLive = true;
              // Clean answer only — mode is shown on the message badge, not in the body
              answer = stripAssistantChrome(result.content);
              set({
                grokConnected: true,
                grokStatusDetail: get().oauth
                  ? `Live · ${result.model || modelId}`
                  : `Live · ${result.model || modelId}`,
              });
            } else {
              const hasOauth = Boolean(get().oauth?.accessToken);
              const err = result.error || "Unknown error";
              answer = [
                "Could not reach Grok.",
                err,
                "",
                hasOauth
                  ? "Your OAuth session is saved. If this keeps failing: Settings → Disconnect → Connect with Grok OAuth again, or paste an xAI API key as fallback."
                  : "Fix: Settings → Connect with Grok OAuth (SuperGrok / X Premium) or paste an xAI API key.",
                "",
                replyFor(trimmed, get(), routed),
              ].join("\n");
              set({
                grokConnected: false,
                grokStatusDetail: hasOauth
                  ? `OAuth session · chat failed: ${err}`
                  : err,
              });
            }
          }
        } catch (e) {
          const msg = e instanceof Error ? e.message : "request failed";
          answer = [
            `Grok connection error: ${msg}`,
            "",
            replyFor(trimmed, get(), routed),
          ].join("\n");
          set({ grokConnected: false, grokStatusDetail: msg });
        }

        const bot: ChatMessage = {
          id: uid("msg"),
          role: "assistant",
          content: answer,
          ts: Date.now(),
          mode: routed,
        };
        set((s) => {
          const chat = [...s.chat, bot];
          const tid = s.activeThreadId;
          const threads = s.threads.map((t) =>
            t.id === tid
              ? {
                  ...t,
                  messages: chat,
                  updatedAt: Date.now(),
                  title: titleFromMessages(chat),
                  mode: routed,
                }
              : t,
          );
          return { chat, threads, running: false };
        });
        get().setAgentStatus("primary", "idle", 0);
        get().setAgentStatus("builder", "idle", 0);
        get().setAgentStatus("research", "idle", 0);
        get().setAgentStatus("ops", "idle", 0);
        get().pushActivity({
          kind: "chat",
          title: usedLive ? `Grok · ${m.label}` : `Agent reply · ${m.label}`,
          detail: `${trimmed.slice(0, 80)} · ${bill.cost}u`,
          status: usedLive ? "success" : "failed",
        });
      },

      setImaginePrompt: (v) => set({ imaginePrompt: v }),
      setImagineAspect: (v) => set({ imagineAspect: v }),

      runImagine: async (prompt) => {
        const p = (prompt ?? get().imaginePrompt).trim();
        if (!p) return;
        const bill = get().recordUsage("imagine");
        if (!bill.ok) return;
        const aspect = get().imagineAspect;
        const mode = get().mode;
        const id = uid("img");
        const job: ImagineJob = {
          id,
          prompt: p,
          aspect,
          ts: Date.now(),
          status: "rendering",
          mode,
        };
        set((s) => ({
          imagineJobs: [job, ...s.imagineJobs].slice(0, 24),
          running: true,
          imaginePrompt: p,
        }));
        get().pushActivity({
          kind: "imagine",
          title: "Imagine rendering",
          detail: `${p.slice(0, 100)} · ${bill.cost}u`,
          status: "running",
        });
        const m = getMode(resolveMode(mode, p));
        await wait(m.latencyMs[0] + Math.random() * (m.latencyMs[1] - m.latencyMs[0]));
        const imageDataUrl = renderImaginePreview(p, aspect);
        set((s) => ({
          running: false,
          imagineJobs: s.imagineJobs.map((j) =>
            j.id === id ? { ...j, status: "ready", imageDataUrl } : j,
          ),
        }));
        get().pushActivity({
          kind: "imagine",
          title: "Imagine ready",
          detail: p.slice(0, 120),
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
        const now = Date.now();
        const DAY = 86_400_000;
        const activity = get().activity;
        const chat = get().chat;
        const oldest = Math.min(
          ...activity.map((a) => a.ts),
          ...chat.map((m) => m.ts),
          now,
        );
        if (now - oldest < 2 * DAY) {
          set((s) => ({ heartbeatAt: now, usage: ensurePeriod(s.usage) }));
          return;
        }
        const fresh = createSeeds(now);
        set((s) => ({
          activity: fresh.activity,
          chat: fresh.chat,
          automations: fresh.automations.map((a) => {
            const prev = s.automations.find((x) => x.id === a.id);
            return prev
              ? {
                  ...a,
                  enabled: prev.enabled,
                  runCount: prev.runCount,
                  lastRun: prev.lastRun && now - prev.lastRun < 2 * DAY ? prev.lastRun : a.lastRun,
                }
              : a;
          }),
          connectors: s.connectors.map((c) => ({
            ...c,
            lastUsed:
              c.lastUsed && now - c.lastUsed < 2 * DAY
                ? c.lastUsed
                : fresh.connectors.find((x) => x.id === c.id)?.lastUsed,
          })),
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
          mode: "auto",
          heartbeatAt: fresh.heartbeatAt,
          running: false,
          nav: "chat",
          modeMenuOpen: false,
          usage: createUsage("pro"),
          grokConnected: null,
          grokStatusDetail: "Not connected — Connect with Grok OAuth in Settings",
          oauth: null,
          oauthPending: null,
        });
      },
    }),
    {
      name: "grokhub-clean-v2",
      partialize: (s) => ({
        connectors: s.connectors,
        skills: s.skills,
        automations: s.automations,
        threads: s.threads,
        activeThreadId: s.activeThreadId,
        agents: s.agents,
        mode: s.mode,
        desktop: s.desktop,
        usage: s.usage,
        imagineJobs: s.imagineJobs.slice(0, 8),
        imagineAspect: s.imagineAspect,
        apiKey: s.apiKey,
        githubToken: s.githubToken,
        oauth: s.oauth,
        profile: s.profile,
        chat: s.chat,
        activity: s.activity.slice(0, 40),
      }),
      skipHydration: true,
    },
  ),
);

function wait(ms: number) {
  return new Promise((r) => setTimeout(r, ms));
}
