import type { ComponentType, CSSProperties } from "react";
import {
  Cable,
  Command,
  HardDrive,
  ImageIcon,
  MessageSquare,
  Minus,
  Settings,
  Sparkles,
  Square,
  TimerReset,
  Users,
  Menu,
  X,
} from "lucide-react";
import { useEffect, useState } from "react";
import { getMode } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import type { NavId } from "@/lib/types";
import { cn } from "@/lib/utils";
import { GrokHubMark } from "./GrokLogo";
import { ModePicker } from "./ModePicker";
import { RelativeTime } from "./RelativeTime";
import { UsageMeterChip } from "./UsageMeter";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { AgentsView } from "./views/AgentsView";
import { AutomationsView } from "./views/AutomationsView";
import { ChatView } from "./views/ChatView";
import { CommandView } from "./views/CommandView";
import { ConnectorsView } from "./views/ConnectorsView";
import { DesktopHostView } from "./views/DesktopHostView";
import { ImagineView } from "./views/ImagineView";
import { SettingsView } from "./views/SettingsView";
import { SkillsView } from "./views/SkillsView";

const APP_VERSION = "0.1";

const NAV: { id: NavId; label: string; icon: ComponentType<{ className?: string }> }[] = [
  { id: "chat", label: "Agent", icon: MessageSquare },
  { id: "command", label: "Command", icon: Command },
  { id: "desktop", label: "Desktop", icon: HardDrive },
  { id: "imagine", label: "Imagine", icon: ImageIcon },
  { id: "connectors", label: "Connectors", icon: Cable },
  { id: "skills", label: "Skills", icon: Sparkles },
  { id: "automations", label: "Automations", icon: TimerReset },
  { id: "agents", label: "Roster", icon: Users },
  { id: "settings", label: "Settings", icon: Settings },
];

export function AppShell() {
  const nav = useGrokHub((s) => s.nav);
  const setNav = useGrokHub((s) => s.setNav);
  const heartbeatAt = useGrokHub((s) => s.heartbeatAt);
  const running = useGrokHub((s) => s.running);
  const mode = useGrokHub((s) => s.mode);
  const tickHeartbeat = useGrokHub((s) => s.tickHeartbeat);
  const refreshStaleTimes = useGrokHub((s) => s.refreshStaleTimes);
  const resetDemo = useGrokHub((s) => s.resetDemo);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const grokStatusDetail = useGrokHub((s) => s.grokStatusDetail);
  const probeGrok = useGrokHub((s) => s.probeGrok);
  const [mobileOpen, setMobileOpen] = useState(false);
  const [isDesktop, setIsDesktop] = useState(false);
  const modeMeta = getMode(mode);

  useEffect(() => {
    const p = useGrokHub.persist.rehydrate();
    Promise.resolve(p).finally(() => {
      useGrokHub.getState().refreshStaleTimes();
      useGrokHub.getState().tickHeartbeat();
      void useGrokHub.getState().probeGrok();
    });
    setIsDesktop(Boolean(window.grokhubDesktop));
  }, []);

  useEffect(() => {
    const hb = window.setInterval(() => tickHeartbeat(), 30000);
    return () => window.clearInterval(hb);
  }, [tickHeartbeat]);

  useEffect(() => {
    setMobileOpen(false);
  }, [nav]);

  const drag = { WebkitAppRegion: "drag" } as CSSProperties;
  const noDrag = { WebkitAppRegion: "no-drag" } as CSSProperties;

  return (
    <div className="flex h-dvh max-h-dvh flex-col overflow-hidden bg-[var(--color-bg)] text-[var(--color-fg)]">
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

      <div className="mx-auto flex min-h-0 w-full max-w-[1600px] flex-1 overflow-hidden">
        <aside className="hidden w-56 shrink-0 flex-col overflow-hidden border-r border-[var(--color-border)] bg-[var(--color-surface)] md:flex">
          <nav className="scroll-panel flex flex-1 flex-col gap-1 p-3">
            {NAV.map((item) => {
              const Icon = item.icon;
              const active = nav === item.id;
              return (
                <button
                  key={item.id}
                  type="button"
                  onClick={() => setNav(item.id)}
                  className={cn(
                    "flex h-10 shrink-0 items-center gap-2.5 rounded-[var(--radius-sm)] px-3 text-sm transition-colors",
                    active
                      ? "bg-[var(--color-elevated)] text-[var(--color-fg)]"
                      : "text-[var(--color-muted)] hover:bg-[var(--color-elevated)]/60 hover:text-[var(--color-fg)]",
                  )}
                >
                  <Icon className="h-4 w-4 shrink-0" />
                  {item.label}
                </button>
              );
            })}
          </nav>

          <div className="shrink-0 space-y-3 border-t border-[var(--color-border)] p-4">
            <UsageMeterChip className="w-full" />
            <div className="flex items-center gap-2 text-xs text-[var(--color-muted)]">
              <span className="pulse-live inline-block h-2 w-2 rounded-full bg-[var(--color-success)]" />
              Heartbeat <RelativeTime ts={heartbeatAt} />
            </div>
            <div className="rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2.5 py-2">
              <div className="text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                Mode
              </div>
              <div className="text-xs font-medium">{modeMeta.label}</div>
              <div className="text-[10px] text-[var(--color-muted)]">{modeMeta.subtitle}</div>
            </div>
            <Button
              variant="secondary"
              size="sm"
              className="w-full"
              onClick={() => {
                resetDemo();
                refreshStaleTimes();
              }}
            >
              Reset demo
            </Button>
          </div>
        </aside>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden">
          <header className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-bg)_88%,transparent)] px-4 py-3 backdrop-blur-md md:px-6">
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
                  {NAV.find((n) => n.id === nav)?.label}
                </div>
                <div className="truncate text-xs text-[var(--color-subtle)]">
                  GrokHub v{APP_VERSION} · {grokConnected ? "live Grok" : "connect in Settings"}
                </div>
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <UsageMeterChip className="max-w-[140px] sm:hidden" />
              {running ? (
                <Badge variant="info">Working</Badge>
              ) : (
                <Badge variant="success">Online</Badge>
              )}
              <Badge className="hidden font-mono sm:inline-flex">
                {modeMeta.label} · 4.5
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

          <main className="flex min-h-0 flex-1 flex-col overflow-hidden p-4 md:p-6">
            <div className="scroll-panel min-h-0 flex-1">
              {nav === "command" && <CommandView />}
              {nav === "connectors" && <ConnectorsView />}
              {nav === "skills" && <SkillsView />}
              {nav === "automations" && <AutomationsView />}
              {nav === "agents" && <AgentsView />}
              {nav === "imagine" && <ImagineView />}
              {nav === "desktop" && <DesktopHostView />}
              {nav === "settings" && <SettingsView />}
            </div>
            {nav === "chat" && (
              <div className="min-h-0 flex-1 overflow-hidden">
                <ChatView />
              </div>
            )}
          </main>
        </div>
      </div>
    </div>
  );
}
