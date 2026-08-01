import { MessageSquarePlus, Send } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { getMode } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import { formatUnits, PLAN_LIMITS, usagePercent } from "@/lib/usage";
import { cn } from "@/lib/utils";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

const SUGGESTIONS = [
  "What can you help me with?",
  "$ uname -a",
  "What's my usage?",
  "Explain my modes",
];

export function ChatView() {
  const chat = useGrokHub((s) => s.chat);
  const sendChat = useGrokHub((s) => s.sendChat);
  const running = useGrokHub((s) => s.running);
  const mode = useGrokHub((s) => s.mode);
  const setNav = useGrokHub((s) => s.setNav);
  const pushActivity = useGrokHub((s) => s.pushActivity);
  const recordUsage = useGrokHub((s) => s.recordUsage);
  const usage = useGrokHub((s) => s.usage);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const newThread = useGrokHub((s) => s.newThread);
  const [text, setText] = useState("");
  const [localRunning, setLocalRunning] = useState(false);
  const endRef = useRef<HTMLDivElement>(null);
  const modeMeta = getMode(mode);
  const busy = running || localRunning;
  const plan = PLAN_LIMITS[usage.plan];
  const pct = Math.round(usagePercent(usage));

  useEffect(() => {
    endRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [chat, busy]);

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
      return { chat, threads };
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
      useGrokHub.setState((s) => ({
        chat: [
          ...s.chat,
          {
            id: `a_${Date.now()}`,
            role: "assistant",
            content: `Host exec error: ${e instanceof Error ? e.message : "failed"}`,
            ts: Date.now(),
            mode,
          },
        ],
      }));
    } finally {
      setLocalRunning(false);
    }
  }

  async function onSend(value?: string) {
    const payload = (value ?? text).trim();
    if (!payload || busy) return;
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

  return (
    <div className="mx-auto flex h-full min-h-0 max-w-3xl flex-col gap-3">
      <Card className="flex min-h-0 flex-1 flex-col overflow-hidden">
        <CardHeader className="shrink-0 border-b border-[var(--color-border)]">
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
                <Button size="sm" variant="secondary" onClick={() => newThread()}>
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
          <div className="scroll-panel min-h-0 flex-1 space-y-3 px-4 py-4 md:px-5">
            {chat.map((m) => (
              <div
                key={m.id}
                className={cn("flex", m.role === "user" ? "justify-end" : "justify-start")}
              >
                <div
                  className={cn(
                    "max-w-[92%] rounded-[var(--radius-lg)] border px-3.5 py-2.5 text-sm leading-relaxed whitespace-pre-wrap",
                    m.role === "user"
                      ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                      : m.role === "system"
                        ? "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-muted)]"
                        : "border-[var(--color-border)] bg-[var(--color-surface)] text-[var(--color-fg)]",
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
                  </div>
                  {m.content}
                </div>
              </div>
            ))}
            {busy && (
              <div className="text-xs text-[var(--color-subtle)]">
                <span className="shimmer rounded px-1">
                  {localRunning ? "Host running…" : `${modeMeta.label} · Grok thinking…`}
                </span>
              </div>
            )}
            <div ref={endRef} />
          </div>

          <div className="shrink-0 space-y-2 border-t border-[var(--color-border)] p-3 md:p-4">
            <div className="flex flex-wrap gap-1.5">
              {SUGGESTIONS.map((s) => (
                <button
                  key={s}
                  type="button"
                  disabled={busy}
                  onClick={() => void onSend(s)}
                  className="rounded-full border border-[var(--color-border)] px-2.5 py-1 text-xs text-[var(--color-muted)] transition-colors hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
                >
                  {s}
                </button>
              ))}
            </div>
            <form
              className="flex gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void onSend();
              }}
            >
              <Input
                value={text}
                onChange={(e) => setText(e.target.value)}
                placeholder="Message Grok… or $ shell"
                disabled={busy}
                className="flex-1"
              />
              <Button type="submit" disabled={busy || !text.trim()} size="icon">
                <Send className="h-4 w-4" />
              </Button>
            </form>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
