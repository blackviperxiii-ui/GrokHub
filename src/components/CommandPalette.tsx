import type { ComponentType } from "react";
import { Command } from "cmdk";
import {
  Cable,
  ClipboardList,
  Command as CommandIcon,
  Download,
  History,
  ImageIcon,
  MessageSquare,
  MessageSquarePlus,
  Moon,
  Search,
  Settings,
  Sparkles,
  Sun,
  Terminal,
  TimerReset,
  Wand2,
  Square,
  Keyboard,
  FileDown,
  ListTodo,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useGrokHub } from "@/lib/store";
import type { GrokModeId, NavId } from "@/lib/types";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "./ui/dialog";
import { cn } from "@/lib/utils";

type Item = {
  id: string;
  label: string;
  hint?: string;
  group: string;
  icon: ComponentType<{ className?: string }>;
  run: () => void;
};

export function CommandPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
}) {
  const setNav = useGrokHub((s) => s.setNav);
  const newThread = useGrokHub((s) => s.newThread);
  const setMode = useGrokHub((s) => s.setMode);
  const selectThread = useGrokHub((s) => s.selectThread);
  const threads = useGrokHub((s) => s.threads);
  const setUiTheme = useGrokHub((s) => s.setUiTheme);
  const compactThread = useGrokHub((s) => s.compactThread);
  const checkUpdateQuiet = useGrokHub((s) => s.checkUpdateQuiet);
  const runAutomation = useGrokHub((s) => s.runAutomation);
  const automations = useGrokHub((s) => s.automations);
  const pinWorkItem = useGrokHub((s) => s.pinWorkItem);
  const stopChat = useGrokHub((s) => s.stopChat);
  const exportThreadMarkdown = useGrokHub((s) => s.exportThreadMarkdown);
  const running = useGrokHub((s) => s.running);
  const getContextStats = useGrokHub((s) => s.getContextStats);
  const [q, setQ] = useState("");

  useEffect(() => {
    if (!open) setQ("");
  }, [open]);

  const items = useMemo<Item[]>(() => {
    const go = (nav: NavId) => {
      setNav(nav);
      onOpenChange(false);
    };
    const mode = (id: GrokModeId, label: string) => ({
      id: `mode-${id}`,
      label: `Mode: ${label}`,
      group: "Modes",
      icon: Sparkles,
      run: () => {
        setMode(id);
        onOpenChange(false);
      },
    });
    const navItems: Item[] = [
      {
        id: "nav-chat",
        label: "Agent",
        hint: "Chat",
        group: "Navigate",
        icon: MessageSquare,
        run: () => go("chat"),
      },
      {
        id: "nav-history",
        label: "History",
        group: "Navigate",
        icon: History,
        run: () => go("history"),
      },
      {
        id: "nav-imagine",
        label: "Imagine",
        group: "Navigate",
        icon: ImageIcon,
        run: () => go("imagine"),
      },
      {
        id: "nav-queue",
        label: "Agent queue",
        group: "Navigate",
        icon: ListTodo,
        run: () => go("queue"),
      },
      {
        id: "nav-command",
        label: "Command",
        group: "Navigate",
        icon: Terminal,
        run: () => go("command"),
      },
      {
        id: "nav-workboard",
        label: "Workboard",
        hint: "Tasks",
        group: "Navigate",
        icon: ClipboardList,
        run: () => go("workboard"),
      },
      {
        id: "nav-skills",
        label: "Skills",
        group: "Navigate",
        icon: Sparkles,
        run: () => go("skills"),
      },
      {
        id: "nav-automations",
        label: "Automations",
        group: "Navigate",
        icon: TimerReset,
        run: () => go("automations"),
      },
      {
        id: "nav-settings",
        label: "Settings",
        group: "Navigate",
        icon: Settings,
        run: () => go("settings"),
      },
      {
        id: "act-new",
        label: "New chat",
        hint: "Ctrl+N",
        group: "Actions",
        icon: MessageSquarePlus,
        run: () => {
          newThread();
          onOpenChange(false);
        },
      },
      {
        id: "act-focus",
        label: "Focus composer",
        hint: "Ctrl+L",
        group: "Actions",
        icon: Search,
        run: () => {
          setNav("chat");
          onOpenChange(false);
          window.dispatchEvent(new CustomEvent("grokhub:focus-chat-input"));
        },
      },
      {
        id: "act-compact",
        label: "Compact this chat",
        hint: "Context",
        group: "Actions",
        icon: Wand2,
        run: () => {
          compactThread();
          onOpenChange(false);
        },
      },
      {
        id: "act-update",
        label: "Check for updates",
        group: "Actions",
        icon: Download,
        run: () => {
          void checkUpdateQuiet();
          setNav("settings");
          onOpenChange(false);
        },
      },
      {
        id: "act-work",
        label: "Pin workboard task",
        hint: "Quick",
        group: "Actions",
        icon: ClipboardList,
        run: () => {
          pinWorkItem({
            title: "New task",
            detail: "From command palette",
            priority: "normal",
            source: "user",
          });
          setNav("workboard");
          onOpenChange(false);
        },
      },
      ...automations
        .filter((a) => a.enabled)
        .slice(0, 5)
        .map((a) => ({
          id: `auto-${a.id}`,
          label: `Run automation: ${a.name}`,
          group: "Automations",
          icon: TimerReset,
          run: () => {
            void runAutomation(a.id);
            onOpenChange(false);
          },
        })),
      mode("auto", "Adaptive"),
      mode("fast", "Fast"),
      mode("balanced", "Balanced"),
      mode("expert", "Expert"),
      mode("heavy", "Heavy"),
      mode("max", "Max"),
      mode("build", "Build"),
      {
        id: "theme-dark",
        label: "Theme: Dark",
        group: "Appearance",
        icon: Moon,
        run: () => {
          setUiTheme("dark");
          onOpenChange(false);
        },
      },
      {
        id: "theme-light",
        label: "Theme: Light",
        group: "Appearance",
        icon: Sun,
        run: () => {
          setUiTheme("light");
          onOpenChange(false);
        },
      },
      {
        id: "theme-system",
        label: "Theme: System",
        group: "Appearance",
        icon: CommandIcon,
        run: () => {
          setUiTheme("system");
          onOpenChange(false);
        },
      },
    ];

    const recent = [...threads]
      .sort((a, b) => b.updatedAt - a.updatedAt)
      .slice(0, 8)
      .map((t) => ({
        id: `thread-${t.id}`,
        label: t.title || "Untitled",
        hint: t.pinned ? "Pinned" : undefined,
        group: "Recent chats",
        icon: MessageSquare,
        run: () => {
          selectThread(t.id);
          setNav("chat");
          onOpenChange(false);
        },
      }));

    return [
      {
        id: "pause-autonomy",
        label: "Pause / resume autonomy",
        group: "Actions",
        icon: Square,
        run: () => {
          const a = useGrokHub.getState().autonomy;
          useGrokHub.getState().pauseAutonomy(!a.paused);
          onOpenChange(false);
        },
      },
      {
        id: "stop",
        label: "Stop agent",
        hint: "Esc",
        group: "Actions",
        icon: Square,
        run: () => {
          stopChat();
          onOpenChange(false);
        },
      },
      {
        id: "export-thread",
        label: "Export current chat",
        group: "Actions",
        icon: FileDown,
        run: () => {
          const md = exportThreadMarkdown();
          const blob = new Blob([md], { type: "text/markdown" });
          const a = document.createElement("a");
          a.href = URL.createObjectURL(blob);
          a.download = `grokhub-chat-${Date.now()}.md`;
          a.click();
          URL.revokeObjectURL(a.href);
          onOpenChange(false);
        },
      },
      {
        id: "context-stats",
        label: "Context budget",
        group: "Actions",
        icon: Search,
        run: () => {
          const r = getContextStats();
          useGrokHub.getState().pushActivity({
            kind: "system",
            title: "Context",
            detail: r.report.slice(0, 200),
            status: "success",
          });
          onOpenChange(false);
        },
      },
      {
        id: "shortcuts",
        label: "Keyboard shortcuts",
        hint: "Ctrl+/",
        group: "Actions",
        icon: Keyboard,
        run: () => {
          window.dispatchEvent(new CustomEvent("grokhub:open-shortcuts"));
          onOpenChange(false);
        },
      },
...navItems, ...recent];
  }, [
    threads,
    setNav,
    setMode,
    newThread,
    selectThread,
    setUiTheme,
    onOpenChange,
    automations,
    compactThread,
    stopChat,
    exportThreadMarkdown,
    getContextStats,
    checkUpdateQuiet,
    runAutomation,
    pinWorkItem,
  ]);

  const groups = useMemo(() => {
    const map = new Map<string, Item[]>();
    for (const it of items) {
      const list = map.get(it.group) || [];
      list.push(it);
      map.set(it.group, list);
    }
    return [...map.entries()];
  }, [items]);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent showClose={false} className="overflow-hidden p-0 top-[18%]">
        <DialogHeader className="sr-only">
          <DialogTitle>Command palette</DialogTitle>
          <DialogDescription>Jump to pages, modes, and chats</DialogDescription>
        </DialogHeader>
        <Command
          label="Command palette"
          className="flex flex-col"
          shouldFilter
        >
          <div className="flex items-center gap-2 border-b border-[var(--color-border)] px-3">
            <Search className="h-4 w-4 shrink-0 text-[var(--color-subtle)]" />
            <Command.Input
              value={q}
              onValueChange={setQ}
              placeholder="Search commands, chats, modes…"
              className="h-11 w-full bg-transparent text-sm text-[var(--color-fg)] outline-none placeholder:text-[var(--color-subtle)]"
            />
            <kbd className="hidden rounded border border-[var(--color-border)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--color-subtle)] sm:inline">
              Esc
            </kbd>
          </div>
          <Command.List className="max-h-[min(50vh,22rem)] overflow-y-auto p-2">
            <Command.Empty className="px-3 py-8 text-center text-sm text-[var(--color-muted)]">
              No matches
            </Command.Empty>
            {groups.map(([group, list]) => (
              <Command.Group
                key={group}
                heading={group}
                className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1.5 [&_[cmdk-group-heading]]:text-[10px] [&_[cmdk-group-heading]]:font-semibold [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wide [&_[cmdk-group-heading]]:text-[var(--color-subtle)]"
              >
                {list.map((it) => {
                  const Icon = it.icon;
                  return (
                    <Command.Item
                      key={it.id}
                      value={`${it.label} ${it.hint || ""} ${it.group}`}
                      onSelect={() => it.run()}
                      className={cn(
                        "flex cursor-pointer items-center gap-2 rounded-[var(--radius-sm)] px-2.5 py-2 text-sm text-[var(--color-fg)]",
                        "data-[selected=true]:bg-[var(--color-elevated)]",
                      )}
                    >
                      <Icon className="h-4 w-4 shrink-0 text-[var(--color-muted)]" />
                      <span className="min-w-0 flex-1 truncate">{it.label}</span>
                      {it.hint ? (
                        <span className="shrink-0 font-mono text-[10px] text-[var(--color-subtle)]">
                          {it.hint}
                        </span>
                      ) : null}
                    </Command.Item>
                  );
                })}
              </Command.Group>
            ))}
          </Command.List>
          <div className="border-t border-[var(--color-border)] px-3 py-2 text-[10px] text-[var(--color-subtle)]">
            <span className="font-mono">Ctrl+K</span> palette · <span className="font-mono">Ctrl+N</span> new
            chat · <span className="font-mono">Ctrl+L</span> focus
          </div>
        </Command>
      </DialogContent>
    </Dialog>
  );
}
