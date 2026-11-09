import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LearnedAct, LearnedRow, LearnedView } from "./api";

const api = vi.hoisted(() => ({
  getLearned: vi.fn<() => Promise<LearnedView>>(),
  learnedAct: vi.fn<(act: LearnedAct) => Promise<LearnedView>>(),
}));

vi.mock("./api", () => api);

import { LEARNED_EMPTY, Learned } from "./Learned";

function row(over: Partial<LearnedRow> = {}): LearnedRow {
  return {
    key: "card:suggestion",
    title: "Suggestion cards",
    says: "You take these about 8 times in 10.",
    evidence: 10,
    sure: 54,
    stage: "suggesting",
    lastMs: 1_800_000_000_000,
    ...over,
  };
}

function line(name: string) {
  const list = within(screen.getByRole("list", { name: "Learned" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Learned", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("says so when nothing is learned, with no Forget everything", async () => {
    api.getLearned.mockResolvedValue({ rows: [], singleChips: 0 });
    render(<Learned />);
    expect(await screen.findByText(LEARNED_EMPTY)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Forget everything…" })).toBeNull();
  });

  it("lists each belief with what it says and how sure it is", async () => {
    api.getLearned.mockResolvedValue({
      rows: [row(), row({ key: "model:grok-code/run-ms", title: "How long grok-code takes", says: "A run takes about 3.0 seconds.", evidence: 1, sure: 27, stage: "measuring" })],
      singleChips: 4,
    });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    expect(line("Suggestion cards").getByText("You take these about 8 times in 10.")).toBeVisible();
    expect(line("Suggestion cards").getByText("Seen 10 times · 54% sure · Sure enough to suggest")).toBeVisible();
    expect(line("How long grok-code takes").getByText("Seen once · 27% sure · A measurement")).toBeVisible();
    expect(screen.getByText("Plus counts for 4 single quick chips, which rank the chip rows.")).toBeVisible();
  });

  it("forgets one line and shows the list that comes back", async () => {
    api.getLearned.mockResolvedValue({ rows: [row(), row({ key: "skill:tidy", title: "Skill tidy" })], singleChips: 0 });
    api.learnedAct.mockResolvedValue({ rows: [row({ key: "skill:tidy", title: "Skill tidy" })], singleChips: 0 });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(line("Suggestion cards").getByRole("button", { name: "Forget" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "forget", key: "card:suggestion" });
    expect(screen.getByRole("status")).toHaveTextContent("Forgot Suggestion cards.");
    expect(screen.queryByRole("listitem", { name: "Suggestion cards" })).toBeNull();
    expect(line("Skill tidy").getByRole("button", { name: "Forget" })).toBeVisible();
  });

  it("marks a line wrong, which leaves it listed without buttons", async () => {
    api.getLearned.mockResolvedValue({ rows: [row()], singleChips: 0 });
    api.learnedAct.mockResolvedValue({
      rows: [row({ says: "You said this is wrong. It is not counted or used.", evidence: 0, sure: 0, stage: "wrong" })],
      singleChips: 0,
    });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(line("Suggestion cards").getByRole("button", { name: "That's wrong" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "wrong", key: "card:suggestion" });
    expect(line("Suggestion cards").getByText("Marked wrong")).toBeVisible();
    expect(line("Suggestion cards").queryByRole("button")).toBeNull();
  });

  it("asks before forgetting everything, and Cancel changes nothing", async () => {
    api.getLearned.mockResolvedValue({ rows: [row()], singleChips: 0 });
    api.learnedAct.mockResolvedValue({ rows: [], singleChips: 0 });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(screen.getByRole("button", { name: "Forget everything…" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.learnedAct).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Forget everything…" }));
    await userEvent.click(screen.getByRole("button", { name: "Forget everything" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "forgetAll" });
    expect(screen.getByRole("status")).toHaveTextContent("Forgot everything.");
    expect(screen.getByText(LEARNED_EMPTY)).toBeVisible();
  });

  it("shows why when the list can't load", async () => {
    api.getLearned.mockRejectedValue("The learning files can't be read.");
    render(<Learned />);
    expect(await screen.findByRole("status")).toHaveTextContent("The learning files can't be read.");
  });
});
