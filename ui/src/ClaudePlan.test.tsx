import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeChats, ClaudePlan } from "./api";

const api = vi.hoisted(() => ({
  claudePlanStatus: vi.fn<() => Promise<ClaudePlan>>(),
  claudePlanChats: vi.fn<() => Promise<ClaudeChats>>(),
  setClaudePlanAllowed: vi.fn<(on: boolean) => Promise<ClaudeChats>>(),
}));
vi.mock("./api", () => api);

import { CLAUDE_ALLOW, CLAUDE_ALLOWED, CLAUDE_MISSING, CLAUDE_NO_ANSWER, ClaudePlanCard, claudePlanLine } from "./ClaudePlan";

const ALLOW_FOR_CHATS = { name: "Allow for chats" };

beforeEach(() => {
  vi.resetAllMocks();
  api.claudePlanChats.mockResolvedValue({ allowed: false, chats: [] });
});

describe("Claude (your plan)", () => {
  it("says what this computer's Claude Code can do", () => {
    expect(claudePlanLine({ state: "missing" })).toBe(CLAUDE_MISSING);
    expect(claudePlanLine({ state: "noAnswer" })).toBe(CLAUDE_NO_ANSWER);
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
    expect(screen.getByRole("button", { name: "Checking…" })).toBeDisabled();
    expect(await screen.findByText(/not signed in/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check again" })).toBeEnabled();
    // Signed out: nothing to allow yet.
    expect(screen.queryByRole("button", ALLOW_FOR_CHATS)).not.toBeInTheDocument();
    api.claudePlanStatus.mockResolvedValueOnce({ state: "plan", version: "2.1.300" });
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(await screen.findByText(/signed in with your Claude plan/)).toBeInTheDocument();
    expect(screen.getByRole("button", ALLOW_FOR_CHATS)).toBeEnabled();
    expect(api.claudePlanStatus).toHaveBeenCalledTimes(2);
  });

  it("asks before allowing chats on the plan, and revoking takes it back", async () => {
    api.claudePlanStatus.mockResolvedValue({ state: "plan", version: "2.1.300" });
    api.setClaudePlanAllowed.mockResolvedValueOnce({ allowed: true, chats: [] });
    render(<ClaudePlanCard />);
    await userEvent.click(await screen.findByRole("button", ALLOW_FOR_CHATS));
    const ask = screen.getByRole("group", { name: "Allow chats on your Claude plan" });
    expect(ask).toHaveTextContent(CLAUDE_ALLOW);
    expect(api.setClaudePlanAllowed).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(api.setClaudePlanAllowed).toHaveBeenCalledWith(true);
    expect(await screen.findByText(CLAUDE_ALLOWED)).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Allow chats on your Claude plan" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", ALLOW_FOR_CHATS)).not.toBeInTheDocument();

    api.setClaudePlanAllowed.mockResolvedValueOnce({ allowed: false, chats: [] });
    await userEvent.click(screen.getByRole("button", { name: "Stop using for chats" }));
    expect(api.setClaudePlanAllowed).toHaveBeenLastCalledWith(false);
    expect(await screen.findByRole("button", ALLOW_FOR_CHATS)).toBeInTheDocument();
    expect(screen.queryByText(CLAUDE_ALLOWED)).not.toBeInTheDocument();
  });

  it("Not now leaves it off, and a signed-out Claude Code can still be revoked", async () => {
    api.claudePlanStatus.mockResolvedValue({ state: "plan", version: "2.1.300" });
    render(<ClaudePlanCard />);
    await userEvent.click(await screen.findByRole("button", ALLOW_FOR_CHATS));
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(api.setClaudePlanAllowed).not.toHaveBeenCalled();
    expect(screen.getByRole("button", ALLOW_FOR_CHATS)).toBeInTheDocument();
  });

  it("shows a failed check instead of a blank", async () => {
    api.claudePlanStatus.mockRejectedValue("AI models is only on the desktop app.");
    render(<ClaudePlanCard />);
    expect(await screen.findByRole("alert")).toHaveTextContent("AI models is only on the desktop app.");
    expect(screen.queryByText("Checking…")).not.toBeInTheDocument();
  });
});
