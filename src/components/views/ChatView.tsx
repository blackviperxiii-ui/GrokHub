import { Loader2, MessageSquarePlus, Send, Square, Sparkles, Terminal, Compass, Gauge, ShieldAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { buildQuickChips, type QuickChip } from "@/lib/quick-assistant";
import { useGrokHub } from "@/lib/store";
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
  const [text, setText] = useState("");
  const [localRunning, setLocalRunning] = useState(false);
  const [hostOnline, setHostOnline] = useState<boolean | undefined>(undefined);
  const endRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const modeMeta = getMode(mode);
  const busy = running || localRunning;
  const plan = PLAN_LIMITS[usage.plan];
  const pct = Math.round(usagePercent(usage));

  // Lightweight host presence for chips (non-blocking)
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
        // 4 when empty draft + idle; grow with context up to 10
        max: text.trim().length > 0 ? 10 : Math.min(10, Math.max(4, 4 + Math.min(chat.length, 4))),
      }),
    [chat, activity, threads, connectors, mode, grokConnected, usage, text, hostOnline],
  );

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat, busy, streamStatus]);

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
        useGrokHub.setState((s) => ({
          chat: [
            ...s.chat,
            {
              id: `a_${Date.now()}`,
              role: "system",
              content: `Host blocked: ${plan.label} unit quota exhausted.`,
              ts: Date.now(),
            },
          ],
        }));
        return;
      }
      const cmd = command.replace(/^\$\s*/, "").replace(/^\/sh\s+/, "").trim();
      const { hostExec } = await import("@/lib/host-client");
      const r = await hostExec(cmd);
      const body = [
        `[Desktop host · ${r.ok ? "ok" : "fail"} · exit ${r.code ?? "?"} · ${bill.cost}u]`,
        `cwd: ${r.cwd}`,
        "",
        r.stdout || "(no stdout)",
        r.stderr ? `\n[stderr]\n${r.stderr}` : "",
      ]
        .filter(Boolean)
        .join("\n");
      useGrokHub.setState((s) => {
        const chat = [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "assistant" as const,
            content: body,
            ts: Date.now(),
            mode,
          },
        ];
        const threads = s.threads.map((t) =>
          t.id === s.activeThreadId
            ? { ...t, messages: chat, updatedAt: Date.now() }
            : t,
        );
        return { chat, threads };
      });
      pushActivity({
        kind: "desktop",
        title: r.ok ? "Host command ok" : "Host command failed",
        detail: `${cmd.slice(0, 100)} · ${bill.cost}u`,
        status: r.ok ? "success" : "failed",
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "host failed";
      useGrokHub.setState((s) => ({
        chat: [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "system" as const,
            content: `Desktop gateway offline: ${msg}\n\nConnect the host in Settings → Desktop host gateway so I can run shell/files/apps on your machine.`,
            ts: Date.now(),
          },
        ],
      }));
      setNav("settings");
    } finally {
      setLocalRunning(false);
      useGrokHub.setState({ running: false, streamStatus: null });
    }
  }

  async function onChip(chip: QuickChip) {
    if (busy) return;
    if (chip.kind === "nav" && chip.value.startsWith("__nav:")) {
      const nav = chip.value.slice("__nav:".length) as
        | "settings"
        | "imagine"
        | "desktop"
        | "chat";
      setNav(nav);
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
    await sendChat(chip.value);
  }

  async function onSend(value?: string) {
    if (busy) return;
    const payload = (value ?? text).trim();
    if (!payload) return;
    if (
      payload.toLowerCase().includes("imagine") &&
      !payload.startsWith("/") &&
      !payload.startsWith("$")
    ) {
      setNav("imagine");
    }
    setText("");
    if (payload.startsWith("$") || payload.startsWith("/sh ")) {
      await runShell(payload);
      return;
    }
    await sendChat(payload);
  }

  function onStop() {
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
                Live Grok · <span className="font-mono">$</span> host shell · History in the
                sidebar
              </CardDescription>
            </div>
            <div className="flex flex-col items-end gap-1">
              <div className="flex gap-1">
                <Button size="sm" variant="secondary" onClick={() => newThread()} disabled={busy}>
                  <MessageSquarePlus className="h-3.5 w-3.5" />
                  New
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
          <div className="shrink-0 px-4 pt-3 md:px-6 3xl:px-8">
            <HostGatewayBanner variant="compact" />
          </div>
          <div className="scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-6 3xl:px-10 uw:px-16">
            {chat.map((m) => (
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
                    m.streaming && "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]",
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
                        streaming
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
                      <div className="whitespace-pre-wrap">{m.content}</div>
                    ) : (
                      <MarkdownBody content={m.content} />
                    )
                  ) : m.streaming ? (
                    <span className="inline-flex items-center gap-1.5 text-[var(--color-subtle)]">
                      <span className="inline-block h-1.5 w-1.5 animate-pulse rounded-full bg-[var(--color-info)]" />
                      …
                    </span>
                  ) : (
                    ""
                  )}
                  {m.streaming && m.content ? (
                    <span className="ml-0.5 inline-block h-3 w-1.5 animate-pulse bg-[var(--color-fg)] align-middle opacity-70" />
                  ) : null}
                </div>
              </div>
            ))}
            {busy && (
              <div className="flex items-center gap-2 text-xs text-[var(--color-subtle)]">
                <Loader2 className="h-3.5 w-3.5 animate-spin text-[var(--color-info)]" />
                <span className="shimmer rounded px-1">
                  {streamStatus ||
                    (localRunning ? "Host running…" : `${modeMeta.label} · working…`)}
                </span>
              </div>
            )}
            <div ref={endRef} />
          </div>

          <div className="shrink-0 space-y-2 border-t border-[var(--color-border)] p-3 md:p-4 3xl:px-8 uw:px-12">
            {/* Quick assistant — predictive chips from recent activity */}
            {!busy && chips.length > 0 && (
              <div className="mx-auto w-full max-w-[min(56rem,100%)] 3xl:max-w-[min(64rem,100%)] uw:max-w-[min(72rem,100%)]">
                <div className="mb-1 flex items-center justify-center gap-2 px-0.5">
                  <span className="text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                    Quick assist
                  </span>
                  <span className="text-[10px] text-[var(--color-subtle)]">·</span>
                  <span className="text-[10px] text-[var(--color-subtle)]">
                    {chips.length} suggestion{chips.length === 1 ? "" : "s"}
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
                          c.score >= 80 && "border-[color-mix(in_oklab,var(--color-info)_35%,var(--color-border))]",
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
                      <span className="text-[var(--color-subtle)]">[{pendingHostConfirm.risks[i] || "run"}]</span>{" "}
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
              <Textarea
                ref={inputRef}
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder={
                  busy
                    ? "Agent running — press Stop to interrupt…"
                    : "Message Grok…  Enter to send · Shift+Enter for newline · $ shell"
                }
                rows={1}
                className="max-h-40 min-h-[2.5rem] flex-1 resize-y"
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
                <Button type="submit" disabled={!text.trim()} size="icon" aria-label="Send">
                  <Send className="h-4 w-4" />
                </Button>
              )}
            </form>
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
