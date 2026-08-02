import type { ComponentType, CSSProperties } from "react";
import { ListTodo,
  ClipboardList,
  ChevronDown,
  ChevronRight,
  Command,
  History,
  ImageIcon,
  Menu,
  MessageSquare,
  MessageSquarePlus,
  Minus,
  MoreHorizontal,
  Pencil,
  Pin,
  Sparkles,
  Search,
  Settings,
  Square,
  TimerReset,
  Trash2,
  Users,
  X,
  Download,
  Wand2,
} from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { NavId } from "@/lib/types";
import { cn } from "@/lib/utils";
import { APP_VERSION } from "@/lib/version";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { UserButton } from "@/lib/auth/gates";
import { ShortcutsDialog } from "./ShortcutsDialog";
import { UndoToast } from "./UndoToast";
import { CommandPalette } from "./CommandPalette";
import { GrokHubMark } from "./GrokLogo";
import { ModePicker } from "./ModePicker";
import { UsageMeterChip } from "./UsageMeter";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "./ui/tooltip";

const AgentsView = lazy(() =>
  import("./views/AgentsView").then((m) => ({ default: m.AgentsView })),
);
const AutomationsView = lazy(() =>
  import("./views/AutomationsView").then((m) => ({ default: m.AutomationsView })),
);
const ChatView = lazy(() =>
  import("./views/ChatView").then((m) => ({ default: m.ChatView })),
);
const CommandView = lazy(() =>
  import("./views/CommandView").then((m) => ({ default: m.CommandView })),
);
const DesktopHostView = lazy(() =>
  import("./views/DesktopHostView").then((m) => ({ default: m.DesktopHostView })),
);
const HistoryView = lazy(() =>
  import("./views/HistoryView").then((m) => ({ default: m.HistoryView })),
);
const ImagineView = lazy(() =>
  import("./views/ImagineView").then((m) => ({ default: m.ImagineView })),
);
const SettingsView = lazy(() =>
  import("./views/SettingsView").then((m) => ({ default: m.SettingsView })),
);
const SkillsView = lazy(() =>
  import("./views/SkillsView").then((m) => ({ default: m.SkillsView })),
);
const WorkboardView = lazy(() =>
  import("./views/WorkboardView").then((m) => ({ default: m.WorkboardView })),
);
const AgentQueueView = lazy(() =>
  import("./views/AgentQueueView").then((m) => ({ default: m.AgentQueueView })),
);



const NAV: { id: NavId; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "chat", label: "Agent", icon: MessageSquare },
  { id: "history", label: "History", icon: History },
  { id: "command", label: "Command", icon: Command },
  { id: "workboard", label: "Workboard", icon: ClipboardList },
  { id: "imagine", label: "Imagine", icon: ImageIcon },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "queue", label: "Queue", icon: ListTodo },
  { id: "automations", label: "Automations", icon: TimerReset },
  { id: "agents", label: "Roster", icon: Users },
  { id: "settings", label: "Settings", icon: Settings },
];

function RecentThreadRow({
  id,
  title,
  active,
  pinned,
  onSelect,
}: {
  id: string;
  title: string;
  active: boolean;
  pinned?: boolean;
  folder?: string | null;
  onSelect: () => void;
}) {
  const renameThread = useGrokHub((s) => s.renameThread);
  const deleteThread = useGrokHub((s) => s.deleteThread);
  const pinThread = useGrokHub((s) => s.pinThread);
  const autoRenameThread = useGrokHub((s) => s.autoRenameThread);
  const [renaming, setRenaming] = useState(false);
  const [naming, setNaming] = useState(false);
  const [draft, setDraft] = useState(title);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setDraft(title);
  }, [title]);

  useEffect(() => {
    if (renaming) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [renaming]);

  function commitRename() {
    const next = draft.trim();
    if (next) renameThread(id, next);
    else setDraft(title);
    setRenaming(false);
  }

  if (renaming) {
    return (
      <div className="mb-0.5 px-1 py-0.5">
        <input
          ref={inputRef}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitRename();
            }
            if (e.key === "Escape") {
              setDraft(title);
              setRenaming(false);
            }
          }}
          className="w-full rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-2 py-1 text-xs text-[var(--color-fg)] outline-none focus:ring-1 focus:ring-[var(--color-ring)]"
          aria-label="Rename chat"
        />
      </div>
    );
  }

  return (
    <div
      className={cn(
        "group relative mb-0.5 flex w-full items-center gap-0.5 rounded-[var(--radius-sm)]",
        active ? "nav-item-active" : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/50",
      )}
    >
      <button
        type="button"
        onClick={onSelect}
        className="min-w-0 flex-1 truncate px-2.5 py-1.5 text-left text-xs font-medium"
      >
        {pinned ? (
          <Pin className="mr-1 inline h-3 w-3 text-[var(--color-info)]" aria-hidden />
        ) : null}
        {title}
      </button>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <button
            type="button"
            className={cn(
              "rounded p-1 text-[var(--color-subtle)] transition-opacity hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]",
              "opacity-0 group-hover:opacity-100 focus:opacity-100 data-[state=open]:opacity-100",
            )}
            aria-label="Chat options"
            onClick={(e) => e.stopPropagation()}
          >
            <MoreHorizontal className="h-3.5 w-3.5" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="min-w-[140px]">
          <DropdownMenuItem
            onClick={() => {
              setDraft(title);
              setRenaming(true);
            }}
          >
            <Pencil className="h-3 w-3" />
            Rename
          </DropdownMenuItem>
          <DropdownMenuItem
            disabled={naming}
            onClick={() => {
              setNaming(true);
              void autoRenameThread(id).finally(() => setNaming(false));
            }}
          >
            <Wand2 className="h-3 w-3" />
            {naming ? "Naming…" : "Auto name"}
          </DropdownMenuItem>
          <DropdownMenuItem onClick={() => pinThread(id, !pinned)}>
            <Pin className="h-3 w-3" />
            {pinned ? "Unpin" : "Pin"}
          </DropdownMenuItem>
          <DropdownMenuSeparator />
          <DropdownMenuItem
            danger
            onClick={() => {
              if (window.confirm("Delete this chat? You can Undo for a few seconds.")) {
                deleteThread(id);
              }
            }}
          >
            <Trash2 className="h-3 w-3" />
            Delete
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

export function AppShell() {
  const nav = useGrokHub((s) => s.nav);
  const setNav = useGrokHub((s) => s.setNav);
  const running = useGrokHub((s) => s.running);
  const mode = useGrokHub((s) => s.mode);
  const tickHeartbeat = useGrokHub((s) => s.tickHeartbeat);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
  const apiKey = useGrokHub((s) => s.apiKey);
  const probeGrok = useGrokHub((s) => s.probeGrok);
  const syncFromGrok = useGrokHub((s) => s.syncFromGrok);
  const newThread = useGrokHub((s) => s.newThread);
  const threads = useGrokHub((s) => s.threads);
  const selectThread = useGrokHub((s) => s.selectThread);
  const activeThreadId = useGrokHub((s) => s.activeThreadId);
  const oauth = useGrokHub((s) => s.oauth);
  const profile = useGrokHub((s) => s.profile);
  const uiTheme = useGrokHub((s) => s.uiTheme);
  const toolsNavCollapsed = useGrokHub((s) => s.toolsNavCollapsed);
  const setToolsNavCollapsed = useGrokHub((s) => s.setToolsNavCollapsed);
  const updateBanner = useGrokHub((s) => s.updateBanner);
  const checkUpdateQuiet = useGrokHub((s) => s.checkUpdateQuiet);
  const setUpdateBanner = useGrokHub((s) => s.setUpdateBanner);
  const { user, isPending } = useCurrentUserState();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [isDesktop, setIsDesktop] = useState(false);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [shortcutsOpen, setShortcutsOpen] = useState(false);
  const [sidebarQ, setSidebarQ] = useState("");
  const modeMeta = getMode(mode);

  const activeThread = useMemo(
    () => threads.find((t) => t.id === activeThreadId) || null,
    [threads, activeThreadId],
  );

  const accountLabel =
    oauth?.name ||
    oauth?.email ||
    profile?.displayName ||
    profile?.email ||
    (user && !user.isDevFallback
      ? user.displayName || user.primaryEmail || null
      : null);

  const accountConnected = Boolean(
    oauth?.accessToken || (user && !user.isDevFallback) || grokConnected,
  );

  useEffect(() => {
    const root = document.documentElement;
    const apply = (t: "dark" | "light") => {
      root.dataset.theme = t;
      root.style.colorScheme = t;
    };
    if (uiTheme === "system") {
      const mq = window.matchMedia("(prefers-color-scheme: light)");
      const sync = () => apply(mq.matches ? "light" : "dark");
      sync();
      mq.addEventListener("change", sync);
      return () => mq.removeEventListener("change", sync);
    }
    apply(uiTheme === "light" ? "light" : "dark");
  }, [uiTheme]);

  useEffect(() => {
    const p = useGrokHub.persist.rehydrate();
    Promise.resolve(p).finally(() => {
      const restored = useGrokHub.getState().nav;
      const safeNav =
        !restored || restored === "connectors" || restored === "desktop"
          ? "chat"
          : restored;
      useGrokHub.setState({
        nav: safeNav,
        running: false,
        streamStatus: null,
        streamingMessageId: null,
        pendingHostConfirm: null,
      });
      try {
        void useGrokHub.getState().syncWebsiteConnectors();
      } catch {
        /* ignore */
      }
      void import("@/lib/imagine-media").then(async ({ rehydrateImagineJobs }) => {
        const jobs = useGrokHub.getState().imagineJobs || [];
        if (!jobs.length) return;
        try {
          const next = await rehydrateImagineJobs(jobs);
          useGrokHub.setState({ imagineJobs: next });
        } catch {
          /* ignore */
        }
      });
      const st = useGrokHub.getState();
      st.refreshStaleTimes();
      st.tickHeartbeat();
      void st.hydrateSecrets().then(() => {
        void useGrokHub.getState().probeGrok();
        void useGrokHub.getState().refreshUsage();
        // Immediately ensure OAuth is not near expiry after restore
        void useGrokHub.getState().refreshOAuthSession();
      });
      if (st.oauth?.accessToken) {
        useGrokHub.setState({
          connectors: st.connectors.map((c) =>
            c.id === "grok-xai"
              ? { ...c, status: "connected" as const, lastUsed: Date.now() }
              : c,
          ),
        });
      }
      void useGrokHub.getState().refreshModels();
      void useGrokHub.getState().checkUpdateQuiet();
      void (async () => {
        try {
          const { hostInfo } = await import("@/lib/host-client");
          const info = await hostInfo();
          if (info.unsandboxed && info.bridge !== "none") {
            useGrokHub.setState((s) => ({
              connectors: s.connectors.map((c) =>
                c.id === "desktop-host"
                  ? { ...c, status: "connected" as const, lastUsed: Date.now() }
                  : c,
              ),
            }));
          }
        } catch {
          /* ignore */
        }
      })();
    });
    setIsDesktop(Boolean(window.grokhubDesktop));
    try {
      const safe = useGrokHub.getState().desktop.hostSafeMode;
      void window.grokhubDesktop?.host?.setSafeMode?.(Boolean(safe));
    } catch {
      /* ignore */
    }

    const flush = () => {
      try {
        void import("@/lib/persistent-storage").then((m) => m.flushPersistentStorage());
      } catch {
        /* ignore */
      }
    };
    const onVis = () => {
      if (document.visibilityState === "hidden") flush();
    };
    window.addEventListener("beforeunload", flush);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      window.removeEventListener("beforeunload", flush);
      document.removeEventListener("visibilitychange", onVis);
    };
  }, []);

  useEffect(() => {
    let stop = () => {};
    void import("@/lib/smart-poll").then(({ startSmartPoll }) => {
      stop = startSmartPoll({
        // Keep OAuth alive through the ~6h access-token window (refresh ~30m early)
        intervalMs: 10 * 60 * 1000,
        maxBackoffMs: 30 * 60 * 1000,
        // Run even in background so overnight/idle sessions stay signed in
        onlyWhenVisible: false,
        tick: async () => {
          const st = useGrokHub.getState();
          if (!st.oauth?.accessToken || !st.oauth?.refreshToken) return true;
          try {
            const r = await st.refreshOAuthSession();
            return r.ok;
          } catch {
            return false;
          }
        },
      });
    });
    return () => stop();
  }, []);

  useEffect(() => {
    let stop = () => {};
    void import("@/lib/smart-poll").then(({ startSmartPoll }) => {
      stop = startSmartPoll({
        intervalMs: 5 * 60 * 1000,
        maxBackoffMs: 30 * 60 * 1000,
        onlyWhenVisible: true,
        tick: () => {
          const st = useGrokHub.getState();
          if (st.oauth?.accessToken || st.apiKey || st.grokConnected) {
            void st.refreshModels();
          }
        },
      });
    });
    return () => stop();
  }, []);

  useEffect(() => {
    let stop = () => {};
    void import("@/lib/smart-poll").then(({ startSmartPoll }) => {
      stop = startSmartPoll({
        intervalMs: 60_000,
        maxBackoffMs: 5 * 60 * 1000,
        onlyWhenVisible: true,
        tick: async () => {
          const st = useGrokHub.getState();
          try {
            await st.refreshUsage();
            if (st.ssoCookie || useGrokHub.getState().ssoCookie) {
              try {
                await useGrokHub.getState().syncWebsiteConnectors();
              } catch {
                /* non-fatal */
              }
            }
            const u = useGrokHub.getState().usage;
            if (
              (st.ssoCookie || useGrokHub.getState().ssoCookie) &&
              u.website?.error &&
              u.source !== "website"
            ) {
              return false;
            }
            return true;
          } catch {
            return false;
          }
        },
      });
    });
    return () => stop();
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => {
      if (document.visibilityState === "hidden") return;
      void useGrokHub.getState().tickAutomations();
    }, 30_000);
    const t = window.setTimeout(() => void useGrokHub.getState().tickAutomations(), 5_000);
    return () => {
      window.clearInterval(id);
      window.clearTimeout(t);
    };
  }, []);

  useEffect(() => {
    if (oauth?.name || oauth?.email) {
      void syncFromGrok({
        displayName: oauth.name ?? null,
        email: oauth.email ?? null,
        imageUrl: oauth.picture ?? null,
      });
      return;
    }
    if (isPending) return;
    if (user && !user.isDevFallback) {
      void syncFromGrok({
        displayName: user.displayName,
        email: user.primaryEmail,
        imageUrl: user.profileImageUrl,
      });
    }
  }, [user, isPending, syncFromGrok, oauth?.name, oauth?.email, oauth?.picture]);

  useEffect(() => {
    const hb = window.setInterval(() => tickHeartbeat(), 30000);
    return () => window.clearInterval(hb);
  }, [tickHeartbeat]);

  useEffect(() => {
    setMobileOpen(false);
  }, [nav]);

  // Desktop agent core → UI commands (tray)
  useEffect(() => {
    const api = (window as unknown as {
      grokhubDesktop?: {
        // ipc on via electron - use custom events from preload if needed
      };
    }).grokhubDesktop;
    const onCmd = (e: Event) => {
      const d = (e as CustomEvent).detail || {};
      if (d.type === "open-queue") setNav("queue");
      if (d.type === "new-chat") useGrokHub.getState().newThread();
      if (d.type === "set-paused") useGrokHub.getState().pauseAutonomy(Boolean(d.paused));
    };
    window.addEventListener("grokhub:agent-command", onCmd as EventListener);
    // Bridge ipc if exposed
    try {
      const { ipcRenderer } = (window as unknown as { require?: (m: string) => { ipcRenderer?: { on: Function } } }).require?.("electron") || {};
      // sandboxed — use periodic process instead
    } catch {
      /* ignore */
    }
    return () => window.removeEventListener("grokhub:agent-command", onCmd as EventListener);
  }, [setNav]);

  // Drain agent queue on heartbeat
  useEffect(() => {
    const t = window.setInterval(() => {
      void useGrokHub.getState().processAgentQueue();
    }, 20_000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const tag = (e.target as HTMLElement | null)?.tagName || "";
      const typing =
        tag === "INPUT" || tag === "TEXTAREA" || (e.target as HTMLElement)?.isContentEditable;
      if (mod && (e.key === "k" || e.key === "K")) {
        e.preventDefault();
        setPaletteOpen((v) => !v);
        return;
      }
      if (mod && (e.key === "n" || e.key === "N")) {
        e.preventDefault();
        newThread();
        return;
      }
      if (mod && (e.key === "l" || e.key === "L")) {
        e.preventDefault();
        setNav("chat");
        window.dispatchEvent(new CustomEvent("grokhub:focus-chat-input"));
        return;
      }
      if (mod && (e.key === "/" || e.key === "?")) {
        e.preventDefault();
        setShortcutsOpen(true);
        return;
      }
      // Ctrl+1…5 nav jumps
      if (mod && !e.shiftKey && e.key >= "1" && e.key <= "5") {
        const map: NavId[] = ["chat", "history", "command", "workboard", "imagine"];
        const n = map[Number(e.key) - 1];
        if (n) {
          e.preventDefault();
          setNav(n);
        }
        return;
      }
      // Ctrl+[ / ] cycle threads
      if (mod && (e.key === "[" || e.key === "]") && !typing) {
        e.preventDefault();
        const list = [...useGrokHub.getState().threads].sort((a, b) => {
          if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
          return b.updatedAt - a.updatedAt;
        });
        if (!list.length) return;
        const cur = useGrokHub.getState().activeThreadId;
        const idx = Math.max(0, list.findIndex((t) => t.id === cur));
        const next =
          e.key === "]"
            ? list[(idx + 1) % list.length]!
            : list[(idx - 1 + list.length) % list.length]!;
        useGrokHub.getState().selectThread(next.id);
        return;
      }
      if (!typing && e.key === "/" && !mod) {
        e.preventDefault();
        setNav("chat");
        window.dispatchEvent(new CustomEvent("grokhub:focus-chat-input"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newThread, setNav]);

  useEffect(() => {
    const open = () => setShortcutsOpen(true);
    window.addEventListener("grokhub:open-shortcuts", open);
    return () => window.removeEventListener("grokhub:open-shortcuts", open);
  }, []);

  // Settings deep-link: #sec-oauth etc. or ?settings=
  useEffect(() => {
    const apply = () => {
      const hash = window.location.hash.replace(/^#/, "");
      const params = new URLSearchParams(window.location.search);
      const sec = params.get("settings") || (hash.startsWith("sec-") ? hash : "");
      if (sec) {
        setNav("settings");
        requestAnimationFrame(() => {
          document.getElementById(sec)?.scrollIntoView({ behavior: "smooth", block: "start" });
        });
      }
    };
    apply();
    window.addEventListener("hashchange", apply);
    return () => window.removeEventListener("hashchange", apply);
  }, [setNav]);

  const drag = { WebkitAppRegion: "drag" } as CSSProperties;
  const noDrag = { WebkitAppRegion: "no-drag" } as CSSProperties;
  const recent = [...threads]
    .sort((a, b) => {
      if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
      return b.updatedAt - a.updatedAt;
    })
    .filter((t) => {
      const q = sidebarQ.trim().toLowerCase();
      if (!q) return true;
      if (t.title.toLowerCase().includes(q)) return true;
      if (t.folder?.toLowerCase().includes(q)) return true;
      return (t.messages || []).some((m) => (m.content || "").toLowerCase().includes(q));
    })
    .slice(0, sidebarQ.trim() ? 20 : 10);
  const showOffline = grokConnected === false && !oauth?.accessToken && !apiKey;

  const primaryNav = NAV.filter((item) =>
    ["chat", "history", "command", "workboard", "imagine"].includes(item.id),
  );
  const toolsNav = NAV.filter((item) =>
    ["queue", "skills", "automations", "agents", "settings"].includes(item.id),
  );

  const stageTitle =
    nav === "chat"
      ? activeThread?.title || "New chat"
      : nav === "desktop"
        ? "Desktop host"
        : (NAV.find((n) => n.id === nav)?.label ?? "GrokHub");

  const stageSubtitle =
    nav === "chat"
      ? running
        ? "Working…"
        : accountConnected
          ? grokStatusDetail || "Ready"
          : "Connect Grok in Settings"
      : `GrokHub v${APP_VERSION}`;

  function renderNavButton(item: (typeof NAV)[number], compact = false) {
    const Icon = item.icon;
    const active = nav === item.id;
    return (
      <button
        key={item.id}
        type="button"
        onClick={() => setNav(item.id)}
        className={cn(
          "flex h-9 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors",
          compact && "h-11",
          active
            ? "nav-item-active"
            : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]",
        )}
      >
        <Icon className="h-4 w-4 shrink-0 opacity-80" />
        {item.label}
      </button>
    );
  }

  return (
    <TooltipProvider delayDuration={350}>
      <div className="flex h-dvh max-h-dvh w-full max-w-none flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]">
        <div
          className="flex h-10 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3"
          style={drag}
        >
          <div className="flex items-center gap-2" style={noDrag}>
            <GrokHubMark className="h-6 w-6" />
            <span className="text-xs font-semibold tracking-tight">GrokHub</span>
            <Badge className="hidden font-mono text-[10px] sm:inline-flex">v{APP_VERSION}</Badge>
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  onClick={() => void probeGrok()}
                  className="hidden items-center gap-1.5 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] md:inline-flex"
                  aria-label={grokStatusDetail || "Grok connection"}
                >
                  <span
                    className={cn(
                      "inline-block h-1.5 w-1.5 rounded-full",
                      grokConnected === true
                        ? "bg-[var(--color-success)]"
                        : grokConnected === false
                          ? "bg-[var(--color-danger)]"
                          : "bg-[var(--color-subtle)]",
                    )}
                  />
                  {grokConnected === true
                    ? "Live"
                    : grokConnected === false
                      ? "Offline"
                      : "…"}
                </button>
              </TooltipTrigger>
              <TooltipContent>{grokStatusDetail || "Probe Grok connection"}</TooltipContent>
            </Tooltip>
          </div>
          <div className="flex min-w-0 items-center gap-1.5" style={noDrag}>
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  size="icon"
                  variant="ghost"
                  className="hidden h-7 w-7 sm:inline-flex"
                  onClick={() => setPaletteOpen(true)}
                  aria-label="Command palette"
                >
                  <Search className="h-3.5 w-3.5" />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Command palette (Ctrl+K)</TooltipContent>
            </Tooltip>
            <UsageMeterChip className="hidden max-w-[140px] sm:flex" />
            {updateBanner?.available && (
              <Tooltip>
                <TooltipTrigger asChild>
                  <button
                    type="button"
                    onClick={() => setNav("settings")}
                    className="hidden items-center gap-1 rounded-full border border-[color-mix(in_oklab,var(--color-info)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-info)_12%,transparent)] px-2 py-0.5 text-[10px] font-medium text-[var(--color-info)] sm:inline-flex"
                  >
                    <Download className="h-3 w-3" />
                    Update
                  </button>
                </TooltipTrigger>
                <TooltipContent>
                  {updateBanner.detail || "Update available — open Settings → Updates"}
                  <button
                    type="button"
                    className="ml-2 underline"
                    onClick={(e) => {
                      e.stopPropagation();
                      setUpdateBanner(null);
                    }}
                  >
                    dismiss
                  </button>
                </TooltipContent>
              </Tooltip>
            )}
            <ModePicker />
            {isPending && !oauth ? (
              <div className="hidden h-7 w-16 animate-pulse rounded bg-[var(--color-elevated)] sm:block" />
            ) : accountLabel ? (
              <button
                type="button"
                onClick={() => setNav("settings")}
                className="hidden max-w-[9rem] truncate rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)] sm:inline"
                title={accountLabel}
              >
                {accountLabel}
              </button>
            ) : user && !user.isDevFallback ? (
              <div className="hidden scale-90 sm:block">
                <UserButton />
              </div>
            ) : (
              <button
                type="button"
                onClick={() => setNav("settings")}
                className="hidden rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:text-[var(--color-fg)] sm:inline"
              >
                Connect
              </button>
            )}
            {isDesktop && (
              <div className="ml-1 flex items-center gap-0.5">
                <button
                  type="button"
                  className="flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]"
                  onClick={() => window.grokhubDesktop?.minimize?.()}
                  aria-label="Minimize"
                >
                  <Minus className="h-3.5 w-3.5" />
                </button>
                <button
                  type="button"
                  className="flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[var(--color-elevated)]"
                  onClick={() => window.grokhubDesktop?.maximize?.()}
                  aria-label="Maximize"
                >
                  <Square className="h-3 w-3" />
                </button>
                <button
                  type="button"
                  className="flex h-7 w-8 items-center justify-center rounded text-[var(--color-muted)] hover:bg-[color-mix(in_oklab,var(--color-danger)_25%,transparent)] hover:text-[var(--color-danger)]"
                  onClick={() => window.grokhubDesktop?.close?.()}
                  aria-label="Close"
                >
                  <X className="h-3.5 w-3.5" />
                </button>
              </div>
            )}
          </div>
        </div>

        {showOffline ? (
          <div className="shrink-0 border-b border-[color-mix(in_oklab,var(--color-warn)_28%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_14%,var(--color-surface))] px-3 py-1.5 text-center text-[11px] text-[var(--color-fg)]">
            <span className="text-[var(--color-warn)]">Offline</span>
            <span className="text-[var(--color-muted)]"> — free fallback when possible. </span>
            <button
              type="button"
              className="font-medium text-[var(--color-fg)] underline decoration-[var(--color-warn)] underline-offset-2 hover:text-[var(--color-warn)]"
              onClick={() => setNav("settings")}
            >
              Connect Grok
            </button>
          </div>
        ) : null}

        <div className="app-frame flex min-h-0 w-full flex-1 overflow-hidden">
          <aside className="sidebar-rail hidden shrink-0 flex-col overflow-hidden md:flex">
            <div className="shrink-0 space-y-2 p-3 pb-1">
              <Button size="sm" className="w-full" onClick={() => newThread()} title="Ctrl+N">
                <MessageSquarePlus className="h-4 w-4" />
                New chat
              </Button>
              <button
                type="button"
                onClick={() => setPaletteOpen(true)}
                className="flex h-8 w-full items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2.5 text-left text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
              >
                <Search className="h-3.5 w-3.5 shrink-0" />
                <span className="flex-1 truncate">Search…</span>
                <kbd className="font-mono text-[10px] text-[var(--color-subtle)]">Ctrl+K</kbd>
              </button>
            </div>
            <nav className="scroll-panel flex flex-1 flex-col gap-0.5 p-3 pt-1">
              <div className="mb-1 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                Workspace
              </div>
              {primaryNav.map((item) => renderNavButton(item))}

              <button
                type="button"
                className="mb-0.5 mt-3 flex w-full items-center gap-1 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)] hover:text-[var(--color-muted)]"
                onClick={() => setToolsNavCollapsed(!toolsNavCollapsed)}
              >
                {toolsNavCollapsed ? (
                  <ChevronRight className="h-3 w-3" />
                ) : (
                  <ChevronDown className="h-3 w-3" />
                )}
                Tools
              </button>
              {!toolsNavCollapsed && toolsNav.map((item) => renderNavButton(item))}

              <div className="mt-3 border-t border-[var(--color-border)] pt-3">
                <div className="mb-1.5 flex items-center justify-between px-1">
                  <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                    Recent
                  </span>
                  <button
                    type="button"
                    className="text-[10px] text-[var(--color-subtle)] hover:text-[var(--color-fg)]"
                    onClick={() => setNav("history")}
                  >
                    All
                  </button>
                </div>
                <input
                  value={sidebarQ}
                  onChange={(e) => setSidebarQ(e.target.value)}
                  placeholder="Filter chats…"
                  className="mb-1.5 w-full rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-bg)] px-2 py-1 text-[11px] text-[var(--color-fg)] outline-none placeholder:text-[var(--color-subtle)] focus:ring-1 focus:ring-[var(--color-ring)]"
                  aria-label="Filter recent chats"
                />
                {recent.map((t) => (
                  <RecentThreadRow
                    key={t.id}
                    id={t.id}
                    title={t.title}
                    pinned={t.pinned}
                    folder={t.folder}
                    active={t.id === activeThreadId}
                    onSelect={() => selectThread(t.id)}
                  />
                ))}
                {recent.length === 0 && (
                  <p className="px-2 py-1 text-[11px] text-[var(--color-subtle)]">No chats yet</p>
                )}
              </div>
            </nav>
          </aside>

          <div className="app-stage flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-[var(--color-bg)]">
            <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[var(--color-panel)] px-4 py-2 md:px-6 3xl:px-8">
              <div className="flex min-w-0 items-center gap-3">
                <Button
                  variant="ghost"
                  size="icon"
                  className="md:hidden"
                  onClick={() => setMobileOpen((v) => !v)}
                  aria-label="Menu"
                >
                  {mobileOpen ? <X className="h-5 w-5" /> : <Menu className="h-5 w-5" />}
                </Button>
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold tracking-tight md:text-[0.95rem]">
                    {stageTitle}
                  </div>
                  <div className="flex items-center gap-2 truncate text-[11px] text-[var(--color-subtle)]">
                    {running ? (
                      <span className="inline-flex items-center gap-1.5 text-[var(--color-info)]">
                        <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-info)]" />
                        Working
                      </span>
                    ) : (
                      <span>{stageSubtitle}</span>
                    )}
                    {nav === "chat" && (
                      <span className="hidden font-mono text-[10px] sm:inline">
                        · {modeMeta.label}
                      </span>
                    )}
                  </div>
                </div>
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <Button
                  size="sm"
                  variant="secondary"
                  className="hidden sm:inline-flex"
                  onClick={() => newThread()}
                  title="New chat (Ctrl+N)"
                >
                  <MessageSquarePlus className="h-3.5 w-3.5" />
                  New
                </Button>
                <Button
                  size="icon"
                  variant="secondary"
                  className="sm:hidden"
                  onClick={() => newThread()}
                  aria-label="New chat"
                >
                  <MessageSquarePlus className="h-4 w-4" />
                </Button>
                <UsageMeterChip className="max-w-[120px] sm:hidden" />
              </div>
            </header>

            {mobileOpen && (
              <div className="shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface)] p-2 md:hidden">
                <div className="mb-2 px-1">
                  <UsageMeterChip className="w-full" />
                </div>
                <div className="grid max-h-[min(50dvh,22rem)] grid-cols-2 gap-1 overflow-y-auto scroll-panel">
                  {NAV.map((item) => renderNavButton(item, true))}
                </div>
              </div>
            )}

            <main
              className={cn(
                "app-stage flex min-h-0 flex-1 flex-col overflow-hidden",
                nav === "chat" ? "p-0" : "p-3 sm:p-4 md:p-5 3xl:p-6 uw:p-8",
              )}
              aria-label={stageTitle}
            >
              <Suspense
                fallback={
                  <div className="flex flex-1 items-center justify-center text-sm text-[var(--color-subtle)]">
                    Loading…
                  </div>
                }
              >
                {nav === "chat" ? (
                  <div className="chat-stage min-h-0 flex-1 overflow-hidden">
                    <ChatView />
                  </div>
                ) : (
                  <div className="scroll-panel min-h-0 flex-1">
                    {nav === "history" && <HistoryView />}
                    {nav === "command" && <CommandView />}
                                        {nav === "skills" && <SkillsView />}
                    {nav === "automations" && <AutomationsView />}
                    {nav === "agents" && <AgentsView />}
                    {nav === "workboard" && <WorkboardView />}
                    {nav === "queue" && <AgentQueueView />}
                    {nav === "imagine" && <ImagineView />}
                    {nav === "desktop" && <DesktopHostView />}
                    {nav === "settings" && <SettingsView />}
                  </div>
                )}
              </Suspense>
            </main>
          </div>
        </div>

        <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
        <ShortcutsDialog open={shortcutsOpen} onOpenChange={setShortcutsOpen} />
        <UndoToast />
      </div>
    </TooltipProvider>
  );
}
