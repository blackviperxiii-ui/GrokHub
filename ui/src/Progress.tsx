// Reusable progress meters and steppers for Fleet, Workboard, and onboarding.
// Visual reference: Quix Studio free Progress & Steppers (restyled for Loadout).
import type { CSSProperties, ReactNode } from "react";

/** Clamp a fraction into 0..1. */
export function clampFrac(frac: number): number {
  if (!Number.isFinite(frac)) return 0;
  return Math.min(1, Math.max(0, frac));
}

/** Whole percent 0..100 from a 0..1 fraction. */
export function pctOf(frac: number): number {
  return Math.round(clampFrac(frac) * 100);
}

/** Command Center card pipeline: submitted → in progress → review → merged. */
export const STATUS_STAGES = [
  { id: "submitted", label: "Submitted" },
  { id: "in_progress", label: "In progress" },
  { id: "review", label: "Review" },
  { id: "merged", label: "Merged" },
] as const;

export type StatusStageId = (typeof STATUS_STAGES)[number]["id"];

/** Map a Workboard lane onto the Command Center pipeline. */
export function statusFromLane(lane: string): StatusStageId {
  switch (lane) {
    case "doing":
      return "in_progress";
    case "blocked":
      return "review";
    case "done":
      return "merged";
    case "todo":
    case "followUp":
    default:
      return "submitted";
  }
}

/** Map helper work onto the same pipeline. */
export function statusFromWork(state: string): StatusStageId {
  switch (state) {
    case "running":
      return "in_progress";
    case "ready":
    case "conflict":
    case "held":
      return "review";
    case "merged":
      return "merged";
    case "empty":
    case "discarded":
    default:
      return "submitted";
  }
}

/** Circular usage ring (Quix-style ring percent, Loadout palette). */
export function UsageRing({
  frac,
  label,
  size = 40,
  hot = false,
}: {
  /** Part used, 0..1. */
  frac: number;
  label: string;
  size?: number;
  /** At or above 90%: stronger red fill. */
  hot?: boolean;
}) {
  const pct = pctOf(frac);
  const isHot = hot || pct >= 90;
  const r = 15.5;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - clampFrac(frac));
  return (
    <span
      className={`usage-ring${isHot ? " usage-ring--hot" : ""}`}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
      style={{ "--ring-size": `${size}px` } as CSSProperties}
    >
      <svg viewBox="0 0 36 36" width={size} height={size} aria-hidden="true">
        <circle className="usage-ring-track" cx="18" cy="18" r={r} />
        <circle
          className="usage-ring-fill"
          cx="18"
          cy="18"
          r={r}
          strokeDasharray={`${c} ${c}`}
          strokeDashoffset={offset}
        />
      </svg>
      <span className="usage-ring-pct">{pct}%</span>
    </span>
  );
}

/** Horizontal glow bar for subscription usage. */
export function GlowBar({
  frac,
  label,
  hot = false,
  className = "",
}: {
  frac: number;
  label: string;
  hot?: boolean;
  className?: string;
}) {
  const pct = pctOf(frac);
  const isHot = hot || pct >= 90;
  const classes = ["glow-bar", isHot ? "glow-bar--hot" : "", className].filter(Boolean).join(" ");
  return (
    <span
      className={classes}
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={pct}
    >
      <span className="glow-bar-fill" style={{ width: `${pct}%` }} />
    </span>
  );
}

/** Vertical status tracker for Command Center / helper work. */
export function StatusTracker({
  current,
  stages = STATUS_STAGES,
  label = "Status",
}: {
  current: StatusStageId | string;
  stages?: readonly { id: string; label: string }[];
  label?: string;
}) {
  const at = Math.max(
    0,
    stages.findIndex((s) => s.id === current),
  );
  return (
    <div className="status-tracker" role="group" aria-label={label}>
      {stages.map((stage, i) => {
        let state = "todo";
        if (i < at) state = "done";
        else if (i === at) state = "current";
        return (
          <div key={stage.id} className={`status-tracker-step status-tracker-step--${state}`} data-stage={stage.id}>
            <span className="status-tracker-mark" aria-hidden="true" />
            <span className="status-tracker-label">
              {stage.label}
              {state === "current" && <span className="sr-only"> (current)</span>}
              {state === "done" && <span className="sr-only"> (done)</span>}
            </span>
          </div>
        );
      })}
    </div>
  );
}

/** Horizontal step tracker for onboarding and multi-step agent runs. */
export function StepTracker({
  steps,
  current,
  label = "Steps",
}: {
  steps: readonly { id: string; label: string }[];
  current: string;
  label?: string;
}) {
  const at = Math.max(
    0,
    steps.findIndex((s) => s.id === current),
  );
  return (
    <div className="step-tracker" role="group" aria-label={label}>
      {steps.map((step, i) => {
        let state = "todo";
        if (i < at) state = "done";
        else if (i === at) state = "current";
        return (
          <div key={step.id} className={`step-tracker-step step-tracker-step--${state}`} data-step={step.id}>
            <span className="step-tracker-dot" aria-hidden="true">
              {i + 1}
            </span>
            <span className="step-tracker-label">
              {step.label}
              {state === "current" && <span className="sr-only"> (current)</span>}
            </span>
            {i < steps.length - 1 && <span className="step-tracker-line" aria-hidden="true" />}
          </div>
        );
      })}
    </div>
  );
}

/** Compact row: ring + glow bar. The bar keeps the progressbar role; the ring
 * is a visual twin (same percent) so screen readers hear the meter once. */
export function UsageMeter({
  frac,
  label,
  children,
  className = "",
}: {
  frac: number;
  label: string;
  children?: ReactNode;
  className?: string;
}) {
  const pct = pctOf(frac);
  const isHot = pct >= 90;
  const r = 15.5;
  const c = 2 * Math.PI * r;
  const offset = c * (1 - clampFrac(frac));
  return (
    <div className={`usage-meter${className ? ` ${className}` : ""}`}>
      <span
        className={`usage-ring${isHot ? " usage-ring--hot" : ""}`}
        aria-hidden="true"
        style={{ "--ring-size": "40px" } as CSSProperties}
      >
        <svg viewBox="0 0 36 36" width={40} height={40}>
          <circle className="usage-ring-track" cx="18" cy="18" r={r} />
          <circle
            className="usage-ring-fill"
            cx="18"
            cy="18"
            r={r}
            strokeDasharray={`${c} ${c}`}
            strokeDashoffset={offset}
          />
        </svg>
        <span className="usage-ring-pct">{pct}%</span>
      </span>
      <div className="usage-meter-body">
        {children}
        <GlowBar frac={frac} label={label} className="fleet-bar" />
      </div>
    </div>
  );
}
