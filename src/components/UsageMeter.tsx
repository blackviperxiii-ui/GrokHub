import { Gauge } from "lucide-react";
import {
  daysLeftInPeriod,
  formatUnits,
  PLAN_LIMITS,
  usagePercent,
  usageTone,
} from "@/lib/usage";
import { useGrokHub } from "@/lib/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "./ui/card";

function barColor(tone: "ok" | "warn" | "danger") {
  if (tone === "danger") return "bg-[var(--color-danger)]";
  if (tone === "warn") return "bg-[var(--color-warn)]";
  return "bg-[var(--color-fg)]";
}

/** Compact titlebar / sidebar chip */
export function UsageMeterChip({ className }: { className?: string }) {
  const usage = useGrokHub((s) => s.usage);
  const setNav = useGrokHub((s) => s.setNav);
  const plan = PLAN_LIMITS[usage.plan];
  const pct = usagePercent(usage);
  const tone = usageTone(pct);

  return (
    <button
      type="button"
      onClick={() => setNav("settings")}
      title={`${plan.label}: ${formatUnits(usage.usedUnits)} / ${formatUnits(plan.units)} units`}
      className={cn(
        "flex min-w-0 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-2 py-1 text-left transition-colors hover:border-[var(--color-border-strong)]",
        className,
      )}
    >
      <Gauge
        className={cn(
          "h-3.5 w-3.5 shrink-0",
          tone === "danger"
            ? "text-[var(--color-danger)]"
            : tone === "warn"
              ? "text-[var(--color-warn)]"
              : "text-[var(--color-muted)]",
        )}
      />
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <span className="truncate text-[10px] font-medium text-[var(--color-fg)]">
            {plan.label}
          </span>
          <span className="tabular text-[10px] text-[var(--color-subtle)]">
            {Math.round(pct)}%
          </span>
        </div>
        <div className="mt-0.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
          <div
            className={cn("h-full rounded-full transition-all duration-300", barColor(tone))}
            style={{ width: `${pct}%` }}
          />
        </div>
      </div>
    </button>
  );
}

/** Full breakdown card for Command / Settings */
export function UsageMeterPanel({ compact }: { compact?: boolean }) {
  const usage = useGrokHub((s) => s.usage);
  const setPlan = useGrokHub((s) => s.setPlan);
  const resetUsage = useGrokHub((s) => s.resetUsagePeriod);
  const plan = PLAN_LIMITS[usage.plan];
  const pct = usagePercent(usage);
  const tone = usageTone(pct);
  const left = daysLeftInPeriod(usage);
  const remaining = Math.max(0, plan.units - usage.usedUnits);

  const rows: { label: string; used: number; cap: number }[] = [
    { label: "Agent messages", used: usage.messages, cap: plan.messages },
    { label: "Imagine", used: usage.imagine, cap: plan.imagine },
    { label: "Automations", used: usage.automations, cap: plan.automations },
    { label: "Host CLI", used: usage.host, cap: plan.host },
  ];

  return (
    <Card>
      <CardHeader className={compact ? "pb-2" : undefined}>
        <div className="flex flex-wrap items-start justify-between gap-2">
          <div>
            <CardTitle className="flex items-center gap-2 text-sm">
              <Gauge className="h-4 w-4 text-[var(--color-muted)]" />
              Usage · subscription limits
            </CardTitle>
            <CardDescription>
              Compute units reset each billing period. Heavy / Expert cost more per turn.
            </CardDescription>
          </div>
          <Badge
            variant={
              tone === "danger" ? "danger" : tone === "warn" ? "warn" : "success"
            }
          >
            {plan.label}
          </Badge>
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        <div>
          <div className="mb-1.5 flex items-end justify-between gap-2">
            <div>
              <div className="text-2xl font-semibold tracking-tight tabular">
                {formatUnits(usage.usedUnits)}
                <span className="text-sm font-normal text-[var(--color-muted)]">
                  {" "}
                  / {formatUnits(plan.units)}
                </span>
              </div>
              <div className="text-xs text-[var(--color-subtle)]">
                {formatUnits(remaining)} units left · {left}d until reset
              </div>
            </div>
            <div
              className={cn(
                "text-lg font-semibold tabular",
                tone === "danger"
                  ? "text-[var(--color-danger)]"
                  : tone === "warn"
                    ? "text-[var(--color-warn)]"
                    : "text-[var(--color-fg)]",
              )}
            >
              {Math.round(pct)}%
            </div>
          </div>
          <div className="h-2.5 overflow-hidden rounded-full bg-[var(--color-border)]">
            <div
              className={cn("h-full rounded-full transition-all duration-500", barColor(tone))}
              style={{ width: `${pct}%` }}
            />
          </div>
        </div>

        <div className="grid gap-2 sm:grid-cols-2">
          {rows.map((r) => {
            const p = r.cap > 0 ? Math.min(100, (r.used / r.cap) * 100) : 0;
            const t = usageTone(p);
            return (
              <div
                key={r.label}
                className="rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-surface)] px-3 py-2"
              >
                <div className="flex justify-between text-xs">
                  <span className="text-[var(--color-muted)]">{r.label}</span>
                  <span className="tabular text-[var(--color-fg)]">
                    {r.used}
                    <span className="text-[var(--color-subtle)]"> / {r.cap}</span>
                  </span>
                </div>
                <div className="mt-1.5 h-1 overflow-hidden rounded-full bg-[var(--color-border)]">
                  <div
                    className={cn("h-full rounded-full", barColor(t))}
                    style={{ width: `${p}%` }}
                  />
                </div>
              </div>
            );
          })}
        </div>

        {!compact && (
          <>
            <div>
              <div className="mb-1.5 text-[10px] font-medium uppercase tracking-wide text-[var(--color-subtle)]">
                By mode (messages)
              </div>
              <div className="flex flex-wrap gap-1.5">
                {(
                  [
                    ["fast", "Fast"],
                    ["auto", "Auto"],
                    ["build", "Build"],
                    ["expert", "Expert"],
                    ["heavy", "Heavy"],
                  ] as const
                ).map(([id, label]) => (
                  <span
                    key={id}
                    className="rounded-full border border-[var(--color-border)] px-2 py-0.5 text-[10px] text-[var(--color-muted)]"
                  >
                    {label}{" "}
                    <span className="tabular text-[var(--color-fg)]">{usage.byMode[id]}</span>
                  </span>
                ))}
              </div>
            </div>

            <div className="flex flex-wrap gap-2">
              {(
                [
                  ["free", "Free"],
                  ["super", "SuperGrok"],
                  ["pro", "SuperGrok Pro"],
                ] as const
              ).map(([id, label]) => (
                <button
                  key={id}
                  type="button"
                  onClick={() => setPlan(id)}
                  className={cn(
                    "rounded-full border px-3 py-1 text-xs transition-colors",
                    usage.plan === id
                      ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
                      : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)]",
                  )}
                >
                  {label}
                </button>
              ))}
              <button
                type="button"
                onClick={() => resetUsage()}
                className="ml-auto rounded-full border border-[var(--color-border)] px-3 py-1 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]"
              >
                Simulate period reset
              </button>
            </div>
          </>
        )}
      </CardContent>
    </Card>
  );
}
