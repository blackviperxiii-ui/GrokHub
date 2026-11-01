import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import type { AutomationRow, RunResult, ScheduledRun } from "./api";

const ranHandlers = vi.hoisted(() => [] as ((run: ScheduledRun) => void)[]);

const api = vi.hoisted(() => ({
  listAutomations: vi.fn<() => Promise<AutomationRow[]>>(),
  addAutomation: vi.fn<(text: string) => Promise<AutomationRow[]>>(),
  setAutomationEnabled: vi.fn<(id: string, enabled: boolean) => Promise<AutomationRow[]>>(),
  removeAutomation: vi.fn<(id: string) => Promise<AutomationRow[]>>(),
  runAutomation: vi.fn<(id: string) => Promise<RunResult>>(),
  editAutomation: vi.fn<(id: string, text: string) => Promise<AutomationRow[]>>(),
  onAutomationRan: vi.fn((handler: (run: ScheduledRun) => void) => {
    ranHandlers.push(handler);
    return Promise.resolve(() => {
      ranHandlers.splice(ranHandlers.indexOf(handler), 1);
    });
  }),
}));

vi.mock("./api", () => api);

import { Automations } from "./Automations";

function row(over: Partial<AutomationRow>): AutomationRow {
  return {
    id: "auto-1",
    name: "summarize my inbox",
    instructions: "every weekday at 9am summarize my inbox",
    enabled: true,
    summary: "weekdays at 09:00 · next today 9:00 AM",
    health: null,
    describe: "every weekday at 9am summarize my inbox",
    cabinOnly: false,
    ...over,
  };
}

describe("Automations", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockClear();
    for (const fn of [
      api.listAutomations,
      api.addAutomation,
      api.setAutomationEnabled,
      api.removeAutomation,
      api.runAutomation,
      api.editAutomation,
    ])
      fn.mockReset();
    ranHandlers.length = 0;
  });

  it("lists jobs with their schedule and health", async () => {
    api.listAutomations.mockResolvedValue([
      row({}),
      row({
        id: "auto-2",
        name: "board",
        summary: "daily at 07:00 · paused",
        enabled: false,
        health: "Last run failed — API key rejected",
      }),
    ]);
    render(<Automations />);

    const inbox = await screen.findByRole("listitem", { name: "summarize my inbox" });
    expect(inbox).toHaveTextContent("weekdays at 09:00 · next today 9:00 AM");
    expect(within(inbox).getByRole("button", { name: "Pause" })).toBeInTheDocument();
    const board = screen.getByRole("listitem", { name: "board" });
    expect(board).toHaveTextContent("Last run failed — API key rejected");
    expect(within(board).getByRole("button", { name: "Resume" })).toBeInTheDocument();
  });

  it("adds a job from plain text and shows the parser's hint on a miss", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.addAutomation
      .mockRejectedValueOnce('Say when, like "every weekday at 9am summarize my inbox".')
      .mockResolvedValueOnce([row({})]);
    render(<Automations />);
    expect(await screen.findByText("No automations yet. Describe one above in plain words and it runs on its own schedule.")).toBeInTheDocument();

    await userEvent.type(screen.getByLabelText("New automation"), "summarize my inbox{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Say when");
    expect(screen.getByLabelText("New automation")).toHaveValue("summarize my inbox");

    await userEvent.clear(screen.getByLabelText("New automation"));
    await userEvent.type(
      screen.getByLabelText("New automation"),
      " every weekday at 9am summarize my inbox {Enter}",
    );
    expect(api.addAutomation).toHaveBeenLastCalledWith("every weekday at 9am summarize my inbox");
    expect(await screen.findByRole("listitem", { name: "summarize my inbox" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByLabelText("New automation")).toHaveValue("");
  });

  it("pause and remove send the row's id", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    api.setAutomationEnabled.mockResolvedValue([row({ enabled: false, summary: "paused" })]);
    api.removeAutomation.mockResolvedValue([]);
    render(<Automations />);

    await userEvent.click(await screen.findByRole("button", { name: "Pause" }));
    expect(api.setAutomationEnabled).toHaveBeenCalledWith("auto-1", false);
    expect(await screen.findByRole("button", { name: "Resume" })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Remove…" }));
    expect(api.removeAutomation).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Keep" }));
    expect(api.removeAutomation).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Remove…" }));
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(api.removeAutomation).toHaveBeenCalledWith("auto-1");
    expect(await screen.findByText("No automations yet. Describe one above in plain words and it runs on its own schedule.")).toBeInTheDocument();
  });

  it("Run now shows the reply and keeps Running… while the tab is hidden", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    let finish: (r: RunResult) => void = () => {};
    api.runAutomation.mockImplementation(() => new Promise((resolve) => (finish = resolve)));
    const view = render(<Automations />);

    await userEvent.click(await screen.findByRole("button", { name: "Run now" }));
    expect(api.runAutomation).toHaveBeenCalledWith("auto-1");
    expect(screen.getByRole("button", { name: "Running…" })).toBeDisabled();
    view.rerender(<Automations active={false} />);
    view.rerender(<Automations active />);
    expect(screen.getByRole("button", { name: "Running…" })).toBeDisabled();

    api.listAutomations.mockResolvedValue([row({ summary: "weekdays at 09:00 · 1 run" })]);
    finish({ text: "3 new mails.", error: "", stop: "end_turn" });

    expect(await screen.findByText("3 new mails.")).toBeInTheDocument();
    expect(await screen.findByText("weekdays at 09:00 · 1 run")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Run now" })).toBeEnabled();
  });

  it("reloads the list each time the tab is shown, not while hidden", async () => {
    api.listAutomations.mockResolvedValueOnce([]).mockResolvedValueOnce([row({})]);
    const view = render(<Automations active />);
    expect(await screen.findByText("No automations yet. Describe one above in plain words and it runs on its own schedule.")).toBeInTheDocument();
    view.rerender(<Automations active={false} />);
    expect(api.listAutomations).toHaveBeenCalledTimes(1);
    view.rerender(<Automations active />);
    expect(await screen.findByRole("listitem", { name: "summarize my inbox" })).toBeInTheDocument();
    expect(api.listAutomations).toHaveBeenCalledTimes(2);
  });

  it("a failed run shows its error", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    api.runAutomation.mockResolvedValue({
      text: "",
      error: "Add an xAI API key in Settings first.",
      stop: "error",
    });
    render(<Automations />);
    await userEvent.click(await screen.findByRole("button", { name: "Run now" }));
    expect(await screen.findByText("Add an xAI API key in Settings first.")).toBeInTheDocument();
  });

  it("tells chat when a run finishes or fails", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    api.runAutomation
      .mockResolvedValueOnce({ text: "3 new mails, none urgent.", error: "", stop: "end_turn" })
      .mockRejectedValueOnce("The chat worker stopped.");
    const onFinished = vi.fn();
    const onStarted = vi.fn();
    render(<Automations onStarted={onStarted} onFinished={onFinished} />);
    await userEvent.click(await screen.findByRole("button", { name: "Run now" }));
    await screen.findByText("3 new mails, none urgent.");
    await userEvent.click(await screen.findByRole("button", { name: "Run now" }));
    await screen.findByText("The chat worker stopped.");
    expect(onFinished.mock.calls).toEqual([
      [{ id: "auto-1", name: "summarize my inbox", ok: true, text: "3 new mails, none urgent." }],
      [{ id: "auto-1", name: "summarize my inbox", ok: false, text: "The chat worker stopped." }],
    ]);
    expect(onStarted.mock.calls).toEqual([
      ["auto-1", "summarize my inbox"],
      ["auto-1", "summarize my inbox"],
    ]);
  });
});

describe("Automations edit and schedule", () => {
  beforeEach(() => {
    for (const fn of [api.listAutomations, api.editAutomation]) fn.mockReset();
    ranHandlers.length = 0;
  });

  it("Edit opens the row's words, saves the new ones, and keeps the box on a miss", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    api.editAutomation
      .mockRejectedValueOnce('Say when, like "every weekday at 9am summarize my inbox".')
      .mockResolvedValueOnce([
        row({
          name: "summarize my mail",
          instructions: "every day at 7am summarize my mail",
          summary: "daily at 07:00 · next Wed 7:00 AM",
          describe: "every day at 7am summarize my mail",
        }),
      ]);
    render(<Automations />);

    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    const box = screen.getByLabelText("Edit summarize my inbox");
    expect(box).toHaveValue("every weekday at 9am summarize my inbox");
    expect(screen.getByRole("button", { name: "Edit" })).toBeDisabled();

    await userEvent.clear(box);
    await userEvent.type(box, "summarize my mail{Enter}");
    expect(api.editAutomation).toHaveBeenLastCalledWith("auto-1", "summarize my mail");
    expect(await screen.findByRole("alert")).toHaveTextContent("Say when");
    expect(screen.getByLabelText("Edit summarize my inbox")).toHaveValue("summarize my mail");

    await userEvent.clear(box);
    await userEvent.type(box, " every day at 7am summarize my mail {Enter}");
    expect(api.editAutomation).toHaveBeenLastCalledWith("auto-1", "every day at 7am summarize my mail");
    const edited = await screen.findByRole("listitem", { name: "summarize my mail" });
    expect(edited).toHaveTextContent("daily at 07:00 · next Wed 7:00 AM");
    expect(screen.queryByRole("textbox", { name: /^Edit / })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("Cancel closes the box without saving", async () => {
    api.listAutomations.mockResolvedValue([row({})]);
    render(<Automations />);
    await userEvent.click(await screen.findByRole("button", { name: "Edit" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByLabelText("Edit summarize my inbox")).toBeNull();
    expect(api.editAutomation).not.toHaveBeenCalled();
  });

  it("marks cabin-only jobs and hides Edit when words can't say the schedule", async () => {
    api.listAutomations.mockResolvedValue([
      row({ id: "auto-gated", name: "tidy", cabinOnly: true }),
      row({ id: "auto-weekly", name: "weekly review", describe: null, summary: "weekly at 09:00" }),
      row({}),
    ]);
    render(<Automations />);
    const gated = await screen.findByRole("listitem", { name: "tidy" });
    expect(within(gated).getByText("Runs in the cabin")).toBeInTheDocument();
    const weekly = screen.getByRole("listitem", { name: "weekly review" });
    expect(within(weekly).queryByRole("button", { name: "Edit" })).toBeNull();
    const inbox = screen.getByRole("listitem", { name: "summarize my inbox" });
    expect(within(inbox).queryByText("Runs in the cabin")).toBeNull();
    expect(screen.getByText(/While the Loadout cabin is open, the cabin runs them\./)).toBeInTheDocument();
  });

  it("a scheduled run shows its reply and reloads the row while the tab shows", async () => {
    api.listAutomations
      .mockResolvedValueOnce([row({})])
      .mockResolvedValueOnce([row({ summary: "weekdays at 09:00 · next Wed 9:00 AM · 1 run" })]);
    render(<Automations />);
    await screen.findByRole("listitem", { name: "summarize my inbox" });
    await waitForHandler();
    act(() => {
      ranHandlers[0]({
        id: "auto-1",
        name: "summarize my inbox",
        result: { text: "3 new mails.", error: "", stop: "end_turn" },
      });
    });
    expect(await screen.findByText("3 new mails.")).toBeInTheDocument();
    expect(await screen.findByText("weekdays at 09:00 · next Wed 9:00 AM · 1 run")).toBeInTheDocument();
    expect(api.listAutomations).toHaveBeenCalledTimes(2);
  });
});

async function waitForHandler() {
  for (let i = 0; i < 20 && ranHandlers.length === 0; i++) await Promise.resolve();
  expect(ranHandlers.length).toBe(1);
}

describe("wallClock", () => {
  it("reads the local weekday, hour and minute", async () => {
    const real = await vi.importActual<typeof import("./api")>("./api");
    expect(real.wallClock(new Date(2026, 9, 27, 8, 5))).toEqual({
      weekday: 2,
      hour: 8,
      minute: 5,
    });
  });
});
