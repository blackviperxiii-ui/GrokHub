import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeChats, ClaudePlan } from "./api";

const api = vi.hoisted(() => ({
  claudePlanStatus: vi.fn<() => Promise<ClaudePlan>>(),
  claudePlanChats: vi.fn<() => Promise<ClaudeChats>>(),
  setClaudePlanAllowed: vi.fn<(on: boolean) => Promise<ClaudeChats>>(),
}));
vi.mock("./api", () => api);

import {
  CLAUDE_ALLOW,
  CLAUDE_ALLOWED,
  CLAUDE_MISSING,
  CLAUDE_NO_ANSWER,
  ClaudePlanCard,
  claudePlanLine,
  headroomLine,
} from "./ClaudePlan";

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

  it("says how much of the plan's limit the last chat saw used, until it resets", () => {
    // Resets Sun, Nov 22, 2026, 4:00 AM in Chicago (10:00 UTC); seen Mon, Nov 16, 7:00 PM.
    const week = { window: "seven_day", usedPct: 43, resetsAt: 1_795_341_600, limited: false, seenAt: 1_794_877_200 };
    const now = 1_794_877_300;
    const clock = { timeZone: "America/Chicago", locale: "en-US" };
    expect(headroomLine(week, now, clock)).toBe(
      "Your last chat on it (Mon, Nov 16, 7:00 PM) saw 43% of your plan's weekly limit used. It resets Sun, Nov 22, 4:00 AM.",
    );
    expect(headroomLine({ ...week, window: "five_hour", limited: true, usedPct: 100 }, now, clock)).toBe(
      "Your last chat on it (Mon, Nov 16, 7:00 PM) found your plan's 5-hour limit reached. Chats on it can run again Sun, Nov 22, 4:00 AM.",
    );
    expect(headroomLine({ ...week, usedPct: null }, now, clock)).toBe(
      "Your plan's weekly limit resets Sun, Nov 22, 4:00 AM.",
    );
    expect(headroomLine({ ...week, usedPct: null, resetsAt: null }, now, clock)).toBeNull();
    expect(headroomLine(week, 1_795_341_600, clock)).toBeNull();

    // No reset time: shown for five hours after it was seen, then dropped.
    const untimed = { ...week, window: "new_kind", resetsAt: null, limited: true };
    expect(headroomLine(untimed, week.seenAt + 4 * 3600, clock)).toBe(
      "Your last chat on it (Mon, Nov 16, 7:00 PM) found your plan's usage limit reached.",
    );
    expect(headroomLine(untimed, week.seenAt + 5 * 3600, clock)).toBeNull();
  });

  it("shows the reading under Allowed, in red once the limit is reached, and drops it when it resets", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      api.claudePlanStatus.mockResolvedValue({ state: "plan", version: "2.1.300" });
      const nowSecs = Date.now() / 1000;
      const reached = { window: "five_hour", usedPct: 100, resetsAt: nowSecs + 90, limited: true, seenAt: nowSecs };
      api.claudePlanChats.mockResolvedValue({ allowed: true, chats: [], headroom: reached });
      render(<ClaudePlanCard />);
      const line = await screen.findByText(/found your plan's 5-hour limit reached/);
      expect(line).toHaveClass("status--error");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(120_000);
      });
      expect(screen.queryByText(/found your plan's 5-hour limit reached/)).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows a failed check instead of a blank", async () => {
    api.claudePlanStatus.mockRejectedValue("AI models is only on the desktop app.");
    render(<ClaudePlanCard />);
    expect(await screen.findByRole("alert")).toHaveTextContent("AI models is only on the desktop app.");
    expect(screen.queryByText("Checking…")).not.toBeInTheDocument();
  });
});
