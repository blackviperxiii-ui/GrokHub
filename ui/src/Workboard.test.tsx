import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkAct, WorkBoard, WorkCard, WorkLogLine, WorkResume } from "./api";

const api = vi.hoisted(() => ({
  getWorkboard: vi.fn<() => Promise<WorkBoard>>(),
  workboardStamp: vi.fn<() => Promise<number>>(),
  workboardLog: vi.fn<(id: string) => Promise<WorkLogLine[]>>(),
  workboardAct: vi.fn<(id: string, act: WorkAct) => Promise<WorkBoard>>(),
  workboardResume: vi.fn<(id: string, conversationId: string) => Promise<WorkResume>>(),
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import { STAMP_EVERY_MS, Workboard, WORKBOARD_EMPTY } from "./Workboard";

function card(over: Partial<WorkCard> = {}): WorkCard {
  return {
    id: "w1",
    title: "Write the parser",
    detail: "",
    notes: "",
    column: "todo",
    owner: "You",
    kind: null,
    leftOff: null,
    steps: [],
    checkpoint: null,
    progress: [0, 4],
    canResume: false,
    chat: null,
    updatedMs: 0,
    fresh: false,
    ...over,
  };
}

const PAUSED = card({
  id: "p1",
  title: "Port the lexer",
  column: "blocked",
  owner: "Agent",
  kind: "Background",
  leftOff: "Finish the lexer",
  canResume: true,
  chat: "chat-p1",
  progress: [1, 3],
  steps: [
    { text: "Read the spec", status: "done" },
    { text: "Write the lexer", status: "doing" },
    { text: "Test it", status: "planned" },
  ],
  checkpoint: { goal: "Parse .tsa files", next: "Finish the lexer", blocker: "Which grammar?", refs: [] },
});

function board(over: Partial<Record<"todo" | "doing" | "blocked" | "done", WorkCard[]>> = {}, extra: Partial<WorkBoard> = {}): WorkBoard {
  return {
    followUp: [],
    columns: [
      { id: "todo", label: "Todo", cards: over.todo ?? [] },
      { id: "doing", label: "Doing", cards: over.doing ?? [] },
      { id: "blocked", label: "Blocked", cards: over.blocked ?? [] },
      { id: "done", label: "Done", cards: over.done ?? [] },
    ],
    archived: [],
    stamp: 1,
    ...extra,
  };
}

function column(name: string) {
  return within(screen.getByRole("region", { name }));
}

function item(name: string) {
  return within(screen.getByRole("listitem", { name }));
}

describe("Workboard", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.workboardStamp.mockResolvedValue(1);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("says what shows here when there are no cards", async () => {
    api.getWorkboard.mockResolvedValue(board({}, { stamp: 0 }));
    render(<Workboard onResume={() => {}} />);
    expect(await screen.findByText(WORKBOARD_EMPTY)).toBeVisible();
  });

  it("renders the columns with each card's badge, left off and progress", async () => {
    api.getWorkboard.mockResolvedValue(board({ todo: [card()], blocked: [PAUSED] }));
    render(<Workboard onResume={() => {}} />);
    await screen.findByRole("region", { name: "Todo" });
    expect(column("Todo").getByText("Write the parser")).toBeVisible();
    for (const name of ["Todo", "Doing", "Blocked", "Done"]) expect(column(name).getByRole("heading")).toBeVisible();
    const paused = item("Port the lexer");
    expect(paused.getByText("Agent")).toBeVisible();
    expect(paused.getByText("Background")).toBeVisible();
    expect(paused.getByText(/Left off: Finish the lexer/)).toBeVisible();
    expect(paused.getByRole("img", { name: "1 of 3" })).toBeInTheDocument();
    expect(item("Write the parser").getByText("You")).toBeVisible();
  });

  it("opens a card to its steps and where it left off, and reads the log only when asked", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardLog.mockResolvedValue([{ at: Date.now(), kind: "step", text: "Wrote the reader" }]);
    render(<Workboard onResume={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /Port the lexer/ }));
    const open = item("Port the lexer");
    const steps = within(open.getByRole("list", { name: "Steps" })).getAllByRole("listitem");
    expect(steps.map((s) => s.textContent)).toEqual([
      "✓ Read the spec (done)",
      "→ Write the lexer (doing)",
      "· Test it (planned)",
    ]);
    expect(open.getByText("Which grammar?")).toBeVisible();
    expect(api.workboardLog).not.toHaveBeenCalled();

    await userEvent.click(open.getByRole("button", { name: "Log" }));
    expect(api.workboardLog).toHaveBeenCalledWith("p1");
    expect(await open.findByText("just now · step · Wrote the reader")).toBeVisible();
    await userEvent.click(open.getByRole("button", { name: "Log" }));
    expect(open.queryByText("just now · step · Wrote the reader")).toBeNull();
  });

  it("Resume calls the resume command and hands the draft to the chat", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardResume.mockResolvedValue({
      conversationId: "chat-p1",
      prompt: "Resume the work card",
      board: board({ blocked: [PAUSED] }),
    });
    const onResume = vi.fn();
    render(<Workboard onResume={onResume} />);
    await userEvent.click(await screen.findByRole("button", { name: /Port the lexer/ }));
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(api.workboardResume).toHaveBeenCalledWith("p1", "chat-p1");
    expect(onResume).toHaveBeenCalledWith({ conversationId: "chat-p1", prompt: "Resume the work card" });
  });

  it("Hand back, move, archive and a confirmed delete go through act", async () => {
    const mine = { ...PAUSED, owner: "You" as const, column: "todo" as const, canResume: false };
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardAct
      .mockResolvedValueOnce(board({ todo: [mine] }))
      .mockResolvedValueOnce(board({ doing: [{ ...mine, column: "doing" }] }))
      .mockResolvedValueOnce(board({}, { archived: [{ ...mine, column: "archived" }] }))
      .mockResolvedValueOnce(board({}, { stamp: 2 }));
    render(<Workboard onResume={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /Port the lexer/ }));

    await userEvent.click(screen.getByRole("button", { name: "Hand back" }));
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", { kind: "handBack" });
    expect(column("Todo").getByText("You")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();

    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Move to" }), "doing");
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", { kind: "move", column: "doing" });
    expect(column("Doing").getByText("Port the lexer")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "Archive" }));
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", { kind: "archive" });
    expect(screen.getByText("Archived (1)")).toBeInTheDocument();

    await userEvent.click(screen.getByText("Archived (1)"));
    await userEvent.click(item("Port the lexer").getByRole("button", { name: "Delete" }));
    expect(api.workboardAct).toHaveBeenCalledTimes(3);
    await userEvent.click(screen.getByRole("group", { name: 'Delete "Port the lexer"?' }).querySelector("button.danger")!);
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", { kind: "delete" });
    expect(screen.getByRole("status")).toHaveTextContent("Deleted.");
  });

  it("is read only in the web app", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    render(<Workboard readOnly onResume={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: /Port the lexer/ }));
    for (const name of ["Resume", "Hand back", "Archive", "Delete", "Move up"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    expect(screen.getByRole("button", { name: "Log" })).toBeVisible();
  });

  it("opens the card the Activity dock asks for", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    const onCards = vi.fn();
    render(<Workboard onResume={() => {}} onCards={onCards} openCard={{ id: "p1", n: 1 }} />);
    await screen.findByRole("listitem", { name: "Port the lexer" });
    expect(item("Port the lexer").getByRole("list", { name: "Steps" })).toBeVisible();
    expect(onCards).toHaveBeenCalledWith({ "chat-p1": "p1" });
  });

  it("reloads when the cabin saves the board", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.getWorkboard
      .mockResolvedValueOnce(board({ todo: [card()] }))
      .mockResolvedValueOnce(board({ doing: [card({ column: "doing" })] }, { stamp: 2 }));
    render(<Workboard onResume={() => {}} />);
    await screen.findByRole("region", { name: "Todo" });
    expect(column("Todo").getByText("Write the parser")).toBeVisible();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STAMP_EVERY_MS);
    });
    expect(api.getWorkboard).toHaveBeenCalledTimes(1);
    api.workboardStamp.mockResolvedValue(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STAMP_EVERY_MS);
    });
    await waitFor(() => expect(column("Doing").getByText("Write the parser")).toBeVisible());
    expect(api.getWorkboard).toHaveBeenCalledTimes(2);
  });
});
