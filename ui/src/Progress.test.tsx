import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  clampFrac,
  GlowBar,
  pctOf,
  STATUS_STAGES,
  statusFromLane,
  statusFromWork,
  StatusTracker,
  StepTracker,
  UsageMeter,
  UsageRing,
} from "./Progress";

describe("progress helpers", () => {
  it("clamps fractions and rounds whole percents", () => {
    expect(clampFrac(-0.2)).toBe(0);
    expect(clampFrac(1.4)).toBe(1);
    expect(clampFrac(0.5)).toBe(0.5);
    expect(clampFrac(Number.NaN)).toBe(0);
    expect(pctOf(0.904)).toBe(90);
    expect(pctOf(1)).toBe(100);
    expect(pctOf(0)).toBe(0);
  });

  it("maps Workboard lanes onto the Command Center pipeline", () => {
    expect(statusFromLane("todo")).toBe("submitted");
    expect(statusFromLane("followUp")).toBe("submitted");
    expect(statusFromLane("doing")).toBe("in_progress");
    expect(statusFromLane("blocked")).toBe("review");
    expect(statusFromLane("done")).toBe("merged");
    expect(statusFromLane("unknown")).toBe("submitted");
  });

  it("maps helper work states onto the same pipeline", () => {
    expect(statusFromWork("running")).toBe("in_progress");
    expect(statusFromWork("ready")).toBe("review");
    expect(statusFromWork("conflict")).toBe("review");
    expect(statusFromWork("held")).toBe("review");
    expect(statusFromWork("merged")).toBe("merged");
    expect(statusFromWork("empty")).toBe("submitted");
    expect(statusFromWork("discarded")).toBe("submitted");
  });
});

describe("UsageRing", () => {
  it("exposes the used percent and marks hot at 90%", () => {
    const { rerender } = render(<UsageRing frac={0.4} label="Grok requests used" />);
    const ring = screen.getByRole("progressbar", { name: "Grok requests used" });
    expect(ring).toHaveAttribute("aria-valuenow", "40");
    expect(ring).toHaveTextContent("40%");
    expect(ring).not.toHaveClass("usage-ring--hot");

    rerender(<UsageRing frac={0.9} label="Grok requests used" />);
    expect(screen.getByRole("progressbar", { name: "Grok requests used" })).toHaveClass("usage-ring--hot");
    expect(screen.getByRole("progressbar", { name: "Grok requests used" })).toHaveAttribute("aria-valuenow", "90");
  });
});

describe("GlowBar", () => {
  it("fills to the used percent and marks hot at 90%", () => {
    render(<GlowBar frac={0.9} label="Cursor period used" />);
    const bar = screen.getByRole("progressbar", { name: "Cursor period used" });
    expect(bar).toHaveAttribute("aria-valuenow", "90");
    expect(bar).toHaveClass("glow-bar--hot");
    expect(bar.querySelector(".glow-bar-fill")).toHaveStyle({ width: "90%" });
  });

  it("keeps a custom class for Fleet styling", () => {
    render(<GlowBar frac={0.5} label="Grok tokens used" className="fleet-bar" />);
    expect(screen.getByRole("progressbar", { name: "Grok tokens used" })).toHaveClass("glow-bar", "fleet-bar");
    expect(screen.getByRole("progressbar", { name: "Grok tokens used" })).not.toHaveClass("glow-bar--hot");
  });
});

describe("StatusTracker", () => {
  it("marks past stages done and the current one current", () => {
    render(<StatusTracker current="review" />);
    const group = screen.getByRole("group", { name: "Status" });
    const items = [...group.querySelectorAll("[data-stage]")];
    expect(items.map((el) => el.getAttribute("data-stage"))).toEqual(STATUS_STAGES.map((s) => s.id));
    expect(items[0]).toHaveClass("status-tracker-step--done");
    expect(items[1]).toHaveClass("status-tracker-step--done");
    expect(items[2]).toHaveClass("status-tracker-step--current");
    expect(items[2]).toHaveTextContent("Review");
    expect(items[2]).toHaveTextContent("(current)");
    expect(items[3]).toHaveClass("status-tracker-step--todo");
  });
});

describe("StepTracker", () => {
  const STEPS = [
    { id: "welcome", label: "Welcome" },
    { id: "about", label: "About" },
    { id: "review", label: "Review" },
  ];

  it("numbers each step and marks the current one", () => {
    render(<StepTracker steps={STEPS} current="about" label="Onboarding" />);
    const group = screen.getByRole("group", { name: "Onboarding" });
    const items = [...group.querySelectorAll("[data-step]")];
    expect(items.map((el) => el.getAttribute("data-step"))).toEqual(["welcome", "about", "review"]);
    expect(items[0]).toHaveClass("step-tracker-step--done");
    expect(items[1]).toHaveClass("step-tracker-step--current");
    expect(items[1]).toHaveTextContent("2");
    expect(items[1]).toHaveTextContent("About");
    expect(items[2]).toHaveClass("step-tracker-step--todo");
  });
});

describe("UsageMeter", () => {
  it("pairs a decorative ring with one progress bar", () => {
    const { container } = render(
      <UsageMeter frac={0.25} label="Short window used">
        <span>Short window: 25% used</span>
      </UsageMeter>,
    );
    const bar = screen.getByRole("progressbar", { name: "Short window used" });
    expect(bar).toHaveClass("glow-bar", "fleet-bar");
    expect(bar).toHaveAttribute("aria-valuenow", "25");
    expect(container.querySelector(".usage-ring")).toHaveAttribute("aria-hidden", "true");
    expect(container.querySelector(".usage-ring-pct")).toHaveTextContent("25%");
    expect(screen.getByText("Short window: 25% used")).toBeInTheDocument();
  });
});
