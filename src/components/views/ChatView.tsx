import {
  Loader2,
  MessageSquarePlus,
  Download,
  Pencil,
  Paperclip,
  Send,
  Square,
  Sparkles,
  Terminal,
  Compass,
  Gauge,
  ShieldAlert,
  X,
  Cable,
  Wrench,
  Monitor,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { buildQuickChips, type QuickChip } from "@/lib/quick-assistant";
import { useGrokHub } from "@/lib/store";
import { parseStreamStatus } from "@/lib/tool-status";
import { formatUnits, PLAN_LIMITS, usagePercent } from "@/lib/usage";
import { cn } from "@/lib/utils";
import { RelativeTime } from "../RelativeTime";
import { HostGatewayBanner } from "../HostGatewayBanner";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Textarea } from "../ui/textarea";
import { MarkdownBody } from "@/lib/markdown";

function chipIcon(kind: QuickChip["kind"]) {
  if (kind === "shell") return Terminal;
  if (kind === "nav") return Compass;
  if (kind === "mode") return Gauge;
  return Sparkles;
}

function ToolActivityBanner({ status }: { status: string | null }) {
  const tool = parseStreamStatus(status);
  if (!tool || tool.phase !== "running") return null;
  const Icon =
    tool.kind === "host"
      ? Monitor
      : tool.kind === "connector"
        ? Cable
        : tool.kind === "selfmod"
          ? Wrench
          : tool.kind === "summarize"
            ? Sparkles
            : Loader2;
  const accent =
    tool.kind === "host"
      ? "border-[color-mix(in_oklab,var(--color-success)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-success)_10%,var(--color-surface))]"
      : tool.kind === "connector"
        ? "border-[color-mix(in_oklab,var(--color-info)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-info)_10%,var(--color-surface))]"
        : tool.kind === "selfmod"
          ? "border-[color-mix(in_oklab,var(--color-warn)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,var(--color-surface))]"
          : "border-[var(--color-border)] bg-[var(--color-elevated)]";
  return (
    <div
      className={cn(
        "mx-auto flex w-full max-w-[min(56rem,100%)] items-start gap-3 rounded-[var(--radius-md)] border px-3 py-2.5 3xl:max-w-[min(64rem,100%)]",
        accent,
      )}
      role="status"
      aria-live="polite"
    >
      <span className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-[var(--color-surface)]">
        <Icon
          className={cn(
            "h-3.5 w-3.5",
            (tool.kind === "stream" || tool.kind === "summarize") &&
              "animate-spin text-[var(--color-info)]",
            tool.kind === "host" && "text-[var(--color-success)]",
            tool.kind === "connector" && "text-[var(--color-info)]",
            tool.kind === "selfmod" && "text-[var(--color-warn)]",
            tool.kind !== "stream" &&
              tool.kind !== "summarize" &&
              tool.kind !== "host" &&
              tool.kind !== "connector" &&
              tool.kind !== "selfmod" &&
              "text-[var(--color-info)]",
          )}
        />
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs font-semibold text-[var(--color-fg)]">{tool.title}</span>
          <span className="inline-flex items-center gap-1 rounded-full border border-[var(--color-border)] px-1.5 py-px text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
            <span className="pulse-live inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-info)]" />
            live
          </span>
        </div>
        <p className="mt-0.5 break-all font-mono text-[11px] leading-snug text-[var(--color-muted)]">
          {tool.detail}
        </p>
      </div>
      <Loader2 className="mt-1 h-3.5 w-3.5 shrink-0 animate-spin text-[var(--color-subtle)]" />
    </div>
  );
}

const MAX_ATTACH_BYTES = 1_200_000; // ~1.2MB data url budget each

async function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result || ""));
    reader.onerror = () => reject(reader.error || new Error("read failed"));
    reader.readAsDataURL(file);
  });
}

export function ChatView() {
  const chat = useGrokHub((s) => s.chat);
  const sendChat = useGrokHub((s) => s.sendChat);
  const stopChat = useGrokHub((s) => s.stopChat);
  const running = useGrokHub((s) => s.running);
  const streamStatus = useGrokHub((s) => s.streamStatus);
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
  const setNav = useGrokHub((s) => s.setNav);
  const pushActivity = useGrokHub((s) => s.pushActivity);
  const recordUsage = useGrokHub((s) => s.recordUsage);
  const usage = useGrokHub((s) => s.usage);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const newThread = useGrokHub((s) => s.newThread);
  const activity = useGrokHub((s) => s.activity);
  const threads = useGrokHub((s) => s.threads);
  const connectors = useGrokHub((s) => s.connectors);
  const pendingHostConfirm = useGrokHub((s) => s.pendingHostConfirm);
  const resolveHostConfirm = useGrokHub((s) => s.resolveHostConfirm);
  const quickAssistMemory = useGrokHub((s) => s.quickAssistMemory);
  const recordQuickAssistChip = useGrokHub((s) => s.recordQuickAssistChip);
  const recordQuickAssistTyped = useGrokHub((s) => s.recordQuickAssistTyped);
  const sessionResume = useGrokHub((s) => s.sessionResume);
  const resumeLastSession = useGrokHub((s) => s.resumeLastSession);
  const dismissSessionResume = useGrokHub((s) => s.dismissSessionResume);
  const exportThreadMarkdown = useGrokHub((s) => s.exportThreadMarkdown);
  const editChatMessage = useGrokHub((s) => s.editChatMessage);
  const clearChat = useGrokHub((s) => s.clearChat);
  const [text, setText] = useState("");
  const [localRunning, setLocalRunning] = useState(false);
  const [pendingBusy, setPendingBusy] = useState(false);
  const [hostOnline, setHostOnline] = useState<boolean | undefined>(undefined);
  const [historyExtra, setHistoryExtra] = useState(0);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editDraft, setEditDraft] = useState("");
  const [attachments, setAttachments] = useState<Array<{ name: string; dataUrl: string; kind: string }>>([]);
  const endRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const modeMeta = getMode(mode);
  const busy = running || localRunning || pendingBusy;
  const plan = PLAN_LIMITS[usage.plan];
  const pct = Math.round(usagePercent(usage));

  const WINDOW = 40;
  const visibleChat = useMemo(() => {
    const take = WINDOW + historyExtra;
    if (chat.length <= take) return chat;
    return chat.slice(chat.length - take);
  }, [chat, historyExtra]);
  const hiddenCount = Math.max(0, chat.length - visibleChat.length);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      try {
        const { hostInfo } = await import("@/lib/host-client");
        const i = await hostInfo();
        if (!cancelled) {
          setHostOnline(i.bridge !== "none" && Boolean(i.unsandboxed));
        }
      } catch {
        if (!cancelled) setHostOnline(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // Clear pending busy once store running takes over
  useEffect(() => {
    if (running) setPendingBusy(false);
  }, [running]);

  const chips = useMemo(
    () =>
      buildQuickChips({
        chat,
        activity,
        threads,
        connectors,
        mode,
        grokConnected,
        usage,
        draft: text,
        hostOnline,
        memory: quickAssistMemory,
        max: text.trim().length > 0 ? 10 : Math.min(10, Math.max(4, 4 + Math.min(chat.length, 4))),
      }),
    [chat, activity, threads, connectors, mode, grokConnected, usage, text, hostOnline, quickAssistMemory],
  );

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat, busy, streamStatus]);

  useEffect(() => {
    const focus = () => {
      inputRef.current?.focus();
      resizeComposer();
    };
    window.addEventListener("grokhub:focus-chat-input", focus);
    return () => window.removeEventListener("grokhub:focus-chat-input", focus);
  }, []);

  /** Grow/shrink the composer with content (single line → multi-line). */
  function resizeComposer(el?: HTMLTextAreaElement | null) {
    const ta = el ?? inputRef.current;
    if (!ta) return;
    ta.style.height = "0px";
    const min = 40; // ~2.5rem single line
    const max = 160; // ~max-h-40
    const next = Math.min(max, Math.max(min, ta.scrollHeight));
    ta.style.height = `${next}px`;
    ta.style.overflowY = ta.scrollHeight > max ? "auto" : "hidden";
  }

  useEffect(() => {
    resizeComposer();
  }, [text, busy]);

  async function addFiles(files: FileList | File[]) {
    const list = Array.from(files).slice(0, 4);
    const next: typeof attachments = [];
    for (const f of list) {
      if (f.size > MAX_ATTACH_BYTES) {
        pushActivity({
          kind: "system",
          title: "Attachment too large",
          detail: `${f.name} — keep under ~1MB`,
          status: "failed",
        });
        continue;
      }
      try {
        const dataUrl = await fileToDataUrl(f);
        if (dataUrl.length > MAX_ATTACH_BYTES * 1.4) {
          pushActivity({
            kind: "system",
            title: "Attachment too large",
            detail: f.name,
            status: "failed",
          });
          continue;
        }
        next.push({
          name: f.name,
          dataUrl,
          kind: f.type || "application/octet-stream",
        });
      } catch {
        /* skip */
      }
    }
    if (next.length) setAttachments((a) => [...a, ...next].slice(0, 6));
  }

  async function runShell(command: string) {
    setLocalRunning(true);
    const userLine = command.startsWith("$") ? command : `$ ${command}`;
    useGrokHub.setState((s) => {
      const chat = [
        ...s.chat,
        {
          id: `u_${Date.now()}`,
          role: "user" as const,
          content: userLine,
          ts: Date.now(),
          mode,
        },
      ];
      const threads = s.threads.map((t) =>
        t.id === s.activeThreadId
          ? { ...t, messages: chat, updatedAt: Date.now() }
          : t,
      );
      return { chat, threads, running: true, streamStatus: "Host running…" };
    });
    try {
      const bill = recordUsage("host");
      if (!bill.ok) {
        useGrokHub.setState({ running: false, streamStatus: null });
        return;
      }
      const { hostExec } = await import("@/lib/host-client");
      const r = await hostExec(command.replace(/^\$\s*/, "").replace(/^\/sh\s+/, ""));
      const out = [
        r.ok ? "```" : "```text",
        `$ ${r.command || command}`,
        r.stdout || "",
        r.stderr ? `\n[stderr]\n${r.stderr}` : "",
        "```",
        `exit ${r.code} · ${r.ms}ms`,
      ]
        .filter(Boolean)
        .join("\n");
      useGrokHub.setState((s) => {
        const chat = [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "assistant" as const,
            content: out,
            ts: Date.now(),
            mode,
          },
        ];
        const threads = s.threads.map((t) =>
          t.id === s.activeThreadId ? { ...t, messages: chat, updatedAt: Date.now() } : t,
        );
        return { chat, threads, running: false, streamStatus: null };
      });
    } catch (e) {
      useGrokHub.setState((s) => ({
        chat: [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "system" as const,
            content: e instanceof Error ? e.message : "host failed",
            ts: Date.now(),
          },
        ],
        running: false,
        streamStatus: null,
      }));
    } finally {
      setLocalRunning(false);
    }
  }

  async function onChip(chip: QuickChip) {
    recordQuickAssistChip(chip);
    if (chip.kind === "nav" && chip.value.startsWith("__nav:")) {
      setNav(chip.value.slice("__nav:".length) as "settings" | "imagine" | "connectors" | "desktop");
      return;
    }
    if (chip.kind === "mode" && chip.value.startsWith("__mode:")) {
      const m = chip.value.slice("__mode:".length) as "auto" | "fast" | "expert" | "heavy" | "build";
      setMode(m);
      return;
    }
    if (chip.kind === "shell" || chip.value.startsWith("$") || chip.value.startsWith("/sh ")) {
      setText("");
      await runShell(chip.value);
      return;
    }
    setText("");
    setPendingBusy(true);
    await sendChat(chip.value);
    setPendingBusy(false);
  }

  async function onSend(value?: string) {
    if (busy) return;
    let payload = (value ?? text).trim();
    if (attachments.length) {
      const blocks = attachments.map((a) => {
        if (a.kind.startsWith("image/")) {
          return `![${a.name}](${a.dataUrl})`;
        }
        return `Attached file: **${a.name}** (\`${a.kind}\`)\n\n\`\`\`\n${a.dataUrl.slice(0, 200)}…\n\`\`\``;
      });
      payload = [payload, ...blocks].filter(Boolean).join("\n\n");
    }
    if (!payload) return;
    recordQuickAssistTyped(payload);
    if (
      payload.toLowerCase().includes("imagine") &&
      !payload.startsWith("/") &&
      !payload.startsWith("$")
    ) {
      setNav("imagine");
    }
    setText("");
    setAttachments([]);
    if (payload.startsWith("$") || payload.startsWith("/sh ")) {
      await runShell(payload);
      return;
    }
    // Instant local busy so Stop / spinner show before store rehydrate lag
    setPendingBusy(true);
    try {
      await sendChat(payload);
    } finally {
      setPendingBusy(false);
    }
  }

  function onStop() {
    setPendingBusy(false);
    if (localRunning) {
      setLocalRunning(false);
      useGrokHub.setState({ running: false, streamStatus: null });
      return;
    }
    stopChat();
  }

  return (
    <div className="chat-stage mx-auto flex h-full min-h-0 w-full flex-col gap-3">
      <Card className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0 border-b border-[var(--color-border)] px-4 py-3 md:px-6 3xl:px-8">
          <div className="flex items-start justify-between gap-3">
            <div>
              <CardTitle className="text-sm">Agent</CardTitle>
              <CardDescription>
                Live Grok · <span className="font-mono">$</span> host shell · attach files · History in the
                sidebar
              </CardDescription>
            </div>
            <div className="flex flex-col items-end gap-1">
              <div className="flex gap-1">
                <Button size="sm" variant="secondary" onClick={() => newThread()} disabled={busy} title="Ctrl+N">
                  <MessageSquarePlus className="h-3.5 w-3.5" />
                  New
                </Button>
                <Button
                  size="sm"
                  variant="secondary"
                  disabled={!chat.length}
                  title="Export chat as Markdown"
                  onClick={() => {
                    const md = exportThreadMarkdown();
                    const blob = new Blob([md], { type: "text/markdown;charset=utf-8" });
                    const a = document.createElement("a");
                    a.href = URL.createObjectURL(blob);
                    a.download = `grokhub-chat-${Date.now()}.md`;
                    a.click();
                    URL.revokeObjectURL(a.href);
                  }}
                >
                  <Download className="h-3.5 w-3.5" />
                  Export
                </Button>
                <Badge className="font-mono">{modeMeta.label}</Badge>
              </div>
              <Badge variant={grokConnected ? "success" : "default"} className="text-[10px]">
                {grokConnected ? "Grok live" : "Connect in Settings"}
              </Badge>
              <span className="text-[10px] tabular text-[var(--color-subtle)]">
                {formatUnits(usage.usedUnits)}/{formatUnits(plan.units)} · {pct}%
              </span>
            </div>
          </div>
        </CardHeader>
        <CardContent className="flex min-h-0 flex-1 flex-col gap-3 overflow-hidden p-0">
          <div className="shrink-0 space-y-2 px-4 pt-3 md:px-6 3xl:px-8">
            <HostGatewayBanner variant="compact" />
            {sessionResume && sessionResume.threadId && !busy && (
              <div className="flex flex-wrap items-center justify-between gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 py-2">
                <div className="min-w-0">
                  <div className="text-xs font-medium">Continue where you left off</div>
                  <div className="truncate text-[11px] text-[var(--color-muted)]">
                    {sessionResume.title}
                    {sessionResume.preview ? ` — ${sessionResume.preview}` : ""}
                  </div>
                </div>
                <div className="flex shrink-0 gap-1.5">
                  <Button size="sm" onClick={() => resumeLastSession()}>
                    Resume
                  </Button>
                  <Button size="sm" variant="ghost" onClick={() => dismissSessionResume()}>
                    Dismiss
                  </Button>
                </div>
              </div>
            )}
          </div>
          <div className="scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-6 3xl:px-10 uw:px-16">
            {chat.length === 0 && !busy && (
              <div className="flex flex-col items-center justify-center gap-2 py-16 text-center">
                <p className="text-sm font-medium text-[var(--color-fg)]">Start a conversation</p>
                <p className="max-w-sm text-xs text-[var(--color-muted)]">
                  Ask anything, run <span className="font-mono">$ shell</span> commands, or try{" "}
                  <span className="font-mono">/help</span> for slash commands.
                </p>
              </div>
            )}
            {hiddenCount > 0 && (
              <div className="flex justify-center">
                <Button
                  size="sm"
                  variant="secondary"
                  onClick={() => setHistoryExtra((n) => n + WINDOW)}
                >
                  Show {Math.min(WINDOW, hiddenCount)} earlier · {hiddenCount} hidden
                </Button>
              </div>
            )}
            {visibleChat.map((m) => (
              <div
                key={m.id}
                className={cn("flex", m.role === "user" ? "justify-end" : "justify-start")}
              >
                <div
                  className={cn(
                    "chat-bubble rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed",
                    m.role === "user"
                      ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                      : m.role === "system"
                        ? "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]"
                        : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)]",
                    m.streaming &&
                      "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]",
                  )}
                >
                  <div className="mb-1 flex flex-wrap items-center gap-2 text-[10px] uppercase tracking-wide text-[var(--color-subtle)]">
                    <span>
                      {m.role} · <RelativeTime ts={m.ts} />
                    </span>
                    {m.mode && (
                      <span className="rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case">
                        {getMode(m.mode).label}
                      </span>
                    )}
                    {m.streaming && (
                      <span className="inline-flex items-center gap-1 rounded border border-[color-mix(in_oklab,var(--color-info)_40%,transparent)] px-1.5 py-px font-mono normal-case text-[var(--color-info)]">
                        <Loader2 className="h-2.5 w-2.5 animate-spin" />
                        {streamStatus?.startsWith("Host")
                          ? "host"
                          : streamStatus?.startsWith("Connector")
                            ? "connector"
                            : streamStatus?.includes("Summariz")
                              ? "summarizing"
                              : "streaming"}
                      </span>
                    )}
                    {m.stopped && (
                      <span className="rounded border border-[var(--color-border)] px-1.5 py-px font-mono normal-case text-[var(--color-warn)]">
                        stopped
                      </span>
                    )}
                  </div>
                  {m.content ? (
                    m.role === "user" ? (
                      editingId === m.id ? (
                        <div className="space-y-2">
                          <textarea
                            value={editDraft}
                            onChange={(e) => setEditDraft(e.target.value)}
                            className="min-h-[4rem] w-full rounded border border-[var(--color-border-strong)] bg-[var(--color-surface)] p-2 text-sm"
                            autoFocus
                          />
                          <div className="flex gap-1.5">
                            <Button
                              size="sm"
                              onClick={() => {
                                void editChatMessage(m.id, editDraft, true);
                                setEditingId(null);
                              }}
                            >
                              Save & resend
                            </Button>
                            <Button
                              size="sm"
                              variant="secondary"
                              onClick={() => {
                                void editChatMessage(m.id, editDraft, false);
                                setEditingId(null);
                              }}
                            >
                              Save
                            </Button>
                            <Button size="sm" variant="ghost" onClick={() => setEditingId(null)}>
                              Cancel
                            </Button>
                          </div>
                        </div>
                      ) : (
                        <div className="group/user relative">
                          <div className="whitespace-pre-wrap">
                            <MarkdownBody content={m.content} />
                          </div>
                          {!busy && (
                            <button
                              type="button"
                              className="absolute -right-1 -top-1 rounded border border-[var(--color-border)] bg-[var(--color-surface)] p-1 opacity-0 transition-opacity group-hover/user:opacity-100"
                              title="Edit message"
                              onClick={() => {
                                setEditingId(m.id);
                                setEditDraft(m.content);
                              }}
                            >
                              <Pencil className="h-3 w-3" />
                            </button>
                          )}
                        </div>
                      )
                    ) : (
                      <MarkdownBody content={m.content} streaming={Boolean(m.streaming)} />
                    )
                  ) : m.streaming ? (
                    <span className="inline-flex items-center gap-1.5 text-[var(--color-subtle)]">
                      <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-info)]" />
                      Thinking…
                    </span>
                  ) : (
                    ""
                  )}
                </div>
              </div>
            ))}
            {busy && (
              <div className="space-y-2">
                <ToolActivityBanner
                  status={
                    streamStatus || (localRunning ? "Host: running…" : "Thinking…")
                  }
                />
                <div className="flex items-center gap-2 text-xs text-[var(--color-subtle)]">
                  <Loader2 className="h-3.5 w-3.5 animate-spin text-[var(--color-info)]" />
                  <span className="shimmer rounded px-1">
                    {streamStatus ||
                      (localRunning ? "Host running…" : `${modeMeta.label} · working…`)}
                  </span>
                </div>
              </div>
            )}
            <div ref={endRef} />
          </div>

          <div className="shrink-0 space-y-2 border-t border-[var(--color-border)] p-3 md:p-4 3xl:px-8 uw:px-12">
            {!busy && chips.length > 0 && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]">
                <div className="mb-1 flex items-center justify-center gap-2 px-0.5">
                  <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                    Quick assist
                  </span>
                </div>
                <div
                  className="flex flex-wrap items-center justify-center gap-1.5"
                  role="listbox"
                  aria-label="Quick assistant suggestions"
                >
                  {chips.map((c) => {
                    const Icon = chipIcon(c.kind);
                    return (
                      <button
                        key={c.id}
                        type="button"
                        role="option"
                        disabled={busy}
                        title={c.value.startsWith("__") ? c.label : c.value}
                        onClick={() => void onChip(c)}
                        className={cn(
                          "inline-flex max-w-[14rem] items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs transition-colors",
                          "border-[var(--color-border)] text-[var(--color-muted)]",
                          "hover:border-[var(--color-border-strong)] hover:bg-[var(--color-elevated)] hover:text-[var(--color-fg)]",
                          "disabled:opacity-50",
                          c.kind === "shell" && "font-mono",
                        )}
                      >
                        <Icon className="h-3 w-3 shrink-0 opacity-70" />
                        <span className="truncate">{c.label}</span>
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            {pendingHostConfirm && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] rounded-[var(--radius-md)] border border-[color-mix(in_oklab,var(--color-warn)_45%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-warn)_10%,var(--color-surface))] p-3 3xl:max-w-[min(64rem,100%)]">
                <div className="mb-2 flex items-center gap-2 text-sm font-medium text-[var(--color-fg)]">
                  <ShieldAlert className="h-4 w-4 text-[var(--color-warn)]" />
                  Allow host commands?
                </div>
                <ul className="mb-3 space-y-1 font-mono text-xs text-[var(--color-muted)]">
                  {pendingHostConfirm.cmds.map((c, i) => (
                    <li key={c + i} className="break-all">
                      <span className="text-[var(--color-subtle)]">
                        [{pendingHostConfirm.risks[i] || "run"}]
                      </span>{" "}
                      $ {c}
                    </li>
                  ))}
                </ul>
                <div className="flex gap-2">
                  <Button size="sm" onClick={() => resolveHostConfirm(true)}>
                    Run on this machine
                  </Button>
                  <Button size="sm" variant="secondary" onClick={() => resolveHostConfirm(false)}>
                    Cancel
                  </Button>
                </div>
              </div>
            )}

            {attachments.length > 0 && (
              <div className="mx-auto flex w-full max-w-[min(56rem,100%)] flex-wrap gap-2">
                {attachments.map((a, i) => (
                  <div
                    key={a.name + i}
                    className="relative flex items-center gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1.5"
                  >
                    {a.kind.startsWith("image/") ? (
                      <img src={a.dataUrl} alt="" className="h-10 w-10 rounded object-cover" />
                    ) : (
                      <span className="font-mono text-[10px]">{a.kind}</span>
                    )}
                    <span className="max-w-[8rem] truncate text-[11px]">{a.name}</span>
                    <button
                      type="button"
                      className="rounded p-0.5 text-[var(--color-muted)] hover:text-[var(--color-fg)]"
                      onClick={() => setAttachments((list) => list.filter((_, j) => j !== i))}
                    >
                      <X className="h-3 w-3" />
                    </button>
                  </div>
                ))}
              </div>
            )}

            <form
              className="mx-auto flex w-full max-w-[min(56rem,100%)] gap-2 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]"
              onSubmit={(e) => {
                e.preventDefault();
                if (busy) {
                  onStop();
                  return;
                }
                void onSend();
              }}
            >
              <input
                ref={fileRef}
                type="file"
                className="hidden"
                multiple
                accept="image/*,.txt,.md,.json,.csv,.log,.ts,.tsx,.js,.py"
                onChange={(e) => {
                  if (e.target.files?.length) void addFiles(e.target.files);
                  e.target.value = "";
                }}
              />
              <Button
                type="button"
                size="icon"
                variant="secondary"
                disabled={busy}
                title="Attach file or screenshot"
                onClick={() => fileRef.current?.click()}
              >
                <Paperclip className="h-4 w-4" />
              </Button>
              <Textarea
                ref={inputRef}
                value={text}
                onChange={(e) => {
                  setText(e.target.value);
                  resizeComposer(e.target);
                }}
                placeholder={
                  busy
                    ? "Agent running — press Stop to interrupt…"
                    : "Message Grok…  /help · Enter send · Ctrl+L focus · paste image · $ shell"
                }
                rows={1}
                className="max-h-40 min-h-[2.5rem] flex-1 resize-none overflow-hidden leading-5"
                style={{ height: 40 }}
                onPaste={(e) => {
                  const items = e.clipboardData?.items;
                  if (!items) return;
                  const files: File[] = [];
                  for (const it of items) {
                    if (it.kind === "file") {
                      const f = it.getAsFile();
                      if (f) files.push(f);
                    }
                  }
                  if (files.length) {
                    e.preventDefault();
                    void addFiles(files);
                  }
                  // text paste still fires onChange; resize after paint
                  requestAnimationFrame(() => resizeComposer());
                }}
                onInput={(e) => resizeComposer(e.currentTarget)}
                onKeyDown={(e) => {
                  if (e.key === "Escape" && busy) {
                    e.preventDefault();
                    onStop();
                    return;
                  }
                  if (e.key === "Enter" && !e.shiftKey && !busy) {
                    e.preventDefault();
                    void onSend();
                  }
                }}
              />
              {busy ? (
                <Button
                  type="button"
                  variant="secondary"
                  size="icon"
                  onClick={onStop}
                  aria-label="Stop"
                  title="Stop (Esc)"
                  className="border border-[color-mix(in_oklab,var(--color-danger)_40%,transparent)] text-[var(--color-danger)]"
                >
                  <Square className="h-3.5 w-3.5 fill-current" />
                </Button>
              ) : (
                <Button
                  type="submit"
                  disabled={!text.trim() && attachments.length === 0}
                  size="icon"
                  aria-label="Send"
                >
                  <Send className="h-4 w-4" />
                </Button>
              )}
            </form>
            {!busy && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] text-center text-[10px] text-[var(--color-subtle)]">
                <span className="font-mono">/help</span> commands · <span className="font-mono">Ctrl+N</span> new ·{" "}
                <span className="font-mono">Ctrl+L</span> focus · attach or paste images
              </div>
            )}
            {busy && (
              <ToolActivityBanner status={streamStatus || "Working…"} />
            )}
            {busy && (
              <div className="mx-auto flex w-full max-w-[min(56rem,100%)] items-center justify-between text-[10px] text-[var(--color-subtle)] 3xl:max-w-[min(64rem,100%)]">
                <span className="inline-flex items-center gap-1.5">
                  <span className="pulse-live inline-block h-1.5 w-1.5 rounded-full bg-[var(--color-info)]" />
                  {streamStatus || "Running"}
                </span>
                <button
                  type="button"
                  className="font-medium text-[var(--color-danger)] hover:underline"
                  onClick={onStop}
                >
                  Stop generating
                </button>
              </div>
            )}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
