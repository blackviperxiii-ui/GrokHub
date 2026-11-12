import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { ChatEvent } from "./api";
import { EvidenceCard, FindingsCard, RecapCard } from "./TurnExtras";

type FindingsEvent = Extract<ChatEvent, { kind: "findings" }>;
type RecapEvent = Extract<ChatEvent, { kind: "recap" }>;
type EvidenceEvent = Extract<ChatEvent, { kind: "evidence" }>;

const FINDINGS: FindingsEvent = {
  kind: "findings",
  items: [
    { severity: "high", text: "Port 53 is taken by another service" },
    { severity: "low", text: "20 boot errors" },
  ],
  fixes: [
    { label: "Fix port 53 conflict", prompt: "Fix port 53 conflict: Stop the other service" },
    { label: "Explain all", prompt: "Explain all: Explain each finding" },
  ],
  doAll: null,
};

const RECAP: RecapEvent = {
  kind: "recap",
  body: "All fixed.",
  lines: [
    { status: "done", text: "Chips scrubbed" },
    { status: "partly", text: "Pulse noise tamed" },
    { status: "notDone", text: "Soul file" },
  ],
  needsYou: ["Restart the app"],
  undo: ["Settings, Backups"],
  steps: [
    { title: "read_file", failed: false },
    { title: "run_command", failed: true },
  ],
};

const EVIDENCE: EvidenceEvent = {
  kind: "evidence",
  summary: "Evidence · Turn 2 · verified · Fix the port",
  goal: "Fix the port",
  goalDetail: "",
  plan: ["verify: port binds"],
  actions: ["Stopped the other service"],
  checks: ["Port 53 answers"],
  changes: [],
  confidence: "verified",
  recovery: "unavailable",
  toolRanNotVerified: false,
};

describe("FindingsCard", () => {
  it("renders the findings and a pill per fix; a pill hands its prompt over", async () => {
    const onFix = vi.fn();
    render(<FindingsCard event={FINDINGS} onFix={onFix} />);
    expect(screen.getByText("Findings from this run")).toBeVisible();
    expect(screen.getByText("Port 53 is taken by another service")).toBeVisible();
    expect(screen.getByText("high")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Fix port 53 conflict" }));
    expect(onFix).toHaveBeenCalledWith("Fix port 53 conflict: Stop the other service");
  });

  it("offers Do all when a batch exists, and Dismiss sets the card aside", async () => {
    const onFix = vi.fn();
    const { rerender } = render(
      <FindingsCard event={{ ...FINDINGS, doAll: ["Fix 1/2", "Fix 2/2"] }} onFix={onFix} />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Do all" }));
    expect(onFix).toHaveBeenCalledWith("Fix 1/2\nFix 2/2");
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText("Findings from this run")).toBeNull();
    rerender(<FindingsCard event={FINDINGS} onFix={onFix} />);
    expect(screen.queryByRole("button", { name: "Do all" })).toBeNull();
  });
});

describe("RecapCard", () => {
  it("renders the tally, the marks, what needs you, and the steps fold", async () => {
    render(<RecapCard event={RECAP} />);
    expect(screen.getByText("FINISHED")).toBeVisible();
    expect(screen.getByText("1 of 3 done")).toBeVisible();
    expect(screen.getByText("Chips scrubbed")).toBeVisible();
    expect(screen.getByText("Restart the app")).toBeVisible();
    expect(screen.getByText("Settings, Backups")).toBeVisible();
    expect(screen.queryByText("read_file")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Show details" }));
    expect(screen.getByText("read_file")).toBeVisible();
    expect(screen.getByText("run_command")).toBeVisible();
  });
});

describe("EvidenceCard", () => {
  it("starts closed on its summary and opens into its sections", async () => {
    render(<EvidenceCard event={EVIDENCE} />);
    expect(screen.getByText("Evidence · Turn 2 · verified · Fix the port")).toBeVisible();
    expect(screen.queryByText("Plan status")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: /Evidence · Turn 2/ }));
    expect(screen.getByText("Plan status")).toBeVisible();
    expect(screen.getByText("verify: port binds")).toBeVisible();
    expect(screen.getByText("Port 53 answers")).toBeVisible();
    expect(screen.getByText("verified")).toBeVisible();
  });

  it("warns when tools ran but the goal was not verified", async () => {
    render(<EvidenceCard event={{ ...EVIDENCE, toolRanNotVerified: true }} />);
    await userEvent.click(screen.getByRole("button", { name: /Evidence · Turn 2/ }));
    expect(screen.getByText("A successful tool is not proof the goal succeeded.")).toBeVisible();
  });
});
