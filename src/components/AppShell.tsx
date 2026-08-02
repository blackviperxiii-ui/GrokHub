import type { ComponentType, CSSProperties } from "react";
import {
  Cable,
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
  Settings,
  Sparkles,
  Square,
  TimerReset,
  Trash2,
  Users,
  X,
} from "lucide-react";
import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { NavId } from "@/lib/types";
import { cn } from "@/lib/utils";
import { APP_VERSION } from "@/lib/version";
import { useCurrentUserState } from "@/lib/auth/use-current-user";
import { UserButton } from "@/lib/auth/gates";
import { GrokHubMark } from "./GrokLogo";
import { ModePicker } from "./ModePicker";
import { RelativeTime } from "./RelativeTime";
import { UsageMeterChip } from "./UsageMeter";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
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
const ConnectorsView = lazy(() =>
  import("./views/ConnectorsView").then((m) => ({ default: m.ConnectorsView })),
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


const NAV: { id: NavId; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "chat", label: "Agent", icon: MessageSquare },
  { id: "history", label: "History", icon: History },
  { id: "command", label: "Command", icon: Command },
  { id: "imagine", label: "Imagine", icon: ImageIcon },
  { id: "connectors", label: "Connectors", icon: Cable },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "automations", label: "Automations", icon: TimerReset },
  { id: "agents", label: "Roster", icon: Users },
  { id: "settings", label: "Settings", icon: Settings },
];

function RecentThreadRow({
  id,
  title,
  active,
  pinned,
  folder,
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
  const [menuOpen, setMenuOpen] = useState(false);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(title);
  const menuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    setDraft(title);
  }, [title]);

  useEffect(() => {
    if (!menuOpen) return;
    const onDoc = (e: MouseEvent) => {
      if (!menuRef.current?.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener("mousedown", onDoc);
    return () => document.removeEventListener("mousedown", onDoc);
  }, [menuOpen]);

  useEffect(() => {
    if (renaming) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [renaming]);

  function commitRename() {
    const next = draft.trim();
    // Any explicit rename commit locks the title (even if text unchanged)
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
          className="w-full rounded-[var(--radius-sm)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-2 py-1 text-xs text-[var(--color-fg)] outline-none focus:ring-1 focus:ring-[var(--color-accent)]"
          aria-label="Rename chat"
        />
      </div>
    );
  }

  return (
    <div
      className={cn(
        "group relative mb-0.5 flex w-full items-center gap-0.5 rounded-[var(--radius-sm)]",
        active
          ? "bg-[var(--color-elevated)] text-[var(--color-fg)]"
          : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/50",
      )}
    >
      <button
        type="button"
        onClick={onSelect}
        className="min-w-0 flex-1 truncate px-2.5 py-1.5 text-left text-xs font-medium"
      >
        {title}
      </button>
      <div className="relative shrink-0 pr-0.5" ref={menuRef}>
        <button
          type="button"
          className={cn(
            "rounded p-1 text-[var(--color-subtle)] transition-opacity hover:bg-[var(--color-surface)] hover:text-[var(--color-fg)]",
            menuOpen ? "opacity-100" : "opacity-0 group-hover:opacity-100 focus:opacity-100",
          )}
          aria-label="Chat options"
          onClick={(e) => {
            e.stopPropagation();
            setMenuOpen((v) => !v);
          }}
        >
          <MoreHorizontal className="h-3.5 w-3.5" />
        </button>
        {menuOpen && (
          <div className="absolute right-0 top-full z-50 mt-0.5 min-w-[132px] overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] py-1 shadow-lg">
            <button
              type="button"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-fg)] hover:bg-[var(--color-surface)]"
              onClick={() => {
                setMenuOpen(false);
                setDraft(title);
                setRenaming(true);
              }}
            >
              <Pencil className="h-3 w-3" />
              Rename
            </button>
            <button
              type="button"
              className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs text-[var(--color-danger)] hover:bg-[var(--color-surface)]"
              onClick={() => {
                setMenuOpen(false);
                deleteThread(id);
              }}
            >
              <Trash2 className="h-3 w-3" />
              Delete
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

export function AppShell() {
  const nav = useGrokHub((s) => s.nav);
  const setNav = useGrokHub((s) => s.setNav);
  const heartbeatAt = useGrokHub((s) => s.heartbeatAt);
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
  const { user, isPending } = useCurrentUserState();
  const [mobileOpen, setMobileOpen] = useState(false);
  const [isDesktop, setIsDesktop] = useState(false);
  const modeMeta = getMode(mode);

  const accountLabel =
    oauth?.name ||
    oauth?.email ||
    profile?.displayName ||
    profile?.email ||
    (user && !user.isDevFallback
      ? user.displayName || user.primaryEmail || null
      : null);

  const accountConnected = Boolean(oauth?.accessToken || (user && !user.isDevFallback) || grokConnected);

  useEffect(() => {
    const p = useGrokHub.persist.rehydrate();
    Promise.resolve(p).finally(() => {
      // Always land on Agent; clear sticky run/stream flags from a crashed session
      useGrokHub.setState({
        nav: "chat",
        running: false,
        streamStatus: null,
        streamingMessageId: null,
        pendingHostConfirm: null,
      });
      // Re-load Imagine media from disk (paths only in JSON after update)
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
      });
      // Reflect persisted OAuth on the Grok connector
      if (st.oauth?.accessToken) {
        useGrokHub.setState({
          connectors: st.connectors.map((c) =>
            c.id === "grok-xai"
              ? { ...c, status: "connected" as const, lastUsed: Date.now() }
              : c,
          ),
        });
      }
      void useGrokHub.getState().probeGrok();
      void useGrokHub.getState().refreshModels();
      // Auto-probe desktop host connector
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
    // Sync host safe mode preference into Electron host bridge
    try {
      const safe = useGrokHub.getState().desktop.hostSafeMode;
      void window.grokhubDesktop?.host?.setSafeMode?.(Boolean(safe));
    } catch {
      /* ignore */
    }

    // Flush memory on hide/close so restarts keep the latest turn
    const flush = () => {
      try {
        void import("@/lib/persistent-storage").then((m) => m.flushPersistentStorage());
        useGrokHub.setState((s) => ({ heartbeatAt: s.heartbeatAt }));
      } catch {
        /* ignore */
      }
    };
    window.addEventListener("beforeunload", flush);
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") flush();
    });
    return () => {
      window.removeEventListener("beforeunload", flush);
    };
  }, []);

  // Poll essential models every 5 minutes while connected (pause when hidden)
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

  // Subscription usage — poll every 1 minute when linked; backoff when failing / hidden
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
            // Back off when SSO is present but website still errors
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

  // Automations scheduler — every 30s
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

  // Prefer Grok OAuth identity once connected (not only Better Auth session)
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

  // Global shortcuts: Ctrl/Cmd+N new chat, Ctrl/Cmd+L or / focus composer
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const mod = e.metaKey || e.ctrlKey;
      const tag = (e.target as HTMLElement | null)?.tagName || "";
      const typing =
        tag === "INPUT" || tag === "TEXTAREA" || (e.target as HTMLElement)?.isContentEditable;
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
      if (!typing && e.key === "/" && !mod) {
        e.preventDefault();
        setNav("chat");
        window.dispatchEvent(new CustomEvent("grokhub:focus-chat-input"));
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [newThread, setNav]);

  const drag = { WebkitAppRegion: "drag" } as CSSProperties;
  const noDrag = { WebkitAppRegion: "no-drag" } as CSSProperties;
  const recent = [...threads]
    .sort((a, b) => {
      if (Boolean(a.pinned) !== Boolean(b.pinned)) return a.pinned ? -1 : 1;
      return b.updatedAt - a.updatedAt;
    })
    .slice(0, 8);
  const showOffline =
    grokConnected === false && !oauth?.accessToken && !apiKey;

  return (
    <div className="flex h-dvh max-h-dvh w-full max-w-none flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]">
      <div
        className="flex h-10 shrink-0 items-center justify-between border-b border-[var(--color-border)] bg-[var(--color-surface)] px-3"
        style={drag}
      >
        <div className="flex items-center gap-2" style={noDrag}>
          <GrokHubMark className="h-6 w-6" />
          <span className="text-xs font-semibold tracking-tight">GrokHub</span>
          <Badge className="hidden font-mono text-[10px] sm:inline-flex">v{APP_VERSION}</Badge>
          <button
            type="button"
            onClick={() => void probeGrok()}
            className="hidden items-center gap-1.5 rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] md:inline-flex"
            title={grokStatusDetail}
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
              ? "Grok live"
              : grokConnected === false
                ? "Grok offline"
                : "Grok…"}
          </button>
        </div>
        <div className="flex min-w-0 items-center gap-2" style={noDrag}>
          <UsageMeterChip className="hidden max-w-[160px] sm:flex" />
          <ModePicker />
          {isPending && !oauth ? (
            <div className="hidden h-7 w-20 animate-pulse rounded bg-[var(--color-elevated)] sm:block" />
          ) : accountLabel ? (
            <button
              type="button"
              onClick={() => setNav("settings")}
              className="hidden max-w-[10rem] truncate rounded-full border border-[var(--color-border)] px-2.5 py-1 text-[11px] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)] sm:inline"
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
              Connect Grok
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
        <div className="shrink-0 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-warn)_12%,var(--color-surface))] px-3 py-1.5 text-center text-[11px] text-[var(--color-muted)]">
          Offline / not connected — free fallback when possible.{" "}
          <button type="button" className="underline" onClick={() => setNav("settings")}>
            Connect Grok in Settings
          </button>
        </div>
      ) : null}

      <div className="app-frame flex min-h-0 w-full flex-1 overflow-hidden">
        <aside className="sidebar-rail hidden shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)] bg-[var(--color-surface)] md:flex">
          <div className="shrink-0 p-3 pb-1">
            <Button size="sm" className="w-full" onClick={() => newThread()} title="Ctrl+N">
              <MessageSquarePlus className="h-4 w-4" />
              New chat
            </Button>
          </div>
          <nav className="scroll-panel flex flex-1 flex-col gap-1 p-3 pt-1">
            <div className="mb-1 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
              Workspace
            </div>
            {NAV.filter((item) =>
              ["chat", "history", "imagine", "command"].includes(item.id),
            ).map((item) => {
              const Icon = item.icon;
              const active = nav === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setNav(item.id)}
                  className={cn(
                    "flex h-9 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors",
                    active
                      ? "bg-[var(--color-elevated)] text-[var(--color-fg)] font-medium"
                      : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]",
                  )}
                >
                  <Icon className="h-4 w-4 shrink-0 opacity-80" />
                  {item.label}
                </button>
              );
            })}
            <div className="mb-1 mt-3 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
              Tools
            </div>
            {NAV.filter((item) =>
              ["connectors", "skills", "automations", "agents", "settings"].includes(item.id),
            ).map((item) => {
              const Icon = item.icon;
              const active = nav === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setNav(item.id)}
                  className={cn(
                    "flex h-9 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors",
                    active
                      ? "bg-[var(--color-elevated)] text-[var(--color-fg)] font-medium"
                      : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]",
                  )}
                >
                  <Icon className="h-4 w-4 shrink-0 opacity-80" />
                  {item.label}
                </button>
              );
            })}

            <div className="mt-3 border-t border-[var(--color-border)] pt-3">
              <div className="mb-1.5 px-1 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                Recent
              </div>
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

          <div className="shrink-0 space-y-2 border-t border-[var(--color-border)] p-4">
            <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
              <span className="pulse-live inline-block h-2 w-2 rounded-full bg-[var(--color-success)]" />
              Heartbeat <RelativeTime ts={heartbeatAt} />
            </div>
          </div>
        </aside>

        <div className="app-stage flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-bg)_88%,transparent)] px-4 py-3 backdrop-blur-md md:px-6 3xl:px-8 uw:px-10">
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
                <div className="text-sm font-medium md:text-base">
                  {nav === "desktop"
                    ? "Desktop host"
                    : (NAV.find((n) => n.id === nav)?.label ?? "Agent")}
                </div>
                <div className="truncate text-xs text-[var(--color-subtle)]">
                  GrokHub v{APP_VERSION} ·{" "}
                  {accountLabel
                    ? accountLabel
                    : accountConnected
                      ? grokStatusDetail || "Grok connected"
                      : "Connect Grok in Settings"}
                </div>
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <Button
                size="sm"
                variant="secondary"
                className="hidden sm:inline-flex"
                onClick={() => newThread()}
                title="New chat (Ctrl+N)"
              >
                <MessageSquarePlus className="h-3.5 w-3.5" />
                New chat
              </Button>
              <Button
                size="icon"
                variant="secondary"
                className="sm:hidden"
                onClick={() => newThread()}
                aria-label="New chat"
                title="New chat (Ctrl+N)"
              >
                <MessageSquarePlus className="h-4 w-4" />
              </Button>
              <UsageMeterChip className="max-w-[140px] sm:hidden" />
              {running ? (
                <Badge variant="info">Working…</Badge>
              ) : (
                <Badge variant="success">Ready</Badge>
              )}
              <Badge className="hidden font-mono sm:inline-flex">
                {modeMeta.label} · {modeMeta.model}
              </Badge>
            </div>
          </header>

          {mobileOpen && (
            <div className="shrink-0 border-b border-[var(--color-border)] bg-[var(--color-surface)] p-2 md:hidden">
              <div className="mb-2 px-1">
                <UsageMeterChip className="w-full" />
              </div>
              <div className="grid grid-cols-2 gap-1">
                {NAV.map((item) => {
                  const Icon = item.icon;
                  const active = nav === item.id;
                  return (
                    <button
                      key={item.id}
                      type="button"
                      onClick={() => setNav(item.id)}
                      className={cn(
                        "flex h-11 items-center gap-2 rounded-[var(--radius-sm)] px-3 text-sm",
                        active
                          ? "bg-[var(--color-elevated)] text-[var(--color-fg)]"
                          : "text-[var(--color-muted)]",
                      )}
                    >
                      <Icon className="h-4 w-4" />
                      {item.label}
                    </button>
                  );
                })}
              </div>
            </div>
          )}

          <main className="app-stage flex min-h-0 flex-1 flex-col overflow-hidden p-3 sm:p-4 md:p-5 3xl:p-6 uw:p-8">
            <Suspense
              fallback={
                <div className="flex flex-1 items-center justify-center text-sm text-[var(--color-subtle)]">
                  Loading…
                </div>
              }
            >
              {(nav === "chat" || nav === "history") && nav === "chat" ? (
                <div className="chat-stage min-h-0 flex-1 overflow-hidden">
                  <ChatView />
                </div>
              ) : (
                <div className="scroll-panel min-h-0 flex-1">
                  {nav === "history" && <HistoryView />}
                  {nav === "command" && <CommandView />}
                  {nav === "connectors" && <ConnectorsView />}
                  {nav === "skills" && <SkillsView />}
                  {nav === "automations" && <AutomationsView />}
                  {nav === "agents" && <AgentsView />}
                  {nav === "imagine" && <ImagineView />}
                  {nav === "desktop" && <DesktopHostView />}
                  {nav === "settings" && <SettingsView />}
                </div>
              )}
            </Suspense>
          </main>
        </div>
      </div>
    </div>
  );
}
