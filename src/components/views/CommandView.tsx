import {
  Activity,
  ArrowRight,
  Cable,
  HardDrive,
  ImageIcon,
  Sparkles,
  TimerReset,
  Zap,
} from "lucide-react";
import type { ReactNode } from "react";
import { getModesWithCatalog } from "@/lib/modes";
import { useGrokHub } from "@/lib/store";
import { RelativeTime } from "../RelativeTime";
import { UsageMeterPanel } from "../UsageMeter";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";

export function CommandView() {
  const connectors = useGrokHub((s) => s.connectors);
  const skills = useGrokHub((s) => s.skills);
  const automations = useGrokHub((s) => s.automations);
  const activity = useGrokHub((s) => s.activity);
  const agents = useGrokHub((s) => s.agents);
  const mode = useGrokHub((s) => s.mode);
  const setMode = useGrokHub((s) => s.setMode);
  const modelCatalog = useGrokHub((s) => s.modelCatalog);
  const modelOverrides = useGrokHub((s) => s.modelOverrides);
  const setNav = useGrokHub((s) => s.setNav);
  const runAutomation = useGrokHub((s) => s.runAutomation);
  const sendChat = useGrokHub((s) => s.sendChat);

  const connected = connectors.filter((c) => c.status === "connected").length;
  const enabledSkills = skills.filter((s) => s.enabled).length;
  const activeAutos = automations.filter((a) => a.enabled).length;

  return (
    <div className="space-y-6">
      <section className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <StatCard
          label="Live tools"
          value={`${connected}/${connectors.length}`}
          hint="OAuth tools live"
          icon={<Cable className="h-4 w-4" />}
          onClick={() => setNav("settings")}
        />
        <StatCard
          label="Skills"
          value={`${enabledSkills} on`}
          hint={`${skills.length} total`}
          icon={<Sparkles className="h-4 w-4" />}
          onClick={() => setNav("skills")}
        />
        <StatCard
          label="Automations"
          value={String(activeAutos)}
          hint="scheduled + triggers"
          icon={<TimerReset className="h-4 w-4" />}
          onClick={() => setNav("automations")}
        />
        <StatCard
          label="Agents"
          value={String(agents.filter((a) => a.status !== "offline").length)}
          hint="online roster"
          icon={<Zap className="h-4 w-4" />}
          onClick={() => setNav("agents")}
        />
      </section>

      <UsageMeterPanel compact />

      <section className="grid gap-4 lg:grid-cols-[1.35fr_1fr]">
        <Card>
          <CardHeader>
            <CardTitle>Grok modes</CardTitle>
            <CardDescription>
              Baked in exactly like the web picker — Adaptive, Fast, Expert, Heavy, Build. Costs scale
              by mode.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-2 sm:grid-cols-2">
            {getModesWithCatalog(modelCatalog, modelOverrides).map((m) => {
              const selected = m.id === mode;
              const costHint =
                m.id === "max"
                  ? "text-amber-400"
                  : m.id === "heavy"
                  ? "8u"
                  : m.id === "expert"
                    ? "4u"
                    : m.id === "build"
                      ? "2u"
                      : m.id === "auto"
                        ? "1.5u"
                        : "1u";
              return (
                <button
                  key={m.id}
                  type="button"
                  onClick={() => setMode(m.id)}
                  className={
                    selected
                      ? "rounded-[var(--radius-md)] border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-3 text-left"
                      : "rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-3 text-left hover:border-[var(--color-border-strong)]"
                  }
                >
                  <div className="flex items-center gap-2 text-sm font-medium">
                    {m.label}
                    {m.id === "build" && <Badge className="text-[10px]">Beta</Badge>}
                    <span className="ml-auto font-mono text-[10px] text-[var(--color-subtle)]">
                      {costHint}
                    </span>
                  </div>
                  <div className="mt-0.5 text-xs text-[var(--color-muted)]">{m.subtitle}</div>
                </button>
              );
            })}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle>Quick actions</CardTitle>
            <CardDescription>Agent, host desktop, Imagine, automations.</CardDescription>
          </CardHeader>
          <CardContent className="grid gap-2">
            <Button
              variant="secondary"
              className="h-auto justify-start px-4 py-3 text-left"
              onClick={() => setNav("desktop")}
            >
              <div className="flex items-center gap-2">
                <HardDrive className="h-4 w-4 text-[var(--color-muted)]" />
                <div>
                  <div className="text-sm font-medium text-[var(--color-fg)]">
                    Desktop host
                  </div>
                  <div className="text-xs text-[var(--color-subtle)]">
                    Unsandboxed CLI · files · apps
                  </div>
                </div>
              </div>
            </Button>
            <Button
              variant="secondary"
              className="h-auto justify-start px-4 py-3 text-left"
              onClick={() => {
                setNav("chat");
                void sendChat("/morning");
              }}
            >
              <div>
                <div className="text-sm font-medium text-[var(--color-fg)]">Run morning brief</div>
                <div className="text-xs text-[var(--color-subtle)]">Uses active mode routing</div>
              </div>
            </Button>
            <Button
              variant="secondary"
              className="h-auto justify-start px-4 py-3 text-left"
              onClick={() => setNav("imagine")}
            >
              <div className="flex items-center gap-2">
                <ImageIcon className="h-4 w-4 text-[var(--color-muted)]" />
                <div>
                  <div className="text-sm font-medium text-[var(--color-fg)]">Open Imagine</div>
                  <div className="text-xs text-[var(--color-subtle)]">5 units per render</div>
                </div>
              </div>
            </Button>
            <Button
              variant="secondary"
              className="h-auto justify-start px-4 py-3 text-left"
              onClick={() => {
                const id = automations.find((a) => a.enabled)?.id;
                if (id) void runAutomation(id);
              }}
            >
              <div>
                <div className="text-sm font-medium text-[var(--color-fg)]">Run top automation</div>
                <div className="text-xs text-[var(--color-subtle)]">First enabled job</div>
              </div>
            </Button>
          </CardContent>
        </Card>
      </section>

      <Card>
        <CardHeader className="flex-row items-center justify-between gap-3 space-y-0">
          <div>
            <CardTitle className="flex items-center gap-2">
              <Activity className="h-4 w-4" />
              Activity
            </CardTitle>
            <CardDescription>
              Runs across modes, host CLI, Imagine, skills, and chat.
            </CardDescription>
          </div>
          <Button variant="ghost" size="sm" onClick={() => setNav("chat")}>
            Open agent
            <ArrowRight className="h-3.5 w-3.5" />
          </Button>
        </CardHeader>
        <CardContent>
          <ul className="divide-y divide-[var(--color-border)]">
            {activity.slice(0, 8).map((item) => (
              <li
                key={item.id}
                className="flex items-start justify-between gap-3 py-3 first:pt-0 last:pb-0"
              >
                <div className="min-w-0">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="text-sm font-medium">{item.title}</span>
                    {item.status && (
                      <Badge
                        variant={
                          item.status === "success"
                            ? "success"
                            : item.status === "running"
                              ? "info"
                              : item.status === "failed"
                                ? "danger"
                                : "default"
                        }
                      >
                        {item.status}
                      </Badge>
                    )}
                  </div>
                  <p className="mt-0.5 truncate text-sm text-[var(--color-muted)]">
                    {item.detail}
                  </p>
                </div>
                <RelativeTime
                  ts={item.ts}
                  className="shrink-0 tabular text-xs text-[var(--color-subtle)]"
                />
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}

function StatCard({
  label,
  value,
  hint,
  icon,
  onClick,
}: {
  label: string;
  value: string;
  hint: string;
  icon: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="rounded-[var(--radius-xl)] border border-[var(--color-border)] bg-[var(--color-panel)] p-4 text-left shadow-[var(--shadow-soft)] transition-colors hover:border-[var(--color-border-strong)]"
    >
      <div className="mb-3 flex items-center justify-between text-[var(--color-muted)]">
        <span className="text-xs font-medium uppercase tracking-wide">{label}</span>
        {icon}
      </div>
      <div className="text-2xl font-semibold tracking-tight tabular">{value}</div>
      <div className="mt-1 text-xs text-[var(--color-subtle)]">{hint}</div>
    </button>
  );
}
