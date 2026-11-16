import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudePlan } from "./api";

const api = vi.hoisted(() => ({
  claudePlanStatus: vi.fn<() => Promise<ClaudePlan>>(),
}));
vi.mock("./api", () => api);

import { CLAUDE_MISSING, CLAUDE_SOON, ClaudePlanCard, claudePlanLine } from "./ClaudePlan";

beforeEach(() => {
  vi.resetAllMocks();
});

describe("Claude (your plan)", () => {
  it("says what this computer's Claude Code can do", () => {
    expect(claudePlanLine({ state: "missing" })).toBe(CLAUDE_MISSING);
    expect(claudePlanLine({ state: "signedOut", version: "2.1.300" })).toBe(
      "Claude Code 2.1.300, not signed in. In a terminal, run claude auth login, then check again.",
    );
    expect(claudePlanLine({ state: "plan", version: "2.1.300" })).toBe(
      "Claude Code 2.1.300, signed in with your Claude plan. Chats would count against your plan's usage.",
    );
    expect(claudePlanLine({ state: "otherBilling", version: "2.1.300", method: "api_key" })).toBe(
      "Claude Code 2.1.300, signed in with an Anthropic API key. Chats would be billed there, not to a Claude plan.",
    );
    expect(claudePlanLine({ state: "otherBilling", version: "2.1.300", method: "something_new" })).toBe(
      "Claude Code 2.1.300, signed in with something_new. Chats would be billed there, not to a Claude plan.",
    );
  });

  it("checks on open and again on Check again, after the user signs in", async () => {
    api.claudePlanStatus.mockResolvedValueOnce({ state: "signedOut", version: "2.1.300" });
    render(<ClaudePlanCard />);
    expect(await screen.findByText(/not signed in/)).toBeInTheDocument();
    expect(screen.queryByText(CLAUDE_SOON)).not.toBeInTheDocument();
    api.claudePlanStatus.mockResolvedValueOnce({ state: "plan", version: "2.1.300" });
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByText(/signed in with your Claude plan/)).toBeInTheDocument();
    expect(screen.getByText(CLAUDE_SOON)).toBeInTheDocument();
    expect(api.claudePlanStatus).toHaveBeenCalledTimes(2);
  });

  it("shows a failed check instead of a blank", async () => {
    api.claudePlanStatus.mockRejectedValue("AI models is only on the desktop app.");
    render(<ClaudePlanCard />);
    expect(await screen.findByRole("alert")).toHaveTextContent("AI models is only on the desktop app.");
    expect(screen.queryByText("Checking…")).not.toBeInTheDocument();
  });
});
