import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BudgetAnswered, BudgetCardData } from "./spendApi";

const spend = vi.hoisted(() => ({
  budgetCard: vi.fn<() => Promise<BudgetCardData | null>>(),
  answerBudgetCard: vi.fn<(key: string, primary: boolean) => Promise<BudgetAnswered>>(),
}));
vi.mock("./spendApi", () => spend);

import { BudgetCard } from "./BudgetCard";

const ASK: BudgetCardData = {
  key: "ask",
  eyebrow: "This week's usage",
  title: "You've used this week's budget. Keep chatting this week anyway?",
  note: "Background work stays paused until next week.",
  primary: "Keep going",
  secondary: "Wait",
};

describe("BudgetCard", () => {
  beforeEach(() => {
    spend.budgetCard.mockReset().mockResolvedValue(null);
    spend.answerBudgetCard.mockReset();
  });

  it("shows nothing while a reply runs, then the ask once the chat is idle, and Keep going answers it", async () => {
    spend.budgetCard.mockResolvedValue(ASK);
    const { rerender } = render(<BudgetCard busy onOpenSettings={() => {}} />);
    expect(spend.budgetCard).not.toHaveBeenCalled();
    rerender(<BudgetCard busy={false} onOpenSettings={() => {}} />);
    const card = await screen.findByRole("group", { name: "This week's usage" });
    expect(card).toHaveTextContent("Keep chatting this week anyway?");

    spend.answerBudgetCard.mockResolvedValue({
      status: "Chats go on this week. Background work waits.",
      openAiModels: false,
      details: null,
    });
    await userEvent.click(screen.getByRole("button", { name: "Keep going" }));
    expect(spend.answerBudgetCard).toHaveBeenCalledWith("ask", true);
    expect(await screen.findByRole("status")).toHaveTextContent("Chats go on this week. Background work waits.");
    expect(screen.queryByRole("button", { name: "Keep going" })).toBeNull();
  });

  it("Change cap opens Settings, and the nudge's Details show in place", async () => {
    const open = vi.fn();
    spend.budgetCard.mockResolvedValueOnce({
      ...ASK,
      key: "eighty",
      title: "You've used 80% of this week's usage.",
      primary: "OK",
      secondary: "Change cap",
    });
    const { unmount } = render(<BudgetCard busy={false} onOpenSettings={open} />);
    spend.answerBudgetCard.mockResolvedValue({ status: "", openAiModels: true, details: null });
    await userEvent.click(await screen.findByRole("button", { name: "Change cap" }));
    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    expect(spend.answerBudgetCard).toHaveBeenCalledWith("eighty", false);
    unmount();

    spend.budgetCard.mockResolvedValueOnce({
      ...ASK,
      key: "nudge",
      eyebrow: "Your plan",
      title: "A bigger Grok plan includes more capacity. Want details?",
      primary: "Details",
      secondary: "Not now",
    });
    render(<BudgetCard busy={false} onOpenSettings={open} />);
    spend.answerBudgetCard.mockResolvedValue({
      status: "",
      openAiModels: false,
      details: "Plans and their limits are on grok.com, under your account.",
    });
    await userEvent.click(await screen.findByRole("button", { name: "Details" }));
    expect(await screen.findByRole("note", { name: "Plan details" })).toHaveTextContent(
      "Plans and their limits are on grok.com",
    );
    await act(async () => {
      await userEvent.click(screen.getByRole("button", { name: "Close" }));
    });
    expect(screen.queryByRole("note")).toBeNull();
    expect(open).toHaveBeenCalledTimes(1);
  });
});
