import "@testing-library/jest-dom/vitest";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BoardAct,
  BoardCard,
  BoardLane,
  BoardPage,
  BoardResume,
  BoardWork,
  PlaybookPage,
  WorkLogLine,
} from "./api";

const api = vi.hoisted(() => ({
  getWorkboard: vi.fn<() => Promise<BoardPage>>(),
  workboardStamp: vi.fn<() => Promise<number>>(),
  workboardLog: vi.fn<(id: string) => Promise<WorkLogLine[]>>(),
  workboardAdd: vi.fn<(title: string, detail: string) => Promise<BoardPage>>(),
  workboardAct: vi.fn<(id: string, act: BoardAct) => Promise<BoardPage>>(),
  workboardWork: vi.fn<(id: string) => Promise<BoardWork>>(),
  workboardResume:
    vi.fn<(id: string, conversationId: string) => Promise<BoardResume>>(),
  workboardDeleteArchived: vi.fn<(days: number) => Promise<BoardPage>>(),
  workboardSweepUndo: vi.fn<() => Promise<BoardPage>>(),
  workboardSweepDismiss: vi.fn<() => Promise<BoardPage>>(),
  workboardPlaybook:
    vi.fn<(id: string, playbook: string, name: string) => Promise<BoardWork>>(),
  workboardRecur: vi.fn<(id: string, choice: number) => Promise<BoardPage>>(),
  getPlaybooks: vi.fn<() => Promise<PlaybookPage>>(),
  RECUR_CHOICES: [
    "Every day at 9 AM",
    "Every weekday at 9 AM",
    "Every day at 6 PM",
  ],
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import {
  BOARD_EMPTY,
  STAMP_EVERY_MS,
  Workboard,
  olderCount,
} from "./Workboard";

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
    owner: "You",
    kind: null,
    leftOff: null,
    steps: [],
    checkpoint: null,
    progress: [0, 4],
    canResume: false,
    chat: null,
    updatedMs: 0,
    archivedMs: 0,
    queued: null,
    flow: null,
    userSteps: false,
    stepsText: "",
    ...over,
  };
}

function board(
  cards: Partial<Record<BoardLane, BoardCard[]>>,
  archived: BoardCard[] = [],
  stamp = 1,
): BoardPage {
  return {
    columns: (Object.keys(LABELS) as BoardLane[]).map((lane) => ({
      lane,
      label: LABELS[lane],
      cards: cards[lane] ?? [],
    })),
    archived,
    stamp,
    swept: null,
    queueNote: null,
  };
}

const TODO = board({
  todo: [card()],
  doing: [card({ id: "w-2", title: "Fix the build" })],
});
const MOVED = board({
  doing: [card({ id: "w-2", title: "Fix the build" }), card()],
});

const PAUSED = card({
  id: "p1",
  title: "Port the lexer",
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
  checkpoint: {
    goal: "Parse .tsa files",
    next: "Finish the lexer",
    blocker: "Which grammar?",
    refs: [],
  },
});

function column(name: string) {
  return screen.getByRole("region", { name });
}

function inCard(name: string) {
  return within(screen.getByRole("listitem", { name }));
}

describe("Workboard", () => {
  beforeEach(() => {
    for (const fn of Object.values(api))
      if (typeof fn === "function") fn.mockReset();
    api.workboardStamp.mockResolvedValue(1);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("says what to do on an empty board, and adds a card to Todo", async () => {
    api.getWorkboard.mockResolvedValue(board({}));
    api.workboardAdd.mockResolvedValue(board({ todo: [card()] }));
    render(<Workboard onWork={() => {}} />);
    expect(await screen.findByText(BOARD_EMPTY)).toBeVisible();

    await userEvent.type(
      screen.getByLabelText("Card title"),
      "Write release notes",
    );
    await userEvent.type(
      screen.getByLabelText("Card detail"),
      "for 3.6{Enter}",
    );
    expect(api.workboardAdd).toHaveBeenCalledWith(
      "Write release notes",
      "for 3.6",
    );
    expect(
      within(column("Todo")).getByText("Write release notes"),
    ).toBeVisible();
    expect(screen.getByLabelText("Card title")).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Added to Todo.");
  });

  it("drags a card to Doing and calls move", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(MOVED);
    render(<Workboard onWork={() => {}} />);
    const item = await screen.findByRole("listitem", {
      name: "Write release notes",
    });
    expect(item).toHaveAttribute("draggable", "true");
    // Follow up is empty, so it shows only while a card is dragged.
    expect(screen.queryByRole("region", { name: "Follow up" })).toBeNull();

    const setData = vi.fn();
    fireEvent.dragStart(item, { dataTransfer: { setData, effectAllowed: "" } });
    expect(setData).toHaveBeenCalledWith("application/x-workboard-card", "w-1");
    expect(column("Follow up")).toBeVisible();
    fireEvent.dragOver(column("Doing"));
    fireEvent.drop(column("Doing"));

    expect(api.workboardAct).toHaveBeenCalledWith("w-1", {
      kind: "move",
      to: "doing",
    });
    expect(
      await within(column("Doing")).findByText("Write release notes"),
    ).toBeVisible();
    expect(
      within(column("Todo")).queryByText("Write release notes"),
    ).toBeNull();
  });

  it("a drop on the card's own column does nothing", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    render(<Workboard onWork={() => {}} />);
    const item = await screen.findByRole("listitem", {
      name: "Write release notes",
    });
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
    expect(
      within(menu)
        .getAllByRole("menuitem")
        .map((b) => b.textContent),
    ).toEqual(["Follow up", "Doing", "Blocked", "Done"]);
    expect(
      within(menu).getByRole("menuitem", { name: "Follow up" }),
    ).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(within(menu).getByRole("menuitem", { name: "Doing" })).toHaveFocus();
    await userEvent.keyboard("{Enter}");

    expect(api.workboardAct).toHaveBeenCalledWith("w-1", {
      kind: "move",
      to: "doing",
    });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(
      await within(column("Doing")).findByText("Write release notes"),
    ).toBeVisible();
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
      prompt:
        "Work on this workboard card: Write release notes\n\nWhen it is done, say so in one line.",
      page: MOVED,
    });
    const onWork = vi.fn();
    render(<Workboard onWork={onWork} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(
      inCard("Write release notes").getByRole("button", { name: "Work" }),
    );

    expect(api.workboardWork).toHaveBeenCalledWith("w-1");
    expect(onWork).toHaveBeenCalledWith(
      "Work on this workboard card: Write release notes\n\nWhen it is done, say so in one line.",
    );
    expect(onWork).toHaveBeenCalledTimes(1);
    expect(
      within(column("Doing")).getByText("Write release notes"),
    ).toBeVisible();
    expect(api.workboardAct).not.toHaveBeenCalled();
  });

  it("a done card has no Work button", async () => {
    api.getWorkboard.mockResolvedValue(board({ done: [card()] }));
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    expect(
      inCard("Write release notes").getByRole("button", { name: "Edit" }),
    ).toBeVisible();
    expect(
      inCard("Write release notes").queryByRole("button", { name: "Work" }),
    ).toBeNull();
  });

  it("asks before deleting, and Keep leaves the card", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(
      board({ doing: [card({ id: "w-2", title: "Fix the build" })] }),
    );
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(
      inCard("Write release notes").getByRole("button", { name: "Delete" }),
    );
    expect(api.workboardAct).not.toHaveBeenCalled();
    const ask = inCard("Write release notes").getByRole("group", {
      name: "Delete this card?",
    });
    expect(within(ask).getByText("Delete this card for good?")).toBeVisible();

    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(api.workboardAct).not.toHaveBeenCalled();
    expect(
      inCard("Write release notes").getByRole("button", { name: "Delete" }),
    ).toBeVisible();

    await userEvent.click(
      inCard("Write release notes").getByRole("button", { name: "Delete" }),
    );
    await userEvent.click(
      inCard("Write release notes").getByRole("button", {
        name: "Delete card",
      }),
    );
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "delete" });
    expect(
      screen.queryByRole("listitem", { name: "Write release notes" }),
    ).toBeNull();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Deleted Write release notes.",
    );
  });

  it("saves notes for the agent", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardAct.mockResolvedValue(
      board({
        todo: [card({ notes: "Use the fragments" })],
        doing: [card({ id: "w-2", title: "Fix the build" })],
      }),
    );
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(
      inCard("Write release notes").getByRole("button", { name: "Add notes" }),
    );
    await userEvent.type(
      screen.getByLabelText("Notes for the agent"),
      "Use the fragments",
    );
    await userEvent.click(screen.getByRole("button", { name: "Save notes" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", {
      kind: "notes",
      notes: "Use the fragments",
    });
    expect(
      inCard("Write release notes").getByText("Notes: Use the fragments"),
    ).toBeVisible();
    expect(
      inCard("Write release notes").getByRole("button", { name: "Edit notes" }),
    ).toBeVisible();
  });

  it("shows archived cards on the toggle and restores one", async () => {
    api.getWorkboard.mockResolvedValue(
      board({ todo: [card()] }, [card({ id: "w-5", title: "Old idea" })]),
    );
    api.workboardAct.mockResolvedValue(
      board({ todo: [card(), card({ id: "w-5", title: "Old idea" })] }),
    );
    render(<Workboard onWork={() => {}} />);
    const toggle = await screen.findByRole("button", { name: "Archived (1)" });
    expect(screen.queryByRole("list", { name: "Archived cards" })).toBeNull();
    await userEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-pressed", "true");
    const archived = screen.getByRole("list", { name: "Archived cards" });
    await userEvent.click(
      within(archived).getByRole("button", { name: "Restore" }),
    );
    expect(api.workboardAct).toHaveBeenCalledWith("w-5", { kind: "restore" });
    expect(within(column("Todo")).getByText("Old idea")).toBeVisible();
  });

  it("finds archived cards, newest first as the file sends them, and shows when each was archived", async () => {
    const now = Date.now();
    const day = 86_400_000;
    api.getWorkboard.mockResolvedValue(
      board({ todo: [card()] }, [
        card({ id: "a1", title: "Router notes", archivedMs: now - 2 * day }),
        card({ id: "a2", title: "Sort photos", archivedMs: now - 40 * day }),
      ]),
    );
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Archived (2)" }),
    );
    const list = screen.getByRole("list", { name: "Archived cards" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((li) => li.getAttribute("aria-label")),
    ).toEqual(["Router notes", "Sort photos"]);
    expect(within(list).getByText("Archived 2d ago")).toBeVisible();
    await userEvent.type(
      screen.getByLabelText("Find an archived card"),
      "PHOTO",
    );
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((li) => li.getAttribute("aria-label")),
    ).toEqual(["Sort photos"]);
    await userEvent.clear(screen.getByLabelText("Find an archived card"));
    await userEvent.type(
      screen.getByLabelText("Find an archived card"),
      "nothing like it",
    );
    expect(screen.getByText("No archived card matches.")).toBeVisible();
  });

  it("bulk deletes archived cards only after a confirm that shows the count", async () => {
    const now = Date.now();
    const day = 86_400_000;
    const archived = [
      card({ id: "a1", title: "New", archivedMs: now - day }),
      card({ id: "a2", title: "Mid", archivedMs: now - 45 * day }),
      card({ id: "a3", title: "Old", archivedMs: now - 95 * day }),
    ];
    expect(olderCount(archived, 0, now)).toBe(3);
    expect(olderCount(archived, 30, now)).toBe(2);
    expect(olderCount(archived, 90, now)).toBe(1);
    api.getWorkboard.mockResolvedValue(board({ todo: [card()] }, archived));
    api.workboardDeleteArchived.mockResolvedValue(
      board({ todo: [card()] }, [archived[0]]),
    );
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "Archived (3)" }),
    );
    const bulk = screen.getByRole("group", { name: "Delete archived" });
    expect(
      within(bulk)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual([
      "Delete all archived",
      "Delete archived older than 30 days",
      "Delete archived older than 90 days",
    ]);

    await userEvent.click(
      within(bulk).getByRole("button", { name: "Delete all archived" }),
    );
    let ask = screen.getByRole("group", { name: "Delete archived cards?" });
    expect(
      within(ask).getByText("Delete 3 archived cards and their logs for good?"),
    ).toBeVisible();
    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(api.workboardDeleteArchived).not.toHaveBeenCalled();

    await userEvent.click(
      screen.getByRole("button", {
        name: "Delete archived older than 30 days",
      }),
    );
    ask = screen.getByRole("group", { name: "Delete archived cards?" });
    expect(
      within(ask).getByText("Delete 2 archived cards and their logs for good?"),
    ).toBeVisible();
    await userEvent.click(
      within(ask).getByRole("button", { name: "Delete 2" }),
    );
    expect(api.workboardDeleteArchived).toHaveBeenCalledWith(30);
    expect(screen.getByRole("button", { name: "Archived (1)" })).toBeVisible();
    expect(
      within(column("Todo")).getByText("Write release notes"),
    ).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent(
      "Deleted 2 archived cards.",
    );
  });

  it("shows the sweep's notice with Undo and Dismiss", async () => {
    const swept = {
      ...board({}, [card({ id: "a1", title: "Old run", archivedMs: 5 })]),
    };
    swept.swept = { text: "Archived 1 finished card", ids: ["a1"], at: 5 };
    api.getWorkboard.mockResolvedValue(swept);
    api.workboardSweepUndo.mockResolvedValue(
      board({ todo: [card({ id: "a1", title: "Old run" })] }),
    );
    render(<Workboard onWork={() => {}} />);
    const tidy = await screen.findByRole("group", { name: "Tidy" });
    expect(within(tidy).getByText("Archived 1 finished card")).toBeVisible();
    await userEvent.click(within(tidy).getByRole("button", { name: "Undo" }));
    expect(api.workboardSweepUndo).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("group", { name: "Tidy" })).toBeNull();
    expect(within(column("Todo")).getByText("Old run")).toBeVisible();

    api.getWorkboard.mockResolvedValue(swept);
    api.workboardSweepDismiss.mockResolvedValue(
      board({}, [card({ id: "a1", title: "Old run" })]),
    );
    const { unmount } = render(<Workboard onWork={() => {}} />);
    const again = await screen.findByRole("group", { name: "Tidy" });
    await userEvent.click(
      within(again).getByRole("button", { name: "Dismiss" }),
    );
    expect(api.workboardSweepDismiss).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("group", { name: "Tidy" })).toBeNull();
    unmount();
  });

  it("shows the error and the file's state when a change fails", async () => {
    api.getWorkboard
      .mockResolvedValueOnce(TODO)
      .mockResolvedValueOnce(
        board({ doing: [card({ id: "w-2", title: "Fix the build" })] }),
      );
    api.workboardAct.mockRejectedValue(
      "That card is gone. It may have been removed in the cabin.",
    );
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("listitem", { name: "Write release notes" });
    await userEvent.click(
      inCard("Write release notes").getByRole("button", { name: "Archive" }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That card is gone. It may have been removed in the cabin.",
    );
    expect(
      screen.queryByRole("listitem", { name: "Write release notes" }),
    ).toBeNull();
  });

  it("shows a board file it can't read as an error", async () => {
    api.getWorkboard.mockRejectedValue(
      "workboard.json: expected value at line 1 column 3",
    );
    render(<Workboard onWork={() => {}} />);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "workboard.json: expected value",
    );
    expect(screen.queryByText(BOARD_EMPTY)).toBeNull();
  });

  it("shows each card's badge, left off and progress", async () => {
    api.getWorkboard.mockResolvedValue(
      board({ todo: [card()], blocked: [PAUSED] }),
    );
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("region", { name: "Todo" });
    for (const name of ["Todo", "Doing", "Blocked", "Done"])
      expect(within(column(name)).getByRole("heading")).toBeVisible();
    const paused = inCard("Port the lexer");
    expect(paused.getByText("Agent")).toBeVisible();
    expect(paused.getByText("Background")).toBeVisible();
    expect(paused.getByText(/Left off: Finish the lexer/)).toBeVisible();
    expect(paused.getByRole("img", { name: "1 of 3" })).toBeInTheDocument();
    expect(inCard("Write release notes").getByText("You")).toBeVisible();
  });

  it("opens a card to its steps and where it left off, and reads the log only when asked", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardLog.mockResolvedValue([
      { at: Date.now(), kind: "step", text: "Wrote the reader" },
    ]);
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Port the lexer/ }),
    );
    const open = inCard("Port the lexer");
    const steps = within(
      open.getByRole("list", { name: "Steps" }),
    ).getAllByRole("listitem");
    expect(steps.map((s) => s.textContent)).toEqual([
      "✓ Read the spec (done)",
      "→ Write the lexer (doing)",
      "· Test it (planned)",
    ]);
    expect(open.getByText("Which grammar?")).toBeVisible();
    expect(api.workboardLog).not.toHaveBeenCalled();

    await userEvent.click(open.getByRole("button", { name: "Log" }));
    expect(api.workboardLog).toHaveBeenCalledWith("p1");
    expect(
      await open.findByText("just now · step · Wrote the reader"),
    ).toBeVisible();
    await userEvent.click(open.getByRole("button", { name: "Log" }));
    expect(open.queryByText("just now · step · Wrote the reader")).toBeNull();
  });

  it("Resume calls the resume command and hands the draft to the chat", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardResume.mockResolvedValue({
      conversationId: "chat-p1",
      prompt: "Resume the work card",
      page: board({ blocked: [PAUSED] }),
    });
    const onResume = vi.fn();
    render(<Workboard onWork={() => {}} onResume={onResume} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Port the lexer/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Resume" }));
    expect(api.workboardResume).toHaveBeenCalledWith("p1", "chat-p1");
    expect(onResume).toHaveBeenCalledWith({
      conversationId: "chat-p1",
      prompt: "Resume the work card",
    });
  });

  it("Hand back, Move up and Move down go through act", async () => {
    const mine = { ...PAUSED, owner: "You" as const, canResume: false };
    const other = card({ id: "w-9", title: "Other" });
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    api.workboardAct
      .mockResolvedValueOnce(board({ todo: [other, mine] }))
      .mockResolvedValueOnce(board({ todo: [mine, other] }))
      .mockResolvedValueOnce(board({ todo: [other, mine] }));
    render(<Workboard onWork={() => {}} onResume={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Port the lexer/ }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Hand back" }));
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", {
      kind: "handBack",
    });
    expect(inCard("Port the lexer").getByText("You")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
    expect(
      inCard("Port the lexer").getByRole("button", { name: "Move down" }),
    ).toBeDisabled();

    await userEvent.click(
      inCard("Port the lexer").getByRole("button", { name: "Move up" }),
    );
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", {
      kind: "rank",
      up: true,
    });
    expect(
      inCard("Port the lexer").getByRole("button", { name: "Move up" }),
    ).toBeDisabled();

    await userEvent.click(
      inCard("Port the lexer").getByRole("button", { name: "Move down" }),
    );
    expect(api.workboardAct).toHaveBeenLastCalledWith("p1", {
      kind: "rank",
      up: false,
    });
  });

  it("leaves Resume out in the web app, and keeps Hand back and the log", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    render(
      <Workboard onWork={() => {}} onResume={() => {}} resumable={false} />,
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /Port the lexer/ }),
    );
    expect(screen.queryByRole("button", { name: "Resume" })).toBeNull();
    expect(screen.getByRole("button", { name: "Hand back" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Log" })).toBeVisible();
  });

  it("opens the card the Activity dock asks for", async () => {
    api.getWorkboard.mockResolvedValue(board({ blocked: [PAUSED] }));
    const onCards = vi.fn();
    render(
      <Workboard
        onWork={() => {}}
        onCards={onCards}
        openCard={{ id: "p1", n: 1 }}
      />,
    );
    await screen.findByRole("listitem", { name: "Port the lexer" });
    expect(
      inCard("Port the lexer").getByRole("list", { name: "Steps" }),
    ).toBeVisible();
    // onCards fires from an effect after the board renders.
    await waitFor(() =>
      expect(onCards).toHaveBeenCalledWith({ "chat-p1": "p1" }),
    );
  });

  it("reloads when the cabin saves the board", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.getWorkboard
      .mockResolvedValueOnce(board({ todo: [card()] }))
      .mockResolvedValueOnce(board({ doing: [card()] }, [], 2));
    render(<Workboard onWork={() => {}} />);
    await screen.findByRole("region", { name: "Todo" });
    expect(
      within(column("Todo")).getByText("Write release notes"),
    ).toBeVisible();
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STAMP_EVERY_MS);
    });
    expect(api.getWorkboard).toHaveBeenCalledTimes(1);
    api.workboardStamp.mockResolvedValue(2);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(STAMP_EVERY_MS);
    });
    await waitFor(() =>
      expect(
        within(column("Doing")).getByText("Write release notes"),
      ).toBeVisible(),
    );
    expect(api.getWorkboard).toHaveBeenCalledTimes(2);
  });
  it("Agent: do this queues a card, and the board shows it waiting for the cabin", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    const queued = board({ todo: [card({ queued: "Queued" })] });
    api.workboardAct.mockResolvedValue({
      ...queued,
      queueNote: "Queued cards start in the cabin.",
    });
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );
    const handOver = within(
      screen.getByRole("group", { name: "Agent: do this" }),
    );
    await userEvent.click(handOver.getByRole("button", { name: "Queue" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "queue" });
    expect(
      await screen.findByText("Queued cards start in the cabin."),
    ).toBeVisible();
    expect(
      inCard("Write release notes").getAllByText("Queued")[0],
    ).toBeVisible();
    await userEvent.click(
      screen.getByRole("button", { name: "Take off the queue" }),
    );
    expect(api.workboardAct).toHaveBeenLastCalledWith("w-1", {
      kind: "unqueue",
    });
  });

  it("Now is Work: the card's prompt goes to the chat box", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.workboardWork.mockResolvedValue({
      prompt: "Work on this workboard card",
      page: MOVED,
    });
    const onWork = vi.fn();
    render(<Workboard onWork={onWork} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Now" }));
    expect(api.workboardWork).toHaveBeenCalledWith("w-1");
    expect(onWork).toHaveBeenCalledWith("Work on this workboard card");
  });

  it("blocks with a reason, sets the priority and saves your steps", async () => {
    const steps = card({
      steps: [{ text: "Read the spec", status: "done" }],
      stepsText: "[x] Read the spec",
    });
    api.getWorkboard.mockResolvedValue(board({ todo: [steps] }));
    api.workboardAct.mockResolvedValue(board({ todo: [steps] }));
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );

    await userEvent.click(screen.getByRole("button", { name: "Block…" }));
    const block = within(screen.getByRole("form", { name: "Block" }));
    expect(block.getByRole("button", { name: "Block" })).toBeDisabled();
    await userEvent.type(
      block.getByRole("textbox", { name: "What is it waiting on?" }),
      "Waiting on review",
    );
    await userEvent.click(block.getByRole("button", { name: "Block" }));
    expect(api.workboardAct).toHaveBeenLastCalledWith("w-1", {
      kind: "block",
      reason: "Waiting on review",
    });

    await userEvent.selectOptions(
      screen.getByRole("combobox", { name: "Priority" }),
      "high",
    );
    expect(api.workboardAct).toHaveBeenLastCalledWith("w-1", {
      kind: "priority",
      priority: "high",
    });

    await userEvent.click(screen.getByRole("button", { name: "Edit steps" }));
    const box = screen.getByRole("textbox", { name: "Steps" });
    expect(box).toHaveValue("[x] Read the spec");
    await userEvent.type(box, "{enter}Write the parser");
    await userEvent.click(screen.getByRole("button", { name: "Save steps" }));
    expect(api.workboardAct).toHaveBeenLastCalledWith("w-1", {
      kind: "steps",
      text: "[x] Read the spec\nWrite the parser",
    });
  });

  it("Unblock shows on a blocked card", async () => {
    const blocked = card({
      checkpoint: { goal: "", next: "", blocker: "Review", refs: [] },
    });
    api.getWorkboard.mockResolvedValue(board({ blocked: [blocked] }));
    api.workboardAct.mockResolvedValue(board({ todo: [card()] }));
    render(<Workboard onWork={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );
    expect(screen.queryByRole("group", { name: "Agent: do this" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Unblock" }));
    expect(api.workboardAct).toHaveBeenCalledWith("w-1", { kind: "unblock" });
  });

  it("a drop on a card puts the dragged card above it", async () => {
    const other = card({ id: "w-3", title: "Ship it" });
    api.getWorkboard.mockResolvedValue(board({ todo: [card(), other] }));
    api.workboardAct.mockResolvedValue(board({ todo: [other, card()] }));
    render(<Workboard onWork={() => {}} />);
    const ship = await screen.findByRole("listitem", { name: "Ship it" });
    fireEvent.dragStart(ship);
    const target = screen.getByRole("listitem", {
      name: "Write release notes",
    });
    fireEvent.dragOver(target);
    fireEvent.drop(target);
    expect(api.workboardAct).toHaveBeenCalledTimes(1);
    expect(api.workboardAct).toHaveBeenCalledWith("w-3", {
      kind: "place",
      to: "todo",
      before: "w-1",
    });
  });

  it("starts a playbook on the card and makes it recurring", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    api.getPlaybooks.mockResolvedValue({
      rows: [
        {
          id: "linux_repair",
          name: "Linux repair",
          summary: "",
          source: "shipped",
          fits: "",
          steps: "",
          text: "",
        },
      ],
      problems: [],
    });
    api.workboardPlaybook.mockResolvedValue({
      prompt: "Work on this workboard card",
      page: TODO,
    });
    api.workboardRecur.mockResolvedValue(TODO);
    const onWork = vi.fn();
    render(<Workboard onWork={onWork} onResume={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Start a flow" }));
    const start = within(screen.getByRole("group", { name: "Start a flow" }));
    expect(start.getByRole("button", { name: "Start" })).toBeDisabled();
    await userEvent.selectOptions(
      await start.findByRole("combobox", { name: "Playbook" }),
      "linux_repair",
    );
    await userEvent.click(start.getByRole("button", { name: "Start" }));
    expect(api.workboardPlaybook).toHaveBeenCalledWith(
      "w-1",
      "linux_repair",
      "Linux repair",
    );
    expect(onWork).toHaveBeenCalledWith("Work on this workboard card");

    const recur = within(
      screen.getByRole("group", { name: "Make it recurring" }),
    );
    await userEvent.selectOptions(
      recur.getByRole("combobox", { name: "Schedule" }),
      "1",
    );
    await userEvent.click(
      recur.getByRole("button", { name: "Make it recurring" }),
    );
    expect(api.workboardRecur).toHaveBeenCalledWith("w-1", 1);
  });

  it("leaves Start a flow out of the web app", async () => {
    api.getWorkboard.mockResolvedValue(TODO);
    render(
      <Workboard onWork={() => {}} onResume={() => {}} resumable={false} />,
    );
    await userEvent.click(
      await screen.findByRole("button", { name: /Write release notes/ }),
    );
    expect(screen.queryByRole("group", { name: "Start a flow" })).toBeNull();
    expect(
      screen.getByRole("group", { name: "Make it recurring" }),
    ).toBeVisible();
  });
});
