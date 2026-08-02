import { Users } from "lucide-react";
import { useGrokHub } from "@/lib/store";
import { Badge } from "../ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";

export function AgentsView() {
  const agents = useGrokHub((s) => s.agents);

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2">
            <Users className="h-4 w-4" />
            Multi-agent roster
          </CardTitle>
          <CardDescription>
            Roster status is UI-only. Real work runs as a single-agent tool loop in Agent chat
            (HOST_CMD / CONNECTOR_CMD). xAI multi-agent model IDs are blocked on chat completions —
            use Max (Grok 4.5) or Adaptive instead.
          </CardDescription>
        </CardHeader>
      </Card>

      <div className="grid gap-3 sm:grid-cols-2">
        {agents.map((a) => (
          <Card key={a.id}>
            <CardHeader>
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div
                    className="flex h-10 w-10 items-center justify-center rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] text-sm font-semibold"
                    style={{ color: a.color }}
                  >
                    {a.name.slice(0, 1)}
                  </div>
                  <div>
                    <CardTitle className="text-sm">{a.name}</CardTitle>
                    <CardDescription>{a.role}</CardDescription>
                  </div>
                </div>
                <Badge
                  variant={
                    a.status === "working"
                      ? "info"
                      : a.status === "idle"
                        ? "success"
                        : "default"
                  }
                >
                  {a.status}
                </Badge>
              </div>
            </CardHeader>
            <CardContent className="space-y-2">
              <div className="flex items-center justify-between rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
                <span className="text-xs text-[var(--color-muted)]">Model</span>
                <span className="font-mono text-xs text-[var(--color-fg)]">{a.model}</span>
              </div>
              <div className="flex items-center justify-between rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2">
                <span className="text-xs text-[var(--color-muted)]">Active tasks</span>
                <span className="tabular text-xs text-[var(--color-fg)]">{a.tasks}</span>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">How this maps to Grok web</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-3 text-sm text-[var(--color-muted)] md:grid-cols-3">
          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
            <div className="mb-1 font-medium text-[var(--color-fg)]">Connectors</div>
            I/O layer — Gmail, Drive, GitHub, Notion, custom MCP. Same idea as grok.com/connectors.
          </div>
          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
            <div className="mb-1 font-medium text-[var(--color-fg)]">Skills</div>
            Procedure layer — teach once, slash forever. Office docs + custom workflows.
          </div>
          <div className="rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] p-3">
            <div className="mb-1 font-medium text-[var(--color-fg)]">Automations</div>
            Schedule or email-trigger jobs that attach connectors + skills and leave a run log.
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
