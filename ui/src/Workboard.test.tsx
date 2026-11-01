import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BoardAct, BoardCard, BoardLane, BoardPage, BoardWork } from "./api";

const api = vi.hoisted(() => ({
  getWorkboard: vi.fn<() => Promise<BoardPage>>(),
  workboardAdd: vi.fn<(title: string, detail: string) => Promise<BoardPage>>(),
  workboardAct: vi.fn<(id: string, act: BoardAct) => Promise<BoardPage>>(),
  workboardWork: vi.fn<(id: string) => Promise<BoardWork>>(),
}));

vi.mock("./api", () => api);

import { BOARD_EMPTY, Workboard } from "./Workboard";

const LABELS: Record<BoardLane, string> = {
  followUp: "Follow up",
  todo: "Todo",
  doing: "Doing",
  blocked: "Blocked",
  done: "Done",
};

function card(over: Partial<BoardCard> = {}): BoardCard {
  return {
    id: "w-1",
    title: "Write release notes",
    detail: "",
    notes: "",
    priority: "",
    fresh: false,
    report: "",
    ...over,
  };
}

function board(cards: Partial<Record<BoardLane, BoardCard[]>>, archived: BoardCard[] = []): BoardPage {
  return {
    columns: (Object.keys(LABELS) as BoardLane[]).map((lane) => ({
      lane,
      label: LABELS[lane],
      cards: cards[lane] ?? [],
    })),
    archived,
  };
}

const TODO = board({ todo: [card()], doing: [card({ id: "w-2", title: "Fix the build" })] });
const MOVED = board({ doing: [card({ id: "w-2", title: "Fix the build" }), card()] });

function column(name: string) {
  return screen.getByRole("region", { name });
}

function inCard(name: string) {
  return within(screen.getByRole("listitem", { name }));
}

describe("Workboard", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("says what to do on an empty board, and adds a card to Todo", async () => {
    api.getWorkboard.mockResolvedValue(board({}));
    api.workboardAdd.mockResolvedValue(board({ todo: [card()] }));
    render(<Workboard onWork={() => {}} />);
    expect(await screen.findByText(BOARD_EMPTY)).toBeVisible();

    await userEvent.type(screen.getByLabelText("Card title"), "Write release notes");
    await userEvent.type(screen.getByLabelText("Card detail"), "for 3.6{Enter}");
    expect(api.workboardAdd).toHaveBeenCalledWith("Write release notes", "for 3.6");
    expect(within(column("Todo")).getByText("Write release notes")).toBeVisible();
    expect(screen.getByLabelText("Card title")).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Added to Todo.");
  });

  it("drags a card to Doing and calls move", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(MOVED);
    render(<Workboard onWork={() => {}} />);
    const item = await screen.findByRole("listitem", { name: "Write release notes" });
    expect(item).toHaveAttribute("draggable", "true");
    // Follow up is empty, so it shows only while a card is dragged.
    expect(screen.queryByRole("region", { name: "Follow up" })).toBeNull();

    const setData = vi.fn();
    fireEvent.dragStart(item, { dataTransfer: { setData, effectAllowed: "" } });
    expect(setData).toHaveBeenCalledWith("application/x-workboard-card", "w-1");
    expect(column("Follow up")).toBeVisible();
    fireEvent.dragOver(column("Doing"));
    fireEvent.drop(column("Doing"));

    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "move", to: "doing" });
    expect(await within(column("Doing")).findByText("Write release notes")).toBeVisible();
    expect(within(column("Todo")).queryByText("Write release notes")).toBeNull();
  });

  it("a drop on the card's own column does nothing", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    render(<Workboard onWork={() => {}} />);
    const item = await screen.findByRole("listitem", { name: "Write release notes" });
    fireEvent.dragStart(item);
    fireEvent.drop(column("Todo"));
    expect(api.workboardAct).not.toHaveBeenCalled();
  });

  it("moves a card from the keyboard with the Move menu", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(MOVED);
    render(<Workboard onWork={() => {}} />);
    const move = await screen.findAllByRole("button", { name: "Move" });
    move[0].focus();
    await userEvent.keyboard("{Enter}");

    const menu = screen.getByRole("menu", { name: "Move to" });
    expect(within(menu).getAllByRole("menuitem").map((b) => b.textContent)).toEqual([
      "Follow up",
      "Doing",
      "Blocked",
      "Done",
    ]);
    expect(within(menu).getByRole("menuitem", { name: "Follow up" })).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(within(menu).getByRole("menuitem", { name: "Doing" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");

    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "move", to: "doing" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(await within(column("Doing")).findByText("Write release notes")).toBeVisible();
  });

  it("Escape closes the Move menu without moving", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    render(<Workboard onWork={() => {}} />);
    const move = (await screen.findAllByRole("button", { name: "Move" }))[0];
    await userEvent.click(move);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(move).toHaveFocus();
    expect(api.workboardAct).not.toHaveBeenCalled();
  });

  it("Work fills the chat box with the card's prompt and sends nothing", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardWork.mockResolvedValue({
      prompt: "Work on this workboard card: Write release notes\n\nWhen it is done, say so in one line.",
      page: MOVED,
    });
    const onWork = vi.fn();
    render(<Workboard onWork={onWork} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Work" }));

    expect(api.workboardWork).toHaveBeenCalledWith("w-1");
    expect(onWork).toHaveBeenCalledWith(
      "Work on this workboard card: Write release notes\n\nWhen it is done, say so in one line.",
    );
    expect(onWork).toHaveBeenCalledTimes(1);
    expect(within(column("Doing")).getByText("Write release notes")).toBeVisible();
    expect(api.workboardAct).not.toHaveBeenCalled();
  });

  it("a done card has no Work button", async () => {
    api.getWorkboard.mockResolvedValue(board({ done: [card()] }));
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    expect(inCard("Write release notes").getByRole("button", { name: "Edit" })).toBeVisible();
    expect(inCard("Write release notes").queryByRole("button", { name: "Work" })).toBeNull();
  });

  it("asks before deleting, and Keep leaves the card", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(board({ doing: [card({ id: "w-2", title: "Fix the build" })] }));
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Delete" }));
    expect(api.workboardAct).not.toHaveBeenCalled();
    const ask = inCard("Write release notes").getByRole("group", { name: "Delete this card?" });
    expect(within(ask).getByText("Delete this card for good?")).toBeVisible();

    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(api.workboardAct).not.toHaveBeenCalled();
    expect(inCard("Write release notes").getByRole("button", { name: "Delete" })).toBeVisible();

    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Delete" }));
    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Delete card" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "delete" });
    expect(screen.queryByRole("listitem", { name: "Write release notes" })).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent("Deleted Write release notes.");
  });

  it("saves notes for the agent", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(
      board({ todo: [card({ notes: "Use the fragments" })], doing: [card({ id: "w-2", title: "Fix the build" })] }),
    );
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Add notes" }));
    await userEvent.type(screen.getByLabelText("Notes for the agent"), "Use the fragments");
    await userEvent.click(screen.getByRole("button", { name: "Save notes" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "notes", notes: "Use the fragments" });
    expect(inCard("Write release notes").getByText("Notes: Use the fragments")).toBeVisible();
    expect(inCard("Write release notes").getByRole("button", { name: "Edit notes" })).toBeVisible();
  });

  it("shows archived cards on the toggle and restores one", async () => {
    api.getWorkboard.mockResolvedValue(board({ todo: [card()] }, [card({ id: "w-5", title: "Old idea" })]));
    api.workboardAct.mockResolvedValue(board({ todo: [card(), card({ id: "w-5", title: "Old idea" })] }));
    render(<Workboard onWork={() => {}} />);
    const toggle = await screen.findByRole("button", { name: "Archived (1)" });
    expect(screen.queryByRole("list", { name: "Archived cards" })).toBeNull();
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    const archived = screen.getByRole("list", { name: "Archived cards" });
    await userEvent.click(within(archived).getByRole("button", { name: "Restore" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-5", { kind: "restore" });
    expect(within(column("Todo")).getByText("Old idea")).toBeVisible();
  });

  it("shows the error and the file's state when a change fails", async () => {
    api.getWorkboard
      .mockResolvedValueOnce(TODO)
      .mockResolvedValueOnce(board({ doing: [card({ id: "w-2", title: "Fix the build" })] }));
    api.workboardAct.mockRejectedValue("That card is gone. It may have been removed in the cabin.");
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(inCard("Write release notes").getByRole("button", { name: "Archive" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That card is gone. It may have been removed in the cabin.",
    );
    expect(screen.queryByRole("listitem", { name: "Write release notes" })).toBeNull();
  });

  it("shows a board file it can't read as an error", async () => {
    api.getWorkboard.mockRejectedValue("workboard.json: expected value at line 1 column 3");
    render(<Workboard onWork={() => {}} />);
    expect(await screen.findByRole("status")).toHaveTextContent("workboard.json: expected value");
    expect(screen.queryByText(BOARD_EMPTY)).toBeNull();
  });
});
