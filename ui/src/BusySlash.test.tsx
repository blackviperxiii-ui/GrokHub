import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatEnvelope, ChatEvent, SessionMode, Settings, SlashEffect, SlashHit } from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  runSlash: vi.fn<(line: string, id?: string) => Promise<SlashEffect>>(),
  slashHits: vi.fn<(draft: string) => Promise<SlashHit[]>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  readTurn: vi.fn((text: string) => Promise.resolve({ route: "queue", text, ack: "Queued for after this reply" })),
}));

vi.mock("./autoSetupApi", () => ({ settingAsk: () => Promise.resolve(null) }));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
  ...chatTabApiStubs(),
  quickChips: () => Promise.resolve([]),
  chipOutcome: () => Promise.resolve(true),
  sendChat: api.sendChat,
  getSettings: api.getSettings,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  cancelChat: api.cancelChat,
  readTurn: api.readTurn,
  closeChat: vi.fn(),
  answerElicit: vi.fn(),
  saveSettings: vi.fn(),
  answerPermission: vi.fn(),
  getStartChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
};
});

import { Chat, SLASH_RUNNING, SLASH_WAITS, SLASH_WAITS_REMOTE } from "./Chat";

const SETTINGS: Settings = {
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/user",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
};

function emit(conversationId: string, event: ChatEvent) {
  act(() => api.handler?.({ conversationId, event }));
}

/** Each sendChat waits until its `end(stop)` is called. */
function turns() {
  const ends: ((stop: string) => void)[] = [];
  api.sendChat.mockReset().mockImplementation(() => new Promise<string>((resolve) => ends.push(resolve)));
  return ends;
}

/** The slash menu offers one row, `cmd`, which runs when picked. */
function menuOffers(cmd: string) {
  api.slashHits.mockImplementation((draft) =>
    Promise.resolve(
      draft.startsWith("/") && cmd.startsWith(draft.trim())
        ? [{ cmd, hint: "", insert: `${cmd} `, runOnPick: true, disabled: null }]
        : [],
    ),
  );
}

/** Each runSlash waits until the returned `answer(effect)` is called. */
function slowSlash() {
  const answers: ((effect: SlashEffect) => void)[] = [];
  api.runSlash.mockReset().mockImplementation(() => new Promise<SlashEffect>((resolve) => answers.push(resolve)));
  return answers;
}

/** A chat whose first reply is running. The chat's id. */
async function replying(): Promise<string> {
  await userEvent.type(screen.getByLabelText("Message"), "write a long essay{Enter}");
  await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
  return api.sendChat.mock.calls[0][0];
}

describe("slash commands typed while a reply runs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    api.getSettings.mockResolvedValue(SETTINGS);
    api.cancelChat.mockResolvedValue();
    api.slashHits.mockReset().mockResolvedValue([]);
  });

  it("/queue holds its message until the reply ends, then sends it", async () => {
    const ends = turns();
    const onQueue = vi.fn();
    api.runSlash.mockResolvedValue({ kind: "queue", text: "then run the tests" });
    render(<Chat onOpenSettings={() => {}} onQueue={onQueue} />);
    const id = await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/queue then run the tests{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledTimes(1));
    expect(api.runSlash).toHaveBeenCalledWith("/queue then run the tests", id);
    await waitFor(() => expect(onQueue).toHaveBeenLastCalledWith(id, ["then run the tests"]));
    expect(box).toHaveValue("");
    expect(screen.queryByRole("alert")).toBeNull();
    // Nothing went to the model, and the reply was left alone.
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(api.readTurn).not.toHaveBeenCalled();
    expect(api.cancelChat).not.toHaveBeenCalled();
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("then run the tests");
  });

  it.each([
    ["/bg index the repo", "/bg"],
    ["/bg stop", "/bg"],
    ["/background index the repo", "/background"],
    ["/send count the files", "/send"],
    ["/consult tabs or spaces", "/consult"],
  ])("%s runs beside the reply and posts its line", async (line, command) => {
    turns();
    api.runSlash.mockResolvedValue({ kind: "notice", text: "It ran" });
    render(<Chat onOpenSettings={() => {}} />);
    const id = await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, `${line}{Enter}`);
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledTimes(1));
    expect(api.runSlash).toHaveBeenCalledWith(line, id);
    expect(await screen.findByRole("status", { name: `${command} result` })).toHaveTextContent("It ran");
    expect(box).toHaveValue("");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(api.readTurn).not.toHaveBeenCalled();
    expect(api.cancelChat).not.toHaveBeenCalled();
  });

  it("a command's line goes above the reply that is streaming, which stays one bubble", async () => {
    turns();
    api.runSlash.mockResolvedValue({ kind: "notice", text: "Background · index the repo" });
    const { container } = render(<Chat onOpenSettings={() => {}} />);
    const id = await replying();
    emit(id, { kind: "text", text: "Once upon" });
    await userEvent.type(screen.getByLabelText("Message"), "/bg index the repo{Enter}");
    const line = await screen.findByRole("status", { name: "/bg result" });
    emit(id, { kind: "text", text: " a time" });
    const replies = container.querySelectorAll('[data-role="assistant"]');
    expect(replies).toHaveLength(1);
    expect(replies[0]).toHaveTextContent("Once upon a time");
    expect(line.compareDocumentPosition(replies[0]) & Node.DOCUMENT_POSITION_FOLLOWING).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
  });

  it("a command that needs the reply to end says so and stays in the box", async () => {
    turns();
    render(<Chat onOpenSettings={() => {}} />);
    await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/new{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("That command waits for the reply. /stop ends it.");
    expect(SLASH_WAITS).toBe("That command waits for the reply. /stop ends it.");
    expect(box).toHaveValue("/new");
    expect(api.runSlash).not.toHaveBeenCalled();
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("in a paired browser /queue still waits its turn, and a held command is not told to /stop", async () => {
    const ends = turns();
    api.runSlash.mockResolvedValue({ kind: "queue", text: "then run the tests" });
    render(<Chat onOpenSettings={() => {}} remote />);
    const id = await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/stop{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("That command waits for the reply.");
    expect(SLASH_WAITS_REMOTE).toBe("That command waits for the reply.");
    expect(screen.getByRole("alert")).not.toHaveTextContent("/stop");
    expect(api.runSlash).not.toHaveBeenCalled();
    await userEvent.clear(box);
    await userEvent.type(box, "/queue then run the tests{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledWith("/queue then run the tests", id));
    await waitFor(() => expect(box).toHaveValue(""));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("then run the tests");
  });

  it.each([
    ["on the computer", false],
    ["in a paired browser", true],
  ])("/queue answered just after the reply ended is sent, not lost (%s)", async (_where, remote) => {
    const ends = turns();
    const answers = slowSlash();
    render(<Chat onOpenSettings={() => {}} remote={remote} />);
    await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/queue then run the tests{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledTimes(1));
    // The reply ends while run_slash is on its way back.
    await act(async () => ends[0]("end_turn"));
    await act(async () => answers[0]({ kind: "queue", text: "then run the tests" }));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("then run the tests");
    expect(api.sendChat.mock.calls[1][2]).toBe("chat");
    expect(box).toHaveValue("");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it.each([
    ["/ne", "/new", false, "That command waits for the reply. /stop ends it."],
    ["/ret", "/retry", false, "That command waits for the reply. /stop ends it."],
    ["/comp", "/compact", false, "That command waits for the reply. /stop ends it."],
    ["/ne", "/new", true, "That command waits for the reply."],
  ])("%s + Enter on the menu's %s row waits for the reply and says so (paired browser: %s)", async (typed, cmd, remote, reason) => {
    turns();
    menuOffers(cmd);
    api.runSlash.mockResolvedValue({ kind: "notice", text: "It ran" });
    render(<Chat onOpenSettings={() => {}} remote={remote} />);
    await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, typed);
    await screen.findByRole("listbox", { name: "Commands" });
    await userEvent.type(box, "{Enter}");
    expect((await screen.findByRole("alert")).textContent).toBe(reason);
    expect(box).toHaveValue(typed);
    expect(api.runSlash).not.toHaveBeenCalled();
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a row that leaves the reply alone still runs when picked from the menu", async () => {
    turns();
    menuOffers("/bg");
    api.runSlash.mockResolvedValue({ kind: "notice", text: "No background runs" });
    render(<Chat onOpenSettings={() => {}} />);
    const id = await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/b");
    await screen.findByRole("listbox", { name: "Commands" });
    await userEvent.type(box, "{Enter}");
    expect(await screen.findByRole("status", { name: "/bg result" })).toHaveTextContent("No background runs");
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    expect(api.runSlash).toHaveBeenCalledWith("/bg", id);
    expect(box).toHaveValue("");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a command still running is not started twice, and its answer leaves what was typed meanwhile", async () => {
    turns();
    const answers = slowSlash();
    render(<Chat onOpenSettings={() => {}} />);
    await replying();
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "/consult tabs or spaces{Enter}");
    await waitFor(() => expect(api.runSlash).toHaveBeenCalledTimes(1));
    expect(box).toHaveValue("/consult tabs or spaces");
    // Enter again while the first is still out.
    await userEvent.type(box, "{Enter}");
    expect(api.runSlash).toHaveBeenCalledTimes(1);
    expect((await screen.findByText("That command is still running.")).getAttribute("role")).toBe("status");
    expect(SLASH_RUNNING).toBe("That command is still running.");
    await userEvent.clear(box);
    await userEvent.type(box, "half a thought");
    await act(async () => answers[0]({ kind: "notice", text: "Tabs" }));
    expect(await screen.findByRole("status", { name: "/consult result" })).toHaveTextContent("Tabs");
    expect(box).toHaveValue("half a thought");
    expect(screen.queryByText("That command is still running.")).toBeNull();
  });
});
