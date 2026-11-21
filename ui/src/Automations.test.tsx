import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import type { AutomationRow, Blueprint, RunResult, ScheduledRun, SuggestedTile } from "./api";

const ranHandlers = vi.hoisted(() => [] as ((run: ScheduledRun) => void)[]);

const DISK: Blueprint = {
  id: "disk-watch",
  title: "Disk watch",
  body: "Check free space on a path; wake the model only when output changes.",
  seed: "every day at {time} run `df -h {path}` only when it changes and summarize what filled up",
  slots: [
    { key: "path", label: "Path", placeholder: "/" },
    { key: "time", label: "Time", placeholder: "09:00" },
  ],
};

const api = vi.hoisted(() => ({
  listAutomations: vi.fn<() => Promise<AutomationRow[]>>(),
  addAutomation: vi.fn<(text: string) => Promise<AutomationRow[]>>(),
  setAutomationEnabled: vi.fn<(id: string, enabled: boolean) => Promise<AutomationRow[]>>(),
  removeAutomation: vi.fn<(id: string) => Promise<AutomationRow[]>>(),
  runAutomation: vi.fn<(id: string) => Promise<RunResult>>(),
  editAutomation: vi.fn<(id: string, text: string) => Promise<AutomationRow[]>>(),
  listSuggestedAutomations: vi.fn<() => Promise<SuggestedTile[]>>(),
  listAutomationBlueprints: vi.fn<() => Promise<Blueprint[]>>(),
  addSuggestedAutomation: vi.fn<(seed: string) => Promise<AutomationRow[]>>(),
  getAutomationsService: vi.fn<() => Promise<boolean>>(),
  setAutomationsService: vi.fn<(on: boolean) => Promise<boolean>>(),
  fillBlueprintSeed: vi.fn((seed: string, values: Record<string, string>) => {
    let out = seed;
    for (const [key, raw] of Object.entries(values)) {
      const value = raw.trim();
      if (!value) throw new Error(`Fill in ${key} first.`);
      out = out.split(`{${key}}`).join(value);
    }
    if (out.includes("{") && out.includes("}")) throw new Error("Fill in every field first.");
    return out;
  }),
  onAutomationRan: vi.fn((handler: (run: ScheduledRun) => void) => {
    ranHandlers.push(handler);
    return Promise.resolve(() => {
      ranHandlers.splice(ranHandlers.indexOf(handler), 1);
    });
  }),
}));

vi.mock("./api", () => api);

import { Automations, AUTOMATIONS_EMPTY, checkLine, KEEP_RUNNING } from "./Automations";

function row(over: Partial<AutomationRow>): AutomationRow {
  return {
    id: "auto-1",
    name: "summarize my inbox",
    instructions: "every weekday at 9am summarize my inbox",
    enabled: true,
    summary: "weekdays at 09:00 · next today 9:00 AM",
    health: null,
    describe: "every weekday at 9am summarize my inbox",
    checkCommand: "",
    gate: { onChange: false, noAgent: false },
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
      api.addSuggestedAutomation,
      api.getAutomationsService,
      api.setAutomationsService,
    ])
      fn.mockReset();
    api.listSuggestedAutomations.mockReset().mockResolvedValue([]);
    api.listAutomationBlueprints.mockReset().mockResolvedValue([]);
    api.getAutomationsService.mockRejectedValue("unknown command");
    ranHandlers.length = 0;
  });

  it("checkLine names the command and gate", () => {
    expect(checkLine(row({}))).toBeNull();
    expect(
      checkLine(row({ checkCommand: "df -h", gate: { onChange: true, noAgent: false } })),
    ).toBe("checks `df -h` first, only when it changes");
    expect(
      checkLine(row({ checkCommand: "uptime", gate: { onChange: false, noAgent: true } })),
    ).toBe("checks `uptime` first, without AI");
  });

  it("shows a check line on each gated row", async () => {
    api.listAutomations.mockResolvedValue([
      row({
        checkCommand: "df -h",
        gate: { onChange: true, noAgent: false },
      }),
      row({
        id: "auto-2",
        name: "disk script",
        checkCommand: "df -h /",
        gate: { onChange: false, noAgent: true },
      }),
    ]);
    render(<Automations />);
    const inbox = await screen.findByRole("listitem", { name: "summarize my inbox" });
    expect(inbox).toHaveTextContent("checks `df -h` first, only when it changes");
    expect(screen.getByRole("listitem", { name: "disk script" })).toHaveTextContent(
      "checks `df -h /` first, without AI",
    );
  });

  it("toggles Keep running when the app is closed", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.getAutomationsService.mockResolvedValue(false);
    api.setAutomationsService.mockResolvedValue(true);
    render(<Automations />);
    const switchEl = await screen.findByRole("switch", { name: KEEP_RUNNING });
    expect(switchEl).toHaveAttribute("aria-checked", "false");
    await userEvent.click(switchEl);
    expect(api.setAutomationsService).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("switch", { name: KEEP_RUNNING })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("hides Keep running when the service command is missing", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.getAutomationsService.mockRejectedValue("unknown command");
    render(<Automations />);
    expect(await screen.findByText(AUTOMATIONS_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("switch", { name: KEEP_RUNNING })).toBeNull();
  });

  it("renders a Blueprint and saves the filled seed", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.listAutomationBlueprints.mockResolvedValue([DISK]);
    api.addAutomation.mockResolvedValue([
      row({
        id: "auto-disk",
        name: "run `df -h /home/user` only when it changes and summarize what filled up",
        summary: "daily at 08:00 · next today 8:00 AM",
      }),
    ]);
    render(<Automations />);
    const gallery = within(await screen.findByRole("region", { name: "Blueprints" }));
    expect(
      gallery.getByText(
        "Starters with a few fields to fill — script-first checks that fail closed before a model call.",
      ),
    ).toBeInTheDocument();
    const tile = within(gallery.getByRole("listitem", { name: "Disk watch" }));
    await userEvent.click(tile.getByRole("button", { name: "Use" }));
    const path = tile.getByRole("textbox", { name: "Disk watch Path" });
    const time = tile.getByRole("textbox", { name: "Disk watch Time" });
    await userEvent.clear(path);
    await userEvent.type(path, "/home/user");
    await userEvent.clear(time);
    await userEvent.type(time, "08:00");
    await userEvent.click(tile.getByRole("button", { name: "Save" }));
    expect(api.fillBlueprintSeed).toHaveBeenCalledWith(DISK.seed, {
      path: "/home/user",
      time: "08:00",
    });
    expect(api.addAutomation).toHaveBeenCalledWith(
      "every day at 08:00 run `df -h /home/user` only when it changes and summarize what filled up",
    );
    expect(
      await screen.findByRole("listitem", { name: /summarize what filled up/ }),
    ).toBeInTheDocument();
  });

  it("shows no Blueprints section when the catalog cannot load", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.listAutomationBlueprints.mockRejectedValue("unknown command");
    render(<Automations />);
    expect(
      await screen.findByText(
        "No automations yet. Describe one above in plain words and it runs on its own schedule.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Blueprints" })).toBeNull();
  });

  it("adds a Suggested tile and drops it from the list", async () => {
    const midday = { title: "Midday board", body: "Every 12h — summarize the workboard.", seed: "/loop 12h summarize the workboard" };
    const brief = { title: "Morning brief", body: "Every 1d — workboard and last host receipt.", seed: "/loop 1d summarize the workboard and last host receipt" };
    api.listAutomations.mockResolvedValue([]);
    api.listSuggestedAutomations.mockResolvedValueOnce([midday, brief]).mockResolvedValueOnce([brief]);
    api.addSuggestedAutomation.mockResolvedValue([
      row({ id: "auto-2", name: "summarize the workboard", summary: "every 12h · next 8:00 AM" }),
    ]);
    render(<Automations />);
    const suggested = within(await screen.findByRole("region", { name: "Suggested" }));
    expect(suggested.getByText("Suggested from your recent work")).toBeInTheDocument();
    const tile = within(suggested.getByRole("listitem", { name: "Midday board" }));
    expect(tile.getByText("Every 12h — summarize the workboard.")).toBeInTheDocument();
    await userEvent.click(tile.getByRole("button", { name: "Add" }));
    expect(api.addSuggestedAutomation).toHaveBeenCalledWith("/loop 12h summarize the workboard");
    expect(await screen.findByRole("listitem", { name: "summarize the workboard" })).toBeInTheDocument();
    expect(screen.queryByRole("listitem", { name: "Midday board" })).toBeNull();
    expect(screen.getByRole("listitem", { name: "Morning brief" })).toBeInTheDocument();
    expect(screen.queryByText("No automations yet. Describe one above in plain words and it runs on its own schedule.")).toBeNull();
  });

  it("brings a tile back when its job is removed", async () => {
    const midday = { title: "Midday board", body: "Every 12h — summarize the workboard.", seed: "/loop 12h summarize the workboard" };
    api.listAutomations.mockResolvedValue([row({ id: "auto-2", name: "summarize the workboard" })]);
    api.listSuggestedAutomations.mockResolvedValueOnce([]).mockResolvedValueOnce([midday]);
    api.removeAutomation.mockResolvedValue([]);
    render(<Automations />);
    const job = within(await screen.findByRole("listitem", { name: "summarize the workboard" }));
    expect(screen.queryByRole("region", { name: "Suggested" })).toBeNull();
    await userEvent.click(job.getByRole("button", { name: "Remove…" }));
    await userEvent.click(screen.getByRole("button", { name: "Remove" }));
    expect(api.removeAutomation).toHaveBeenCalledWith("auto-2");
    expect(await screen.findByRole("listitem", { name: "Midday board" })).toBeInTheDocument();
    expect(api.listSuggestedAutomations).toHaveBeenCalledTimes(2);
  });

  it("shows no Suggested section when there are no tiles or they can't load", async () => {
    api.listAutomations.mockResolvedValue([]);
    api.listSuggestedAutomations.mockRejectedValue("unknown command");
    render(<Automations />);
    expect(await screen.findByText("No automations yet. Describe one above in plain words and it runs on its own schedule.")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Suggested" })).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
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
    api.listSuggestedAutomations.mockReset().mockResolvedValue([]);
    api.listAutomationBlueprints.mockReset().mockResolvedValue([]);
    api.getAutomationsService.mockReset().mockRejectedValue("unknown command");
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

  it("hides Edit when words can't say the schedule", async () => {
    api.listAutomations.mockResolvedValue([
      row({ id: "auto-weekly", name: "weekly review", describe: null, summary: "weekly at 09:00" }),
      row({}),
    ]);
    render(<Automations />);
    const weekly = await screen.findByRole("listitem", { name: "weekly review" });
    expect(within(weekly).queryByRole("button", { name: "Edit" })).toBeNull();
    expect(screen.queryByText("Runs in the cabin")).toBeNull();
    expect(screen.getByText(/Loadout runs these on schedule while it is open or in the tray\./)).toBeInTheDocument();
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
