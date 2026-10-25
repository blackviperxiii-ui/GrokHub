import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AutomationRow, RunResult } from "./api";

const api = vi.hoisted(() => ({
  listAutomations: vi.fn<() => Promise<AutomationRow[]>>(),
  addAutomation: vi.fn<(text: string) => Promise<AutomationRow[]>>(),
  setAutomationEnabled: vi.fn<(id: string, enabled: boolean) => Promise<AutomationRow[]>>(),
  removeAutomation: vi.fn<(id: string) => Promise<AutomationRow[]>>(),
  runAutomation: vi.fn<(id: string) => Promise<RunResult>>(),
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
    ...over,
  };
}

describe("Automations", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
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
});

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
