import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { EpisodePanel } from "./episodeApi";

const api = vi.hoisted(() => ({
  handler: null as ((panel: EpisodePanel) => void) | null,
  state: vi.fn<(id: string) => Promise<EpisodePanel | null>>(),
  // The command answers whether the window should send the resume now.
  answer: vi.fn<(id: string, parkId: string, approve: boolean) => Promise<boolean>>(),
  pause: vi.fn<(id: string) => Promise<boolean>>(),
  stop: vi.fn<(id: string) => Promise<boolean>>(),
  halt: vi.fn<() => Promise<string[]>>(),
  arm: vi.fn<(id: string, on: boolean) => Promise<boolean>>(),
  armed: vi.fn<(id: string) => Promise<boolean>>(),
}));

vi.mock("./episodeApi", () => ({
  onEpisode: (h: (panel: EpisodePanel) => void) => {
    api.handler = h;
    return Promise.resolve(() => {
      api.handler = null;
    });
  },
  episodeState: api.state,
  episodeAnswer: api.answer,
  episodePause: api.pause,
  episodeStop: api.stop,
  episodeArm: api.arm,
  episodeArmed: api.armed,
}));
vi.mock("./api", () => ({ haltDesktop: api.halt }));

import { ARM_LABEL, Episode, endedLine, RESUME_TEXT, stepWords, WAITING_NOTE } from "./Episode";

function panel(over: Partial<EpisodePanel> = {}): EpisodePanel {
  return {
    conversationId: "c1",
    episodeId: "ep-1",
    header: "Desktop session · 2 steps · 1 min",
    steps: [
      { n: 1, tool: "click", decision: "allow", result: "click ok", goalStep: "Open Settings" },
      { n: 2, tool: "click", decision: "deny", result: "denied", goalStep: "Open Settings" },
    ],
    parks: [],
    frame: "data:image/png;base64,AAAA",
    state: "running",
    ended: null,
    cua: false,
    ...over,
  };
}

async function open(p: EpisodePanel | null = null, desktopOn = true) {
  api.state.mockResolvedValue(p);
  render(<Episode conversationId="c1" desktopOn={desktopOn} />);
  await waitFor(() => expect(api.handler).not.toBeNull());
}

beforeEach(() => {
  api.handler = null;
  api.state.mockReset();
  api.answer.mockReset().mockResolvedValue(false);
  api.pause.mockReset().mockResolvedValue(true);
  api.stop.mockReset().mockResolvedValue(true);
  api.halt.mockReset().mockResolvedValue(["c1"]);
  api.arm.mockReset().mockImplementation((_id, on) => Promise.resolve(on));
  api.armed.mockReset().mockResolvedValue(false);
});

describe("Episode", () => {
  it("offers no switch while desktop control is off", async () => {
    await open(null, false);
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("the switch arms the next messages as a session and turning it off disarms them", async () => {
    await open();
    const box = screen.getByRole("checkbox", { name: new RegExp(ARM_LABEL) });
    expect(box).not.toBeChecked();
    await userEvent.click(box);
    expect(api.arm).toHaveBeenCalledWith("c1", true);
    await waitFor(() => expect(box).toBeChecked());
    await userEvent.click(box);
    expect(api.arm).toHaveBeenLastCalledWith("c1", false);
    await waitFor(() => expect(box).not.toBeChecked());
  });

  it("an armed chat shows its switch on when it opens", async () => {
    api.armed.mockResolvedValue(true);
    await open();
    await waitFor(() => expect(screen.getByRole("checkbox")).toBeChecked());
  });

  it("the switch gives way to the panel once the session opens", async () => {
    await open();
    act(() => api.handler?.(panel()));
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("shows nothing until an episode opens, then the header, the steps and the screen", async () => {
    await open();
    expect(screen.queryByLabelText("Desktop session")).toBeNull();
    act(() => api.handler?.(panel()));
    expect(screen.getByText("Desktop session · 2 steps · 1 min")).toBeInTheDocument();
    const steps = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(steps).toEqual(["1click" + "ran: click ok", "2click" + "not run: denied"]);
    expect(screen.getByRole("img", { name: "The latest screen" })).toHaveAttribute(
      "src",
      "data:image/png;base64,AAAA",
    );
  });

  it("reopens an episode that was already running when the chat opened", async () => {
    await open(panel());
    expect(await screen.findByText("Desktop session · 2 steps · 1 min")).toBeInTheDocument();
  });

  it("ignores another chat's episode", async () => {
    await open();
    act(() => api.handler?.(panel({ conversationId: "c2" })));
    expect(screen.queryByLabelText("Desktop session")).toBeNull();
  });

  it("draws no picture when the shell sent none", async () => {
    await open();
    act(() => api.handler?.(panel({ frame: null })));
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("a waiting step shows its card, says it waits even in Always, and Allow and Deny answer it", async () => {
    await open();
    const parks = [{ id: "epk-ep-1-3", tool: "click", action: "Click Send", class: "send" }];
    act(() => api.handler?.(panel({ parks })));
    const card = screen.getByRole("group", { name: "Waiting: Click Send" });
    expect(card).toHaveTextContent(WAITING_NOTE);
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(api.answer).toHaveBeenCalledWith("c1", "epk-ep-1-3", true);
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(api.answer).toHaveBeenLastCalledWith("c1", "epk-ep-1-3", false);
  });

  it("Pause, Stop and Halt call their commands", async () => {
    await open();
    act(() => api.handler?.(panel()));
    await userEvent.click(screen.getByRole("button", { name: "Pause" }));
    expect(api.pause).toHaveBeenCalledWith("c1");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.stop).toHaveBeenCalledWith("c1");
    await userEvent.click(screen.getByRole("button", { name: "Halt" }));
    expect(api.halt).toHaveBeenCalledTimes(1);
  });

  it("a paused episode says so and offers no second Pause", async () => {
    await open();
    act(() => api.handler?.(panel({ state: "paused" })));
    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pause" })).toBeNull();
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
  });

  it("an ended episode names how it ended and Dismiss clears it", async () => {
    await open();
    act(() => api.handler?.(panel({ state: "ended", ended: "halt" })));
    expect(screen.getByRole("status")).toHaveTextContent("Halted. Nothing more ran.");
    expect(screen.queryByRole("button", { name: "Halt" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByLabelText("Desktop session")).toBeNull();
  });

  it("names the Cua driver when steps run through it", async () => {
    await open();
    act(() => api.handler?.(panel({ cua: true })));
    expect(screen.getByText("Using the Cua driver")).toBeInTheDocument();
  });

  it("a refused command shows its reason", async () => {
    await open();
    api.answer.mockRejectedValue("That step already has an answer.");
    act(() => api.handler?.(panel({ parks: [{ id: "epk-ep-1-3", tool: "click", action: "Click Send", class: "send" }] })));
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("That step already has an answer.");
  });

  it("a parked step reads waiting, and not run once the session is over", async () => {
    await open();
    const parked = { n: 3, tool: "type", decision: "park", result: "parked: waiting for your approval", goalStep: "" };
    act(() => api.handler?.(panel({ steps: [parked] })));
    expect(screen.getByRole("listitem")).toHaveTextContent("3typewaiting for your OK");
    act(() => api.handler?.(panel({ steps: [parked], state: "ended", ended: "stop" })));
    expect(screen.getByRole("listitem")).toHaveTextContent("3typenot run");
  });

  it("Allow with no reply running sends the resume through the chat, once", async () => {
    const onResume = vi.fn<(text: string) => void>();
    api.state.mockResolvedValue(null);
    render(<Episode conversationId="c1" desktopOn onResume={onResume} />);
    await waitFor(() => expect(api.handler).not.toBeNull());
    const parks = [{ id: "epk-ep-1-3", tool: "click", action: "Loadout wants to click Send in Mail", class: "send" }];
    act(() => api.handler?.(panel({ parks })));
    expect(screen.getByRole("group", { name: "Waiting: Loadout wants to click Send in Mail" })).toBeInTheDocument();
    // The shell says a resume is due: nothing runs to pick the answer up.
    api.answer.mockResolvedValue(true);
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    await waitFor(() => expect(onResume).toHaveBeenCalledTimes(1));
    expect(onResume).toHaveBeenCalledWith("Continue");
    expect(RESUME_TEXT).toBe("Continue");
  });

  it("an Allow a running reply picks up, and a Deny, send nothing", async () => {
    const onResume = vi.fn<(text: string) => void>();
    api.state.mockResolvedValue(null);
    render(<Episode conversationId="c1" desktopOn onResume={onResume} />);
    await waitFor(() => expect(api.handler).not.toBeNull());
    act(() => api.handler?.(panel({ parks: [{ id: "epk-ep-1-3", tool: "click", action: "Click Send", class: "send" }] })));
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(api.answer).toHaveBeenCalledTimes(2));
    expect(api.answer.mock.calls).toEqual([
      ["c1", "epk-ep-1-3", true],
      ["c1", "epk-ep-1-3", false],
    ]);
    expect(onResume).not.toHaveBeenCalled();
  });

  it("an allowed step reads allowed until it runs, then what it did", async () => {
    await open();
    const row = (decision: string, result: string) => ({ n: 3, tool: "click", decision, result, goalStep: "" });
    act(() => api.handler?.(panel({ steps: [row("allowed", "")] })));
    expect(screen.getByRole("listitem")).toHaveTextContent("3clickallowed, not run yet");
    act(() => api.handler?.(panel({ steps: [row("approve", "click ok")] })));
    expect(screen.getByRole("listitem")).toHaveTextContent("3clickran: click ok");
    act(() => api.handler?.(panel({ steps: [row("allowed", "")], state: "ended", ended: "halt" })));
    expect(screen.getByRole("listitem")).toHaveTextContent("3clicknot run");
  });

  it("keeps the picture it has when the shell says it is the same, and drops it when there is none", async () => {
    await open();
    act(() => api.handler?.(panel({ frame: "data:image/png;base64,ONE" })));
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,ONE");
    act(() => api.handler?.({ ...panel({ frame: null }), frameSame: true } as EpisodePanel));
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,ONE");
    act(() => api.handler?.(panel({ frame: "data:image/png;base64,TWO" })));
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,TWO");
    act(() => api.handler?.(panel({ frame: null })));
    expect(screen.queryByRole("img")).toBeNull();
  });

  it("a panel that opens on a same-picture event takes the picture from the first read", async () => {
    let read: (p: EpisodePanel) => void = () => {};
    api.state.mockReturnValue(new Promise<EpisodePanel | null>((resolve) => (read = resolve)));
    render(<Episode conversationId="c1" desktopOn />);
    await waitFor(() => expect(api.handler).not.toBeNull());
    act(() => api.handler?.({ ...panel({ frame: null }), frameSame: true } as EpisodePanel));
    expect(screen.queryByRole("img")).toBeNull();
    await act(async () => read(panel({ frame: "data:image/png;base64,ONE" })));
    expect(screen.getByRole("img")).toHaveAttribute("src", "data:image/png;base64,ONE");
  });

  it("words for the steps and the endings", () => {
    const step = (decision: string, result: string) => ({ n: 1, tool: "click", decision, result, goalStep: "" });
    expect([
      stepWords(step("allow", ""), false),
      stepWords(step("allow", "ok"), false),
      stepWords(step("park", "x"), false),
      stepWords(step("park", "x"), true),
      stepWords(step("deny", "denied"), false),
      stepWords(step("deny", ""), true),
    ]).toEqual(["ran", "ran: ok", "waiting for your OK", "not run", "not run: denied", "not run"]);
    expect([
      stepWords(step("allowed", ""), false),
      stepWords(step("allowed", ""), true),
      stepWords(step("approve", "click ok"), false),
      stepWords(step("approve", ""), true),
    ]).toEqual(["allowed, not run yet", "not run", "ran: click ok", "ran"]);
    expect(endedLine("verified")).toBe("Done, and checked.");
    expect(endedLine("unconfirmed")).toBe("Ended: the check couldn't confirm it.");
    expect(endedLine("whatever")).toBe("Ended.");
  });
});
