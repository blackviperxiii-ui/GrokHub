import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunResult } from "./api";
import type { RoutineRow } from "./routinesApi";

const api = vi.hoisted(() => ({
  listRoutines: vi.fn<() => Promise<RoutineRow[]>>(),
  setRoutineOn: vi.fn<(name: string, on: boolean) => Promise<RoutineRow[]>>(),
  allowRoutine: vi.fn<(name: string, hash: string) => Promise<RoutineRow[]>>(),
  revokeRoutine: vi.fn<(name: string) => Promise<RoutineRow[]>>(),
  undoRoutine: vi.fn<(name: string) => Promise<string>>(),
  runRoutineNow: vi.fn<(name: string) => Promise<RunResult>>(),
  applyRoutineFix: vi.fn<(name: string) => Promise<string>>(),
  dismissRoutineFix: vi.fn<(name: string) => Promise<RoutineRow[]>>(),
}));
vi.mock("./routinesApi", () => api);

import { Routines, ROUTINES_EMPTY, consentLine, runsLine } from "./Routines";

function row(over: Partial<RoutineRow> = {}): RoutineRow {
  return {
    name: "greet",
    about: "Says hi",
    triggers: ["weekdays 07:30", "chat"],
    steps: 2,
    changes: ["say: run_terminal_command"],
    changeDetails: ['say: run_terminal_command {"command":"echo {{inputs.who}}"}'],
    hash: "4f2a",
    consent: "not_allowed",
    on: true,
    runs: 0,
    modelCalls: 0,
    lastAtMs: null,
    lastOk: null,
    lastLine: null,
    problems: [],
    fix: null,
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  api.listRoutines.mockResolvedValue([row()]);
});

describe("Routines", () => {
  it("shows what starts a routine, its consent and that it hasn't run", async () => {
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    expect(card.getByText("Says hi")).toBeInTheDocument();
    expect(card.getByText("weekdays 07:30 · chat · 2 steps")).toBeInTheDocument();
    expect(
      card.getByText("Changes things (say: run_terminal_command). Those steps won't run until you allow them."),
    ).toBeInTheDocument();
    expect(card.getByText("Not run yet.")).toBeInTheDocument();
    expect(card.getByRole("switch")).toHaveAttribute("aria-checked", "true");
  });

  it("allows changes only after a click and a confirm", async () => {
    api.allowRoutine.mockResolvedValue([row({ consent: "allowed" })]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    // Enter on the focused button opens nothing.
    card.getByRole("button", { name: "Allow changes…" }).focus();
    await user.keyboard("{Enter}");
    expect(card.queryByRole("group", { name: "Allow greet to change things?" })).toBeNull();

    await user.click(card.getByRole("button", { name: "Allow changes…" }));
    // Each step shows with its arguments before the person allows it.
    expect(card.getByText('say: run_terminal_command {"command":"echo {{inputs.who}}"}')).toBeInTheDocument();
    const confirm = within(card.getByRole("group", { name: "Allow greet to change things?" }));
    await user.click(confirm.getByRole("button", { name: "Allow" }));
    // The routine as listed goes back, so an edit since the list isn't allowed.
    expect(api.allowRoutine).toHaveBeenCalledWith("greet", "4f2a");
    expect(await card.findByText("Allowed to change things: say: run_terminal_command.")).toBeInTheDocument();
    expect(card.getByRole("button", { name: "Take back" })).toBeInTheDocument();
  });

  it("loads the list again when Allow finds the routine changed", async () => {
    api.allowRoutine.mockRejectedValue("greet changed since its steps were shown. Look over them again, then allow it.");
    api.listRoutines
      .mockResolvedValueOnce([row()])
      .mockResolvedValueOnce([row({ changeDetails: ['say: run_terminal_command {"command":"rm -r notes"}'], hash: "9c1e" })]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    await user.click(card.getByRole("button", { name: "Allow changes…" }));
    await user.click(
      within(card.getByRole("group", { name: "Allow greet to change things?" })).getByRole("button", { name: "Allow" }),
    );
    expect(await screen.findByText("greet changed since its steps were shown. Look over them again, then allow it.")).toBeInTheDocument();
    // The next Allow shows the new steps and sends the new hash.
    await user.click(card.getByRole("button", { name: "Allow changes…" }));
    expect(card.getByText('say: run_terminal_command {"command":"rm -r notes"}')).toBeInTheDocument();
    await user.click(
      within(card.getByRole("group", { name: "Allow greet to change things?" })).getByRole("button", { name: "Allow" }),
    );
    expect(api.allowRoutine).toHaveBeenLastCalledWith("greet", "9c1e");
  });

  it("switches a routine off", async () => {
    api.setRoutineOn.mockResolvedValue([row({ on: false })]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    await user.click(card.getByRole("switch"));
    expect(api.setRoutineOn).toHaveBeenCalledWith("greet", false);
    expect(card.getByRole("switch")).toHaveAttribute("aria-checked", "false");
  });

  it("runs now and shows the report, then the new counts", async () => {
    api.runRoutineNow.mockResolvedValue({ text: "say: hi\n\nhi", error: "", stop: "routine" });
    api.listRoutines
      .mockResolvedValueOnce([row()])
      .mockResolvedValueOnce([row({ runs: 1, lastAtMs: Date.now(), lastOk: true, lastLine: "hi" })]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    await user.click(card.getByRole("button", { name: "Run now" }));
    expect(api.runRoutineNow).toHaveBeenCalledWith("greet");
    expect(await card.findByText("1 run, 0 model calls. Last just now, finished.")).toBeInTheDocument();
    expect(card.getByText(/say: hi/)).toBeInTheDocument();
  });

  it("undoes the newest change after a confirm", async () => {
    api.undoRoutine.mockResolvedValue("Removed the routine greet. Its text is kept in the change history.");
    api.listRoutines.mockResolvedValueOnce([row()]).mockResolvedValueOnce([]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    await user.click(card.getByRole("button", { name: "Undo…" }));
    await user.click(
      within(card.getByRole("group", { name: "Undo the newest change to greet?" })).getByRole("button", {
        name: "Undo",
      }),
    );
    expect(api.undoRoutine).toHaveBeenCalledWith("greet");
    expect(
      await screen.findByText("Removed the routine greet. Its text is kept in the change history."),
    ).toBeInTheDocument();
    expect(screen.getByText(ROUTINES_EMPTY)).toBeInTheDocument();
  });

  it("lists a broken routine with its problem and no Run now", async () => {
    api.listRoutines.mockResolvedValue([
      row({ name: "broken", consent: null, triggers: [], steps: 0, changes: [], changeDetails: [], hash: null, problems: ["step x: no tool named no_such_tool"] }),
    ]);
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "broken" }));
    expect(card.getByRole("alert")).toHaveTextContent("Can't run: step x: no tool named no_such_tool");
    expect(card.queryByRole("button", { name: "Run now" })).toBeNull();
  });

  it("stays away when the list isn't served", async () => {
    api.listRoutines.mockRejectedValue(new Error("not served"));
    const { container } = render(<Routines />);
    await Promise.resolve();
    await Promise.resolve();
    expect(container).toBeEmptyDOMElement();
  });
});

describe("suggested fixes", () => {
  const fix = { step: "read", before: '{"target_file":"notes.txt"}', after: '{"target_file":"in/notes.txt"}', seen: 3 };

  it("shows the fix and applies it after a click and a confirm", async () => {
    api.listRoutines.mockResolvedValueOnce([row({ fix })]).mockResolvedValueOnce([row()]);
    api.applyRoutineFix.mockResolvedValue("Fixed step read in greet.");
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    const block = within(card.getByRole("group", { name: "Suggested fix for greet" }));
    expect(
      block.getByText("The same repair to step read worked in 3 runs. Make it part of the routine?"),
    ).toBeInTheDocument();
    expect(block.getByText(/Fixed: \{"target_file":"in\/notes.txt"\}/)).toBeInTheDocument();
    // Enter on the focused button opens nothing.
    card.getByRole("button", { name: "Apply fix…" }).focus();
    await user.keyboard("{Enter}");
    expect(card.queryByRole("group", { name: "Fix step read in greet?" })).toBeNull();
    await user.click(card.getByRole("button", { name: "Apply fix…" }));
    await user.click(
      within(card.getByRole("group", { name: "Fix step read in greet?" })).getByRole("button", { name: "Fix" }),
    );
    expect(api.applyRoutineFix).toHaveBeenCalledWith("greet");
    expect(await screen.findByText("Fixed step read in greet.")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "Suggested fix for greet" })).toBeNull();
  });

  it("dismisses a fix", async () => {
    api.listRoutines.mockResolvedValue([row({ fix })]);
    api.dismissRoutineFix.mockResolvedValue([row()]);
    const user = userEvent.setup();
    render(<Routines />);
    const card = within(await screen.findByRole("listitem", { name: "greet" }));
    await user.click(card.getByRole("button", { name: "Dismiss fix" }));
    expect(api.dismissRoutineFix).toHaveBeenCalledWith("greet");
    expect(card.queryByRole("button", { name: "Apply fix…" })).toBeNull();
  });
});

describe("lines", () => {
  it("reads consent and runs", () => {
    expect(consentLine(row({ consent: "read_only", changes: [] }))).toBe("Only reads, so it runs on its own.");
    expect(consentLine(row({ consent: "edited" }))).toBe(
      "Edited since you allowed it. Its changes (say: run_terminal_command) won't run until you allow them again.",
    );
    expect(consentLine(row({ consent: null }))).toBeNull();
    const now = 10 * 3_600_000;
    expect(runsLine(row({ runs: 3, modelCalls: 1, lastAtMs: now - 2 * 3_600_000, lastOk: false }), now)).toBe(
      "3 runs, 1 model call. Last 2h ago, stopped.",
    );
  });
});
