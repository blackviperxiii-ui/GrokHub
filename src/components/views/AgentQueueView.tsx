import {
  CheckCircle2,
  CirclePause,
  ListTodo,
  Play,
  ShieldAlert,
  Trash2,
  XCircle,
} from "lucide-react";
import { useMemo } from "react";
import {
  AUTONOMY_HINT,
  AUTONOMY_LABEL,
  queueStats,
  type AutonomyLevel,
} from "@/lib/agent-jobs";
import { TOOL_REGISTRY } from "@/lib/tool-registry";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";

export function AgentQueueView() {
  const autonomy = useGrokHub((s) => s.autonomy);
  const queue = useGrokHub((s) => s.agentQueue);
  const setAutonomy = useGrokHub((s) => s.setAutonomy);
  const pauseAutonomy = useGrokHub((s) => s.pauseAutonomy);
  const processAgentQueue = useGrokHub((s) => s.processAgentQueue);
  const cancelAgentJob = useGrokHub((s) => s.cancelAgentJob);
  const approveAgentJob = useGrokHub((s) => s.approveAgentJob);
  const claimWorkboardJobs = useGrokHub((s) => s.claimWorkboardJobs);

  const stats = useMemo(() => queueStats(queue), [queue]);
  const jobs = useMemo(
    () =>
      [...queue.jobs].sort((a, b) => {
        const order = ["running", "waiting_user", "queued", "blocked", "failed", "done", "cancelled"];
        return order.indexOf(a.status) - order.indexOf(b.status) || b.createdAt - a.createdAt;
      }),
    [queue.jobs],
  );

  return (
    <div className="content-readable mx-auto space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div>
          <h1 className="flex items-center gap-2 text-lg font-semibold tracking-tight">
            <ListTodo className="h-5 w-5 text-[var(--color-info)]" />
            Agent queue
          </h1>
          <p className="mt-1 max-w-xl text-sm text-[var(--color-muted)]">
            Always-on job lane — automations, workboard, chats, and goal steps. Pause anytime.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant={autonomy.paused ? "default" : "secondary"}
            onClick={() => pauseAutonomy(!autonomy.paused)}
          >
            <CirclePause className="mr-1 h-3.5 w-3.5" />
            {autonomy.paused ? "Resume" : "Pause"}
          </Button>
          <Button size="sm" variant="secondary" onClick={() => void processAgentQueue()}>
            <Play className="mr-1 h-3.5 w-3.5" />
            Drain queue
          </Button>
          {autonomy.level >= 3 && (
            <Button size="sm" variant="secondary" onClick={() => { claimWorkboardJobs(); void processAgentQueue(); }}>
              Claim workboard
            </Button>
          )}
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {(
          [
            ["queued", stats.queued],
            ["running", stats.running],
            ["waiting", stats.waiting],
            ["failed", stats.failed],
          ] as const
        ).map(([k, v]) => (
          <Card key={k}>
            <CardContent className="flex items-center justify-between p-3">
              <span className="text-xs uppercase tracking-wide text-[var(--color-subtle)]">{k}</span>
              <span className="font-mono text-lg tabular">{v}</span>
            </CardContent>
          </Card>
        ))}
      </div>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Autonomy level</CardTitle>
          <CardDescription>
            {AUTONOMY_LABEL[autonomy.level]} — {AUTONOMY_HINT[autonomy.level]}
            {autonomy.paused ? " · PAUSED" : ""}
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-wrap gap-2">
          {([0, 1, 2, 3, 4] as AutonomyLevel[]).map((lv) => (
            <button
              key={lv}
              type="button"
              onClick={() => setAutonomy({ level: lv })}
              className={cn(
                "rounded-full border px-3 py-1 text-xs font-medium transition-colors",
                autonomy.level === lv
                  ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                  : "border-[var(--color-border)] text-[var(--color-muted)] hover:text-[var(--color-fg)]",
              )}
            >
              {lv} · {AUTONOMY_LABEL[lv]}
            </button>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Jobs</CardTitle>
          <CardDescription>Durable across restarts · mirrored to desktop agent core</CardDescription>
        </CardHeader>
        <CardContent className="space-y-2">
          {jobs.length === 0 && (
            <p className="py-6 text-center text-sm text-[var(--color-subtle)]">
              Queue empty. Automations and workboard claim will appear here.
            </p>
          )}
          {jobs.map((j) => (
            <div
              key={j.id}
              className="flex flex-col gap-2 rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-elevated)] p-3 sm:flex-row sm:items-start sm:justify-between"
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-1.5">
                  <Badge
                    variant={
                      j.status === "running"
                        ? "info"
                        : j.status === "failed"
                          ? "danger"
                          : j.status === "done"
                            ? "success"
                            : "default"
                    }
                  >
                    {j.status}
                  </Badge>
                  <Badge variant="default" className="font-mono text-[10px]">
                    {j.type}
                  </Badge>
                  {j.needsApproval && j.approval === "pending" && (
                    <span className="inline-flex items-center gap-1 text-[10px] text-[var(--color-warn)]">
                      <ShieldAlert className="h-3 w-3" /> needs approval
                    </span>
                  )}
                </div>
                <div className="mt-1 text-sm font-medium text-[var(--color-fg)]">{j.title}</div>
                <p className="mt-0.5 line-clamp-2 text-xs text-[var(--color-muted)]">{j.prompt}</p>
                {j.lastError && (
                  <p className="mt-1 text-[11px] text-[var(--color-danger)]">{j.lastError}</p>
                )}
                {j.resultSummary && (
                  <p className="mt-1 text-[11px] text-[var(--color-subtle)]">{j.resultSummary}</p>
                )}
              </div>
              <div className="flex shrink-0 flex-wrap gap-1">
                {j.status === "waiting_user" || (j.needsApproval && j.approval !== "granted") ? (
                  <>
                    <Button size="sm" onClick={() => approveAgentJob(j.id, true)}>
                      <CheckCircle2 className="mr-1 h-3 w-3" />
                      Allow
                    </Button>
                    <Button size="sm" variant="secondary" onClick={() => approveAgentJob(j.id, false)}>
                      <XCircle className="mr-1 h-3 w-3" />
                      Deny
                    </Button>
                  </>
                ) : null}
                {!["done", "cancelled"].includes(j.status) && (
                  <Button size="sm" variant="ghost" onClick={() => cancelAgentJob(j.id)}>
                    <Trash2 className="h-3.5 w-3.5" />
                  </Button>
                )}
              </div>
            </div>
          ))}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-2">
          <CardTitle className="text-sm">Tool registry</CardTitle>
          <CardDescription>What the agent can invoke vs status-only</CardDescription>
        </CardHeader>
        <CardContent className="space-y-1.5">
          {TOOL_REGISTRY.map((t) => (
            <div
              key={t.id}
              className="flex items-start justify-between gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] px-2 py-1.5 text-xs"
            >
              <div>
                <span className="font-mono text-[var(--color-fg)]">{t.id}</span>
                <span className="text-[var(--color-muted)]"> — {t.description}</span>
              </div>
              <Badge variant={t.live ? "success" : "default"}>{t.live ? "live" : "status"}</Badge>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}
