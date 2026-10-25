import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { JournalAct, JournalMission, JournalPage, JournalQuestion } from "./api";

const api = vi.hoisted(() => ({
  getJournal: vi.fn<() => Promise<JournalPage>>(),
  journalAdd: vi.fn<(question: string) => Promise<JournalPage>>(),
  journalAct: vi.fn<(id: string, act: JournalAct) => Promise<JournalPage>>(),
  journalToMission: vi.fn<(id: string, conversationId: string) => Promise<JournalMission>>(),
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import { Journal, JOURNAL_EMPTY } from "./Journal";

function question(over: Partial<JournalQuestion> = {}): JournalQuestion {
  return {
    id: "q1",
    question: "Ship the shell first?",
    status: "Open",
    active: true,
    options: [
      { id: "o1", label: "Yes" },
      { id: "o2", label: "Wait" },
    ],
    leaning: null,
    chosen: null,
    mission: null,
    ...over,
  };
}

const EMPTY: JournalPage = { active: [], history: [] };

function card(name: string) {
  return within(screen.getByRole("listitem", { name }));
}

describe("Journal", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("says what to do when there are no questions, and adds one", async () => {
    api.getJournal.mockResolvedValue(EMPTY);
    api.journalAdd.mockResolvedValue({ active: [question()], history: [] });
    render(<Journal onMission={() => {}} />);
    expect(await screen.findByText(JOURNAL_EMPTY)).toBeVisible();

    const box = screen.getByLabelText("A decision you're weighing");
    await userEvent.type(box, "Ship the shell first?{Enter}");
    expect(api.journalAdd).toHaveBeenCalledWith("Ship the shell first?");
    expect(card("Ship the shell first?").getByText("Open")).toBeVisible();
    expect(box).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Added.");
  });

  it("leans, then decides on the leaning", async () => {
    api.getJournal.mockResolvedValue({ active: [question()], history: [] });
    api.journalAct
      .mockResolvedValueOnce({ active: [question({ leaning: "o2" })], history: [] })
      .mockResolvedValueOnce({
        active: [],
        history: [question({ status: "Decided", active: false, chosen: "Wait" })],
      });
    render(<Journal onMission={() => {}} />);
    expect(await screen.findByRole("button", { name: "Decide" })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Wait" }));
    expect(api.journalAct).toHaveBeenCalledWith("q1", { kind: "lean", optionId: "o2" });
    expect(screen.getByRole("button", { name: "Wait" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Open · leaning Wait")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Decide" }));
    expect(api.journalAct).toHaveBeenLastCalledWith("q1", { kind: "decide", optionId: "o2" });
    expect(screen.getByRole("status")).toHaveTextContent("Decided: Wait.");
    expect(screen.getByText(JOURNAL_EMPTY)).toBeVisible();
    expect(screen.getByText("History (1)")).toBeVisible();
    expect(screen.getByText("Decided · Wait", { selector: ".card-status" })).toBeInTheDocument();
  });

  it("adds an option and hides the box at five", async () => {
    const four = question({
      options: ["a", "b", "c", "d"].map((label) => ({ id: label, label })),
    });
    api.getJournal.mockResolvedValue({ active: [four], history: [] });
    api.journalAct.mockResolvedValue({
      active: [question({ options: [...four.options, { id: "e", label: "e" }] })],
      history: [],
    });
    render(<Journal onMission={() => {}} />);
    await userEvent.type(await screen.findByLabelText("New option"), "e{Enter}");
    expect(api.journalAct).toHaveBeenCalledWith("q1", { kind: "addOption", label: "e" });
    expect(screen.getByRole("button", { name: "e" })).toBeVisible();
    expect(screen.queryByLabelText("New option")).toBeNull();
  });

  it("To mission opens a new chat the first time and its own chat after", async () => {
    const mission: JournalMission = {
      conversationId: "shell-new",
      title: "Ship the shell first?",
      prompt: "Mission from my decision journal: Ship the shell first?",
    };
    const onMission = vi.fn();
    api.getJournal
      .mockResolvedValueOnce({ active: [question()], history: [] })
      .mockResolvedValue({
        active: [question({ status: "Investigating", mission: "shell-new" })],
        history: [],
      });
    api.journalToMission.mockResolvedValue(mission);
    render(<Journal onMission={onMission} />);

    await userEvent.click(await screen.findByRole("button", { name: "To mission" }));
    expect(api.journalToMission).toHaveBeenCalledWith("q1", "shell-new");
    expect(onMission).toHaveBeenCalledWith(mission);
    expect(await screen.findByText("Investigating")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Back to mission" }));
    expect(api.journalToMission).toHaveBeenLastCalledWith("q1", "shell-new");
  });

  it("a deferred question only reopens or abandons", async () => {
    api.getJournal.mockResolvedValue({
      active: [question({ status: "Deferred" })],
      history: [],
    });
    api.journalAct.mockResolvedValue({ active: [question()], history: [] });
    render(<Journal onMission={() => {}} />);
    const buttons = within(await screen.findByRole("listitem", { name: "Ship the shell first?" }))
      .getAllByRole("button")
      .map((b) => b.textContent);
    expect(buttons).toEqual(["Yes", "Wait", "Reopen", "Abandon"]);
    await userEvent.click(screen.getByRole("button", { name: "Reopen" }));
    expect(api.journalAct).toHaveBeenCalledWith("q1", { kind: "reopen" });
    expect(screen.getByRole("button", { name: "To mission" })).toBeVisible();
  });

  it("shows the error and reloads when the cabin removed the question", async () => {
    api.getJournal
      .mockResolvedValueOnce({ active: [question()], history: [] })
      .mockResolvedValue(EMPTY);
    api.journalAct.mockRejectedValue("That question is gone. It may have been removed in the cabin.");
    render(<Journal onMission={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Defer" }));
    expect(screen.getByRole("status")).toHaveTextContent(
      "That question is gone. It may have been removed in the cabin.",
    );
    expect(await screen.findByText(JOURNAL_EMPTY)).toBeVisible();
  });
});
