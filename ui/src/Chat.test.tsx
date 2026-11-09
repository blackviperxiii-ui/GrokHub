import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DELETE_ASK_QUESTION, DELETE_MESSAGE_LABEL, DELETE_REPLY_QUESTION, MESSAGE_MENU_LABEL } from "./DeleteMessage";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Answer,
  Approvals,
  ChatEnvelope,
  ChatEvent,
  ElicitAction,
  QuickChip,
  SessionMode,
  Settings,
  Transcript,
} from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  gap: null as (() => void) | null,
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  answerElicit:
    vi.fn<
      (id: string, callId: string, action: ElicitAction, field?: string | null, value?: string) => Promise<void>
    >(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  saveSettings:
    vi.fn<
      (tray: boolean, ws: string, approvals: Approvals, desk: boolean) => Promise<Settings>
    >(),
  answerPermission: vi.fn<(id: string, callId: string, answer: Answer) => Promise<void>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  steerChat: vi.fn<(id: string, text: string) => Promise<boolean>>(),
  readTurn: vi.fn((text: string) => Promise.resolve({ route: "queue", text, ack: "Queued for after this reply" })),
  sideChat:
    vi.fn<(id: string, sideId: string, question: string, ask: string, soFar: string) => Promise<string>>(),
  tellMain: vi.fn<(id: string, question: string, answer: string) => Promise<{ text: string; steered: boolean }>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
  deleteChat: vi.fn<(id: string) => Promise<void>>(),
  forgetMessage:
    vi.fn<(id: string, cut: { reply: boolean; text: string | null; later: number }) => Promise<boolean>>(),
  getLastChat: vi.fn<() => Promise<Transcript | null>>(),
  loadChat: vi.fn<(id: string, limit?: number) => Promise<Transcript>>(),
  slashHits: vi.fn(() => Promise.resolve([])),
  runSlash: vi.fn(),
  exportChat: vi.fn(),
  openSaved: vi.fn(),
  revealSaved: vi.fn(),
  quickChips: vi.fn<(surface: string, id?: string, draft?: string, mode?: SessionMode) => Promise<QuickChip[]>>(),
  chipOutcome: vi.fn<(surface: string, id: string, picked: boolean) => Promise<boolean>>(),
  goalAct: vi.fn(() => Promise.resolve({ board: { today: [], mine: [], suggested: [], season: { label: "", done: 0, trophies: [], badges: [] } } })),
}));

// A chat line that asks for a setting is answered by auto setup.
const auto = vi.hoisted(() => ({
  settingAsk: vi.fn<(text: string) => Promise<string | null>>(() => Promise.resolve(null)),
}));
vi.mock("./autoSetupApi", () => auto);

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  answerElicit: api.answerElicit,
  getSettings: api.getSettings,
  saveSettings: api.saveSettings,
  answerPermission: api.answerPermission,
  cancelChat: api.cancelChat,
  steerChat: api.steerChat,
  readTurn: api.readTurn,
  sideChat: api.sideChat,
  tellMain: api.tellMain,
  closeChat: api.closeChat,
  deleteChat: api.deleteChat,
  forgetMessage: api.forgetMessage,
  getLastChat: api.getLastChat,
  loadChat: api.loadChat,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  exportChat: api.exportChat,
  openSaved: api.openSaved,
  revealSaved: api.revealSaved,
  quickChips: api.quickChips,
  chipOutcome: api.chipOutcome,
  goalAct: api.goalAct,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
  helperQuestions: () => Promise.resolve([]),
  onEventGap: (handler: () => void) => {
    api.gap = handler;
    return Promise.resolve(() => {});
  },
  // Push to talk has its own tests (Mic.test.tsx).
  transcribe: vi.fn(),
  // Attachments have their own tests (AttachChips.test.tsx).
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
}));

import {
  applyEvent,
  Chat,
  CHAT_EMPTY,
  EARLIER_STEP,
  EVENTS_LOST,
  HALTED_LINE,
  itemsFrom,
  MISSION_BUSY,
  OPEN_BUSY,
  PLAN_NEXT,
  STOP_ASKING_DONE,
  type Item,
  type QueueAct,
} from "./Chat";
import { ASK_TRACKED } from "./Goals";

const SAVED: Transcript = {
  id: "shell-saved",
  title: "paint colours",
  lines: [
    { role: "user", text: "pick a paint colour" },
    { role: "tools", text: "read_file, web_search" },
    { role: "assistant", text: "Try **blue**." },
  ],
  earlier: 0,
};

const SETTINGS: Settings = {
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/me",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
};

function chip(id: string, label: string, value: string, over: Partial<QuickChip> = {}): QuickChip {
  return {
    id,
    label,
    value,
    action: "ask",
    hint: id.startsWith("reply-") ? "Suggested in the reply" : "",
    primary: false,
    ...over,
  };
}

/** Holds sendChat open until `finish` is called, like a turn in flight. */
function holdTurn() {
  let finish: (stop?: string) => void = () => {};
  api.sendChat.mockImplementation(
    () => new Promise<string>((resolve) => (finish = (stop = "end_turn") => resolve(stop))),
  );
  return () => finish();
}

async function startTurn(text: string) {
  await userEvent.type(screen.getByLabelText("Message"), `${text}{Enter}`);
  await waitFor(() => expect(api.sendChat).toHaveBeenCalled());
  return api.sendChat.mock.calls[api.sendChat.mock.calls.length - 1][0];
}

function emit(conversationId: string, event: ChatEvent) {
  act(() => api.handler?.({ conversationId, event }));
}

describe("Chat", () => {
  beforeEach(() => {
    for (const fn of [
      api.sendChat,
      api.answerPermission,
      api.cancelChat,
      api.closeChat,
      api.answerElicit,
      api.saveSettings,
      api.loadChat,
    ]) {
      fn.mockReset();
    }
    api.getLastChat.mockReset().mockResolvedValue(null);
    api.quickChips.mockReset().mockResolvedValue([]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.handler = null;
    api.gap = null;
  });

  it("says part of the reply may be missing after a gap in the web app's events", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    act(() => api.gap?.());
    expect(screen.queryByText(EVENTS_LOST)).toBeNull();
    await startTurn("hello");
    act(() => api.gap?.());
    expect(screen.getByText(EVENTS_LOST)).toBeVisible();
    finish();
  });

  it("puts a prefill in the box without sending it, and a new one replaces it", async () => {
    const { rerender } = render(
      <Chat onOpenSettings={() => {}} prefill={{ text: "close the done cards", n: 1 }} />,
    );
    const box = screen.getByRole("textbox", { name: "Message" });
    expect(box).toHaveValue("close the done cards");
    expect(api.sendChat).not.toHaveBeenCalled();

    rerender(<Chat onOpenSettings={() => {}} prefill={{ text: "check the build", n: 2 }} />);
    expect(box).toHaveValue("check the build");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("a mission prefill moves to its chat once, then stays there", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("hello");
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());

    const mission = { text: "Mission from my decision journal: Q", n: 1, conversationId: "shell-m1" };
    rerender(<Chat onOpenSettings={() => {}} prefill={mission} />);
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(screen.queryByText("hello")).toBeNull();
    expect(screen.getByLabelText("Message")).toHaveValue("Mission from my decision journal: Q");

    await userEvent.type(screen.getByLabelText("Message"), "{Enter}");
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith("shell-m1", "Mission from my decision journal: Q", undefined),
    );

    rerender(<Chat onOpenSettings={() => {}} prefill={{ ...mission, text: "again", n: 2 }} />);
    expect(api.closeChat).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Mission from my decision journal: Q")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("again");
  });

  it("a mission prefill waits for a running reply", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");
    rerender(
      <Chat onOpenSettings={() => {}} prefill={{ text: "Mission", n: 1, conversationId: "shell-m1" }} />,
    );
    expect(screen.getByText(MISSION_BUSY)).toBeVisible();
    expect(screen.getByLabelText("Message")).toHaveValue("");
    expect(api.closeChat).not.toHaveBeenCalled();
    expect(screen.getByText("hello")).toBeVisible();
    finish();
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith(id, "hello", undefined));
  });

  it("a goal the reply noticed is asked about under it, and Track puts it on My goals", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("fishing is so relaxing");
    emit(id, { kind: "text", text: "Nice, go for 99!" });
    emit(id, { kind: "goal", id: "g7", title: "Old School RuneScape · 99 fishing" });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    const ask = await screen.findByRole("group", { name: "Track this goal?" });
    expect(ask).toHaveTextContent("Old School RuneScape · 99 fishing");
    await userEvent.click(within(ask).getByRole("button", { name: "Track" }));
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "track", id: "g7" }]);
    expect(await screen.findByText(ASK_TRACKED)).toBeInTheDocument();
  });

  it("a reply's follow-ups join the row under the box, and a tap sends one in its reply's mode", async () => {
    let finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("the build failed");
    emit(id, { kind: "text", text: "TypeError in parse_row." });
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
    api.quickChips.mockResolvedValue([
      chip("reply-0", "Fix the cause", "Fix the root cause in parse_row.", { mode: "plan" }),
      chip("reply-1", "Add a test", "Add a test for parse_row."),
      chip("pred-1", "Explain it", "Explain the error."),
      chip("pred-2", "Fourth", "Never shown."),
    ]);
    emit(id, { kind: "suggestions", items: [] });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    const row = await screen.findByRole("group", { name: "Suggestions" });
    expect(api.quickChips).toHaveBeenLastCalledWith("composer", id, "", undefined);
    const taps = () =>
      within(row)
        .getAllByRole("button")
        .filter((b) => !b.getAttribute("aria-label")?.startsWith("Hide"));
    expect(taps().map((b) => b.textContent)).toEqual(["Fix the cause", "Add a test", "Explain it"]);
    expect(taps()[0]).toHaveAttribute("title", "Why this? Suggested in the reply  (Alt+1)");
    finish = holdTurn();
    await userEvent.click(within(row).getByRole("button", { name: "Fix the cause" }));
    expect(api.chipOutcome).toHaveBeenCalledWith("composer", "reply-0", true);
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith(id, "Fix the root cause in parse_row.", "plan"),
    );
    expect(screen.getByText("Fix the root cause in parse_row.")).toHaveAttribute("data-role", "user");
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
    finish();
  });

  it("a chip adds to what is typed, Go opens its page, and × hides one", async () => {
    const onOpenView = vi.fn();
    api.quickChips.mockResolvedValue([
      chip("pred-1", "Explain it", "Explain the error."),
      { ...chip("nav-1", "Automations", "__nav:night"), action: "go", view: "automations" },
    ]);
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    const row = await screen.findByRole("group", { name: "Suggestions" });
    await userEvent.type(screen.getByLabelText("Message"), "half typed");
    await userEvent.click(within(row).getByRole("button", { name: "Explain it" }));
    expect(screen.getByLabelText("Message")).toHaveValue("half typed Explain the error.");
    expect(api.sendChat).not.toHaveBeenCalled();

    await waitFor(() => expect(screen.getByRole("button", { name: "Automations" })).toBeVisible());
    await userEvent.click(screen.getByRole("button", { name: "Automations" }));
    expect(onOpenView).toHaveBeenCalledWith("automations");

    await userEvent.click(await screen.findByRole("button", { name: "Hide Explain it" }));
    expect(api.chipOutcome).toHaveBeenLastCalledWith("composer", "pred-1", false);
  });

  it("Alt+2 taps the second chip", async () => {
    const onOpenView = vi.fn();
    api.quickChips.mockResolvedValue([
      chip("pred-1", "Explain it", "Explain the error."),
      { ...chip("nav-1", "Automations", "__nav:night"), action: "go", view: "automations" },
    ]);
    render(<Chat onOpenSettings={() => {}} onOpenView={onOpenView} />);
    await screen.findByRole("group", { name: "Suggestions" });
    fireEvent.keyDown(window, { key: "™", code: "Digit2", altKey: true });
    expect(onOpenView).toHaveBeenCalledWith("automations");
    expect(api.chipOutcome).toHaveBeenCalledWith("composer", "nav-1", true);
  });

  it("a reply without follow-ups and an empty row show no chips", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("list the files");
    emit(id, { kind: "text", text: "There are 12 files." });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());
    expect(screen.getByText("There are 12 files.")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
  });

  it("/export saves this chat and leaves a card that opens or shows the file", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "export" });
    api.exportChat.mockReset().mockResolvedValue({
      token: "saved-1",
      name: "hello-20261106-1415.md",
      folder: "Chats",
      path: "/home/user/Documents/Loadout/Chats/hello-20261106-1415.md",
      line: "Saved hello-20261106-1415.md to Chats",
      revealLabel: "Show in folder",
    });
    api.revealSaved.mockReset().mockResolvedValue(undefined);
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");
    await act(async () => finish());
    await userEvent.type(screen.getByLabelText("Message"), "/export{Enter}");
    expect(api.runSlash).toHaveBeenCalledWith("/export");
    expect(api.exportChat).toHaveBeenCalledWith(id);
    const card = await screen.findByRole("status", { name: "Saved hello-20261106-1415.md to Chats" });
    await userEvent.click(within(card).getByRole("button", { name: "Show in folder" }));
    expect(api.revealSaved).toHaveBeenCalledWith("saved-1");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a failed /export says why and saves no card", async () => {
    api.runSlash.mockReset().mockResolvedValue({ kind: "export" });
    api.exportChat.mockReset().mockRejectedValue("There's nothing in this chat to save yet.");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/export{Enter}");
    expect(await screen.findByText("There's nothing in this chat to save yet.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show in folder" })).toBeNull();
  });

  it("sends the text and streams its own conversation's reply", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");

    expect(id).toMatch(/^shell-/);
    expect(api.sendChat).toHaveBeenCalledWith(id, "hello", undefined);
    expect(screen.getByText("hello")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("");

    emit("someone-else", { kind: "text", text: "nope" });
    emit(id, { kind: "text", text: "Hi from " });
    emit(id, { kind: "text", text: "Grok" });
    // The reply renders as Markdown, so its text sits in a paragraph inside the bubble.
    const bubble = () => screen.getByText("Hi from Grok").closest(".bubble");
    expect(bubble()).toHaveClass("streaming");
    expect(screen.queryByText(/nope/)).toBeNull();

    finish();
    await waitFor(() => expect(bubble()).not.toHaveClass("streaming"));
    expect(bubble()).toHaveAttribute("data-role", "assistant");
  });

  it("answers a setting ask here without sending it to the model", async () => {
    auto.settingAsk.mockResolvedValueOnce("Text size is 115% now.");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "make the text bigger{Enter}");
    expect(await screen.findByText("Text size is 115% now.")).toBeInTheDocument();
    expect(auto.settingAsk).toHaveBeenCalledWith("make the text bigger");
    expect(screen.getByText("make the text bigger")).toHaveAttribute("data-role", "user");
    expect(screen.getByLabelText("Message")).toHaveValue("");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("renders the reply as Markdown and keeps the person's message plain", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("**mine** <b>x</b>");
    emit(id, { kind: "text", text: "## Plan\n- **one**\n<script>alert(1)</script>" });
    finish();
    const reply = await screen.findByRole("heading", { level: 2, name: "Plan" });
    const bubble = reply.closest(".bubble")!;
    expect(bubble).toHaveAttribute("data-role", "assistant");
    expect(within(bubble as HTMLElement).getByRole("listitem")).toHaveTextContent("one");
    expect(bubble.querySelector("strong")).toHaveTextContent("one");
    expect(bubble.querySelector("script")).toBeNull();
    expect(within(bubble as HTMLElement).getByText("<script>alert(1)</script>")).toBeInTheDocument();
    const mine = screen.getByText("**mine** <b>x</b>");
    expect(mine).toHaveAttribute("data-role", "user");
    expect(mine.querySelector("strong, b")).toBeNull();
  });

  it("shows a permission card and Allow once answers it", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write a note");

    emit(id, {
      kind: "permission",
      id: "call-1",
      title: "write",
      action: "note.txt",
      reason: "",
    });
    const card = screen.getByRole("group", { name: "Approve write" });
    expect(card).toHaveTextContent("Loadout wants to run write");
    expect(card).toHaveTextContent("note.txt");

    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenCalledWith(id, "call-1", "allow");
    expect(card).toHaveTextContent("Allowed once");
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
  });

  it("an answer that doesn't reach the agent gives the buttons back", async () => {
    holdTurn();
    api.answerPermission.mockRejectedValueOnce("Can't reach Loadout on your computer. Is it open?");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write a note");

    emit(id, { kind: "permission", id: "call-9", title: "write", action: "note.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Can't reach Loadout on your computer.");
    const card = screen.getByRole("group", { name: "Approve write" });
    expect(card).not.toHaveTextContent("Allowed once");
    await userEvent.click(within(card).getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "call-9", "allow");
    expect(card).toHaveTextContent("Allowed once");
  });

  it("Deny and Always allow send their answers", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("go");

    emit(id, { kind: "permission", id: "a", title: "write", action: "a.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "a", "deny");
    expect(screen.getByText("Denied")).toBeInTheDocument();

    emit(id, {
      kind: "permission",
      id: "b",
      title: "run_terminal_command",
      action: "ls",
      reason: "",
    });
    await userEvent.click(screen.getByRole("button", { name: "Always allow" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "b", "always");
    expect(screen.getByText("Always allowed")).toBeInTheDocument();
  });

  it("a card nobody answered is retired when the turn ends", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write it");

    emit(id, { kind: "permission", id: "c", title: "write", action: "c.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    finish();

    expect(await screen.findByText("Not run: the turn ended")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("Halt retires every waiting card as halted and says so once", async () => {
    let finish: (stop: string) => void = () => {};
    api.sendChat.mockImplementation(() => new Promise<string>((resolve) => (finish = resolve)));
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("type it");

    emit(id, { kind: "permission", id: "d", title: "type", action: "hello", reason: "" });
    emit(id, {
      kind: "elicit",
      id: "ask-h",
      server: "Question",
      message: "Which window?",
      mode: "form",
      url: "",
      fieldName: "answer",
      fieldTitle: "Your answer",
      secret: false,
    });
    emit(id, { kind: "done", stop: "halted" });
    await act(async () => finish("halted"));

    expect(screen.getByText("Not run: halted")).toBeInTheDocument();
    expect(screen.getByText("Not answered: halted.")).toBeInTheDocument();
    expect(screen.getAllByText(HALTED_LINE)).toHaveLength(1);
    expect(screen.getByText(HALTED_LINE)).toHaveAttribute("role", "status");
    expect(screen.queryByRole("button", { name: "Allow once" })).toBeNull();
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("a turn that comes back halted settles its cards even if the done event was missed", async () => {
    let finish: (stop: string) => void = () => {};
    api.sendChat.mockImplementation(() => new Promise<string>((resolve) => (finish = resolve)));
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("type it");

    emit(id, { kind: "permission", id: "e", title: "type", action: "hi", reason: "" });
    await act(async () => finish("halted"));

    expect(screen.getByText("Not run: halted")).toBeInTheDocument();
    expect(screen.getByText(HALTED_LINE)).toBeInTheDocument();
    expect(screen.queryByText("Not run: the turn ended")).toBeNull();
  });

  it("a Deny from a notification shows on its card", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("write both");

    emit(id, { kind: "permission", id: "n1", title: "write", action: "n.txt", reason: "" });
    emit(id, { kind: "permission", id: "n2", title: "edit", action: "m.txt", reason: "" });
    emit(id, { kind: "answered", id: "n1", answer: "deny" });

    expect(screen.getByRole("group", { name: "Approve write" })).toHaveTextContent("Denied");
    const other = screen.getByRole("group", { name: "Approve edit" });
    expect(other).not.toHaveTextContent("Denied");
    expect(screen.getAllByRole("button", { name: "Allow once" })).toHaveLength(1);
    expect(api.answerPermission).not.toHaveBeenCalled();
  });

  it("an answer that comes after the turn ended leaves the card ended", () => {
    const ended: Item[] = [
      { kind: "permission", id: "a", title: "write", action: "", reason: "", answer: "ended" },
      { kind: "permission", id: "b", title: "write", action: "", reason: "", answer: "allow" },
    ];
    expect(applyEvent(ended, { kind: "answered", id: "a", answer: "deny" })).toEqual(ended);
    const halted: Item[] = [{ kind: "permission", id: "h", title: "type", action: "", reason: "", answer: "halted" }];
    expect(applyEvent(halted, { kind: "answered", id: "h", answer: "deny" })).toEqual(halted);
    expect(applyEvent(ended, { kind: "answered", id: "b", answer: "deny" })[1]).toMatchObject({
      answer: "deny",
    });
  });

  it("a reply that hasn't started shows a thinking orb, and a lookup tool a searching one", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("what's new");

    const orb = screen.getByRole("img", { name: "Thinking" });
    expect(orb.tagName).toBe("CANVAS");
    expect(orb.closest(".bubble")).toHaveAttribute("data-role", "assistant");
    expect(screen.queryByText("…")).toBeNull();

    emit(id, { kind: "tool", id: "t", title: "web_search", status: "pending", detail: "" });
    expect(screen.getByRole("img", { name: "Looking it up" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "Thinking" })).toBeNull();

    emit(id, { kind: "text", text: "Here" });
    expect(screen.queryByRole("img", { name: "Looking it up" })).toBeNull();
    finish();
  });

  it("Stop cancels the turn in flight", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("count");

    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledWith(id);

    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());
  });

  it("ignores an event that arrives after the turn ended", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hi");
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());

    emit(id, { kind: "text", text: "late" });
    emit(id, { kind: "permission", id: "x", title: "write", action: "x", reason: "" });

    expect(screen.queryByText("late")).toBeNull();
    expect(screen.queryByRole("group", { name: "Approve write" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows a helper's question after the turn ended, once, and sends its answer", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("look into it");
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeDisabled());

    const ask: ChatEvent = { kind: "permission", id: "h1", title: "write", action: "notes.md", reason: "" };
    act(() => api.handler?.({ conversationId: id, event: ask, helper: true }));
    act(() => api.handler?.({ conversationId: id, event: ask, helper: true }));
    act(() => api.handler?.({ conversationId: "another-chat", event: { ...ask, id: "h2" }, helper: true }));

    expect(screen.getAllByRole("group", { name: "Approve write" })).toHaveLength(1);
    await userEvent.click(screen.getByRole("button", { name: "Allow once" }));
    expect(api.answerPermission).toHaveBeenLastCalledWith(id, "h1", "allow");
  });

  it("a turn's end retires its own cards but leaves a helper's question open", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("go");

    emit(id, { kind: "permission", id: "own", title: "write", action: "a.txt", reason: "" });
    act(() =>
      api.handler?.({
        conversationId: id,
        event: { kind: "permission", id: "h", title: "delete", action: "b.txt", reason: "" },
        helper: true,
      }),
    );
    act(() => finish());

    const own = screen.getByRole("group", { name: "Approve write" });
    await waitFor(() => expect(own).toHaveTextContent("Not run: the turn ended"));
    const helper = screen.getByRole("group", { name: "Approve delete" });
    expect(within(helper).getByRole("button", { name: "Allow once" })).toBeInTheDocument();
  });

  it("shows the error and links to Settings", async () => {
    api.sendChat.mockRejectedValue("Add an xAI API key in Settings first.");
    const openSettings = vi.fn();
    render(<Chat onOpenSettings={openSettings} />);

    await userEvent.type(screen.getByLabelText("Message"), "hi{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Add an xAI API key in Settings first.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(openSettings).toHaveBeenCalledTimes(1);
  });

  it("New chat closes the old conversation and starts a fresh one", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("one");
    await waitFor(() => expect(screen.getByRole("button", { name: "New chat" })).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(screen.queryByText("one")).toBeNull();

    const second = await startTurn("two");
    expect(second).not.toBe(first);
  });

  it("Shift+Enter adds a line instead of sending", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "a{Shift>}{Enter}{/Shift}b");
    expect(screen.getByLabelText("Message")).toHaveValue("a\nb");
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("the conversation log takes keyboard focus, so a long one can be scrolled", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.tab();
    expect(screen.getByRole("log")).toHaveFocus();
  });
});

describe("applyEvent", () => {
  it("joins text, starts a new bubble after a tool, and updates a tool card in place", () => {
    let items: Item[] = [{ kind: "user", text: "go" }];
    items = applyEvent(items, { kind: "text", text: "Look" });
    items = applyEvent(items, { kind: "text", text: "ing." });
    items = applyEvent(items, {
      kind: "tool",
      id: "t",
      title: "read_file",
      status: "pending",
      detail: "",
    });
    items = applyEvent(items, {
      kind: "tool",
      id: "t",
      title: "read_file",
      status: "completed",
      detail: "a.txt",
    });
    items = applyEvent(items, { kind: "text", text: "Done." });
    expect(items).toEqual([
      { kind: "user", text: "go" },
      { kind: "assistant", text: "Looking.", streaming: true },
      { kind: "tool", id: "t", title: "read_file", status: "completed", detail: "a.txt" },
      { kind: "assistant", text: "Done.", streaming: true },
    ]);
  });

  it("the app picks how each message runs: there are no mode or approval chips", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    // The composer names the product, not the model.
    expect(screen.getByLabelText("Message")).toHaveAttribute("placeholder", "Message Loadout");
    for (const name of ["Chat", "Plan", "btw", "Ask", "Auto", "Always"]) {
      expect(screen.queryByRole("button", { name })).toBeNull();
    }
    const id = await startTurn("what does main.rs do");
    expect(api.sendChat).toHaveBeenCalledWith(id, "what does main.rs do", undefined);
    finish();
  });

  it("the composer shows no model or effort and has nothing to pick one with", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    expect(screen.queryByText(/effort/i)).toBeNull();
    expect(screen.queryByText(/grok-\d/i)).toBeNull();
    expect(screen.queryByText(/^model/i)).toBeNull();
    expect(screen.queryByLabelText(/model/i)).toBeNull();
    expect(screen.queryByRole("button", { name: /model|effort|grok/i })).toBeNull();
    expect(document.querySelector(".chips")).toBeNull();
  });

  it("Stop asking me allows the step and moves the setting to only ask for risky things", async () => {
    holdTurn();
    api.answerPermission.mockResolvedValue();
    api.saveSettings.mockResolvedValue({ ...SETTINGS, approvals: "auto" });
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("write a note");
    emit(id, { kind: "permission", id: "call-1", title: "write", action: "note.txt", reason: "" });
    await userEvent.click(screen.getByRole("button", { name: "Stop asking me" }));
    expect(api.answerPermission).toHaveBeenCalledWith(id, "call-1", "allow");
    expect(api.saveSettings).toHaveBeenCalledWith(true, "", "auto", false);
    expect(await screen.findByText(STOP_ASKING_DONE)).toBeVisible();
    emit(id, { kind: "permission", id: "call-2", title: "run", action: "make", reason: "" });
    // Only ask for risky things now: the next card has nothing looser to offer.
    expect(screen.queryByRole("button", { name: "Stop asking me" })).toBeNull();
  });

  it("on the web app the permission card has no Stop asking me", async () => {
    holdTurn();
    render(<Chat onOpenSettings={() => {}} remote />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    const id = await startTurn("write a note");
    emit(id, { kind: "permission", id: "call-1", title: "write", action: "note.txt", reason: "" });
    expect(screen.getByRole("button", { name: "Allow once" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "Stop asking me" })).toBeNull();
  });

  it("a plan card approves, and the next message is read afresh", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("tidy the build");
    emit(id, { kind: "plan", text: "1. Read build.rs" });
    emit(id, {
      kind: "elicit",
      id: "plan-1",
      server: "Plan",
      message: "1. Read build.rs",
      mode: "form",
      url: "",
      fieldName: null,
      fieldTitle: "",
      secret: false,
    });
    const card = screen.getByRole("group", { name: "The plan" });
    expect(card).toHaveTextContent("1. Read build.rs");
    expect(screen.getAllByText("1. Read build.rs")).toHaveLength(1);
    expect(screen.queryByText("…")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(api.answerElicit).toHaveBeenCalledWith(id, "plan-1", "accept", null, "");
    expect(card).toHaveTextContent("Approved. Loadout can make the changes now.");
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeVisible());
    holdTurn();
    await startTurn("thanks");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "thanks", undefined);
  });

  it("/plan makes only the next message plan first", async () => {
    api.runSlash.mockResolvedValue({ kind: "mode", mode: "plan", text: null });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "/plan{Enter}");
    expect(await screen.findByText(PLAN_NEXT)).toBeVisible();
    expect(api.sendChat).not.toHaveBeenCalled();
    let finish = holdTurn();
    const id = await startTurn("set up the release pipeline");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "set up the release pipeline", "plan");
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Send" })).toBeVisible());
    finish = holdTurn();
    await startTurn("and what about docs?");
    expect(api.sendChat).toHaveBeenLastCalledWith(id, "and what about docs?", undefined);
    finish();
  });

  it("a question takes a typed answer, and an unanswered one ends with the turn", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("pick a color");
    const ask = (n: number): ChatEvent => ({
      kind: "elicit",
      id: `ask-${n}`,
      server: "Question",
      message: `Which color? (${n})`,
      mode: "form",
      url: "",
      fieldName: "answer",
      fieldTitle: "Your answer",
      secret: false,
    });
    emit(id, ask(1));
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Your answer"), "blue{Enter}");
    expect(api.answerElicit).toHaveBeenCalledWith(id, "ask-1", "accept", "answer", "blue");
    expect(screen.getByRole("group", { name: "Loadout asks" })).toHaveTextContent("Sent.");
    emit(id, ask(2));
    finish();
    await waitFor(() =>
      expect(screen.getByText("Not answered: the turn ended.")).toBeInTheDocument(),
    );
  });

  it("signed out, it shows one sign-in button and a hint instead of the empty line", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    const onSignIn = vi.fn();
    render(<Chat onOpenSettings={() => {}} onSignIn={onSignIn} />);
    const card = await screen.findByRole("region", { name: "Sign in" });
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Add your API key",
    ]);
    expect(card).toHaveTextContent(
      "Sign in to start" +
        "Loadout chats with your xAI API key. It's kept in this device's keychain, never in a file." +
        "Add your API key" +
        "No key yet? Make one at console.x.ai.",
    );
    expect(screen.queryByText("Ask Loadout anything.")).toBeNull();
    await userEvent.click(within(card).getByRole("button", { name: "Add your API key" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);
  });

  it("where Grok sign-in is offered, the card leads with it, and a sign-in hides the card", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    const onSignIn = vi.fn();
    const { unmount } = render(<Chat onOpenSettings={() => {}} onSignIn={onSignIn} grokSignIn />);
    const card = await screen.findByRole("region", { name: "Sign in" });
    expect(card).toHaveTextContent("Sign in with your Grok account, or add an xAI API key.");
    await userEvent.click(within(card).getByRole("button", { name: "Sign in with Grok" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);
    unmount();
    api.getSettings.mockResolvedValue({
      ...SETTINGS,
      hasApiKey: false,
      signedIn: { name: "Sam", email: null },
    });
    render(<Chat onOpenSettings={() => {}} grokSignIn />);
    expect(await screen.findByText(CHAT_EMPTY)).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sign in" })).toBeNull();
  });

  it("signed out on the web app, it says to sign in on the computer", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    render(<Chat onOpenSettings={() => {}} remote />);
    expect(
      await screen.findByText(
        "Loadout on your computer isn't signed in yet. Sign in there, then chat here.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sign in" })).toBeNull();
  });
});

describe("Chat history", () => {
  beforeEach(() => {
    for (const fn of [api.sendChat, api.closeChat, api.cancelChat, api.loadChat]) fn.mockReset();
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.getLastChat.mockReset().mockResolvedValue(null);
    api.handler = null;
  });

  it("turns a saved chat's lines into bubbles and one muted tools line", () => {
    expect(itemsFrom(SAVED)).toEqual([
      { kind: "user", text: "pick a paint colour" },
      { kind: "tools", text: "read_file, web_search" },
      { kind: "assistant", text: "Try **blue**.", streaming: false },
    ]);
  });

  it("reopens the last chat on mount, and the next message carries it on", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    expect(await screen.findByText("pick a paint colour")).toHaveAttribute("data-role", "user");
    expect(screen.getByText("Tools ran: read_file, web_search")).toHaveAttribute("data-role", "tools");
    expect(screen.getByText("blue").closest("[data-role]")).toHaveAttribute("data-role", "assistant");
    await startTurn("and the trim?");
    expect(api.sendChat).toHaveBeenLastCalledWith("shell-saved", "and the trim?", undefined);
  });

  it("the web app never asks for the desktop's last chat", async () => {
    render(<Chat onOpenSettings={() => {}} remote />);
    await waitFor(() => expect(api.getSettings).toHaveBeenCalled());
    expect(api.getLastChat).not.toHaveBeenCalled();
  });

  it("a last chat that arrives after the person started typing away stays out", async () => {
    let arrive: (t: Transcript) => void = () => {};
    api.getLastChat.mockReturnValue(new Promise((resolve) => (arrive = resolve)));
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("fresh question");
    await act(async () => arrive(SAVED));
    expect(screen.queryByText("pick a paint colour")).toBeNull();
    expect(id).not.toBe("shell-saved");
  });

  it("a chat opened from History replaces the open one, which stays saved", async () => {
    api.sendChat.mockResolvedValue("end_turn");
    api.loadChat.mockResolvedValue(SAVED);
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    const first = await startTurn("hello");
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());

    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(await screen.findByText("pick a paint colour")).toBeVisible();
    expect(api.closeChat).toHaveBeenCalledWith(first);
    expect(api.loadChat).toHaveBeenCalledWith("shell-saved");
    expect(screen.queryByText("hello")).toBeNull();

    // Picking the chat that is already open does nothing.
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 2 }} />);
    expect(api.loadChat).toHaveBeenCalledTimes(1);
    expect(api.closeChat).toHaveBeenCalledTimes(1);
  });

  it("opening a chat waits for a running reply", async () => {
    const finish = holdTurn();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await startTurn("hello");
    rerender(<Chat onOpenSettings={() => {}} open={{ id: "shell-saved", n: 1 }} />);
    expect(screen.getByText(OPEN_BUSY)).toBeVisible();
    expect(api.loadChat).not.toHaveBeenCalled();
    expect(api.closeChat).not.toHaveBeenCalled();
    finish();
  });

  it("deleting the open chat in History starts a new one; another chat's delete changes nothing", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} deleted={{ id: "shell-other", n: 1 }} />);
    expect(screen.getByText("pick a paint colour")).toBeVisible();
    rerender(<Chat onOpenSettings={() => {}} deleted={{ id: "shell-saved", n: 2 }} />);
    await waitFor(() => expect(screen.queryByText("pick a paint colour")).toBeNull());
    const id = await startTurn("new start");
    expect(id).not.toBe("shell-saved");
  });

  it("Clear chat deletes the open chat's saved history and starts a new one", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.deleteChat.mockResolvedValue();
    api.sendChat.mockResolvedValue("end_turn");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(screen.queryByText("pick a paint colour")).toBeNull());
    expect(api.deleteChat).toHaveBeenCalledWith("shell-saved");
    const id = await startTurn("new start");
    expect(id).not.toBe("shell-saved");
  });

  it("Clear chat keeps the chat on screen when the delete fails", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.deleteChat.mockRejectedValue("disk is read-only");
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await screen.findByText("pick a paint colour");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    expect(await screen.findByText(/disk is read-only/)).toBeVisible();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("a bubble's ··· deletes that message after asking, and a reply goes with its tools", async () => {
    api.getLastChat.mockResolvedValue({
      ...SAVED,
      lines: [...SAVED.lines, { role: "user", text: "and the trim?" }, { role: "assistant", text: "White." }],
    });
    api.forgetMessage.mockReset().mockResolvedValue(true);
    render(<Chat onOpenSettings={() => {}} />);
    const ask = await screen.findByText("and the trim?");
    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    const dialog = screen.getByRole("alertdialog", { name: DELETE_ASK_QUESTION });
    await userEvent.click(within(dialog).getByRole("button", { name: "Keep" }));
    expect(api.forgetMessage).not.toHaveBeenCalled();
    expect(screen.getByText("and the trim?")).toBeVisible();

    await userEvent.click(within(ask).getByRole("button", { name: MESSAGE_MENU_LABEL }));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.forgetMessage).toHaveBeenCalledWith("shell-saved", {
      from: 3,
      to: 4,
      reply: false,
      text: "and the trim?",
      later: 0,
    });
    await waitFor(() => expect(screen.queryByText("and the trim?")).toBeNull());

    // The ··· adds nothing to a bubble's text, so copying or reading it
    // gives the message alone.
    expect(screen.getByText("White.").closest(".bubble")?.textContent).toBe("White.");

    // Right-click on a reply: the whole reply to "pick a paint colour" goes,
    // its tools line included, now that "White." answers it too.
    fireEvent.contextMenu(screen.getByText("White."));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    expect(screen.getByRole("alertdialog", { name: DELETE_REPLY_QUESTION })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(api.forgetMessage).toHaveBeenLastCalledWith(
      "shell-saved",
      expect.objectContaining({ reply: true, text: "pick a paint colour", later: 0 }),
    );
    await waitFor(() => expect(screen.queryByText("White.")).toBeNull());
    expect(screen.queryByText("Tools ran: read_file, web_search")).toBeNull();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("a delete the saved chat refuses keeps the message and says why", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.forgetMessage.mockReset().mockRejectedValue("Wait for the reply to finish, then delete the message.");
    render(<Chat onOpenSettings={() => {}} />);
    fireEvent.contextMenu(await screen.findByText("pick a paint colour"));
    await userEvent.click(screen.getByRole("menuitem", { name: DELETE_MESSAGE_LABEL }));
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText(/Wait for the reply to finish/)).toBeVisible();
    expect(screen.getByText("pick a paint colour")).toBeVisible();
  });

  it("the web app offers no message menu", async () => {
    api.getLastChat.mockResolvedValue(SAVED);
    api.sendChat.mockResolvedValue("end_turn");
    render(<Chat onOpenSettings={() => {}} remote />);
    await startTurn("hello from the phone");
    expect(screen.queryByRole("button", { name: MESSAGE_MENU_LABEL })).toBeNull();
  });

  it("Show earlier loads more of a long chat's start", async () => {
    api.getLastChat.mockResolvedValue({ ...SAVED, earlier: 5 });
    api.loadChat.mockResolvedValue({
      ...SAVED,
      lines: [{ role: "user", text: "the very first ask" }, ...SAVED.lines],
      earlier: 0,
    });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Show earlier messages (5)" }));
    expect(api.loadChat).toHaveBeenCalledWith("shell-saved", 3 + EARLIER_STEP);
    expect(await screen.findByText("the very first ask")).toBeVisible();
    expect(screen.queryByRole("button", { name: /Show earlier/ })).toBeNull();
  });
});

describe("Chat queue", () => {
  /** Each sendChat waits until its `end(stop)` is called. */
  function turns() {
    const ends: ((stop: string) => void)[] = [];
    api.sendChat.mockReset().mockImplementation(() => new Promise<string>((resolve) => ends.push(resolve)));
    return ends;
  }

  beforeEach(() => {
    api.getLastChat.mockReset().mockResolvedValue(null);
    api.quickChips.mockReset().mockResolvedValue([]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.cancelChat.mockReset().mockResolvedValue();
    api.steerChat.mockReset();
    api.sideChat.mockReset();
    api.tellMain.mockReset();
    api.readTurn.mockReset().mockImplementation((text: string) =>
      Promise.resolve({ route: "queue", text, ack: "Queued for after this reply" }),
    );
  });

  it("a side question gets its own answer beside the reply, never in the chat", async () => {
    const ends = turns();
    let endSide: (stop: string) => void = () => {};
    api.sideChat.mockImplementation(() => new Promise<string>((resolve) => (endSide = resolve)));
    api.readTurn.mockResolvedValue({ route: "side", text: "what is a lockfile", ack: "Answering on the side" });
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "update the deps{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "btw what is a lockfile{Enter}");
    await waitFor(() =>
      expect(api.sideChat).toHaveBeenCalledWith(id, `${id}~side-1`, "what is a lockfile", "update the deps", ""),
    );
    const card = screen.getByRole("group", { name: "On the side: what is a lockfile" });
    expect(card).toHaveTextContent("Answering…");
    emit(`${id}~side-1`, { kind: "text", text: "It pins exact versions." });
    expect(card).toHaveTextContent("It pins exact versions.");
    await act(async () => endSide("end_turn"));
    // The main chat never shows the side answer, and nothing queued.
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    api.tellMain.mockResolvedValue({ text: "About lockfiles: It pins exact versions.", steered: true });
    await userEvent.click(screen.getByRole("button", { name: "Tell the main task" }));
    expect(api.tellMain).toHaveBeenCalledWith(id, "what is a lockfile", "It pins exact versions.");
    await waitFor(() => expect(screen.queryByRole("group", { name: /On the side/ })).toBeNull());
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a correction during a reply steers it, and stop stops it", async () => {
    turns();
    api.steerChat.mockResolvedValue(true);
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "write the docs{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    api.readTurn.mockResolvedValueOnce({ route: "steer", text: "use British spelling", ack: "Added to the reply" });
    await userEvent.type(screen.getByLabelText("Message"), "use British spelling{Enter}");
    await waitFor(() => expect(api.steerChat).toHaveBeenCalledWith(id, "use British spelling"));
    api.readTurn.mockResolvedValueOnce({ route: "stop", text: "stop", ack: "Stopping" });
    await userEvent.type(screen.getByLabelText("Message"), "stop{Enter}");
    await waitFor(() => expect(api.cancelChat).toHaveBeenCalledWith(id));
  });

  it("lines up messages sent during a reply, reports them, and sends the next when it ends", async () => {
    const ends = turns();
    const onQueue = vi.fn();
    render(<Chat onOpenSettings={() => {}} onQueue={onQueue} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const box = screen.getByLabelText("Message");
    await userEvent.type(box, "then deploy");
    expect(screen.getByRole("button", { name: "Send" })).toBeVisible();
    await userEvent.type(box, "{Enter}");
    await userEvent.type(box, "and tell me{Enter}");
    expect(box).toHaveValue("");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    // Queued messages are not in the chat log until they send.
    expect(screen.queryByText("then deploy")).toBeNull();
    const id = api.sendChat.mock.calls[0][0];
    expect(onQueue).toHaveBeenLastCalledWith(id, ["then deploy", "and tell me"]);
    await act(async () => ends[0]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][1]).toBe("then deploy");
    expect(onQueue).toHaveBeenLastCalledWith(id, ["and tell me"]);
    await act(async () => ends[1]("end_turn"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(3));
    expect(api.sendChat.mock.calls[2][1]).toBe("and tell me");
    await act(async () => ends[2]("end_turn"));
    expect(onQueue).toHaveBeenLastCalledWith(id, []);
  });

  it("Clear chat mid-reply opens a chat that shows Send, and the old chat's waiting words go back in the box", async () => {
    const ends = turns();
    api.deleteChat.mockResolvedValue();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const old = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(api.deleteChat).toHaveBeenCalledWith(old));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Stop" })).toBeNull());
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    // The old reply ends after the new chat is open: nothing of it sends here.
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
  });

  it("a message typed in the new chat while the old reply ends goes to the new chat", async () => {
    const ends = turns();
    api.deleteChat.mockResolvedValue();
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const old = api.sendChat.mock.calls[0][0];
    rerender(<Chat onOpenSettings={() => {}} clear={{ n: 1 }} />);
    await waitFor(() => expect(api.deleteChat).toHaveBeenCalledWith(old));
    await userEvent.type(screen.getByLabelText("Message"), "fresh start");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    await act(async () => ends[0]("cancelled"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    expect(api.sendChat.mock.calls[1][0]).not.toBe(old);
    expect(api.sendChat.mock.calls[1][1]).toBe("fresh start");
  });

  it("Stop puts what waited back in the box instead of sending it", async () => {
    const ends = turns();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledTimes(1);
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    await act(async () => ends[0]("cancelled"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("Remove puts the words back in an empty box; Steer now hands them to the reply", async () => {
    const ends = turns();
    api.steerChat.mockResolvedValue(true);
    const onQueue = vi.fn();
    const chat = (queueAct?: QueueAct) => <Chat onOpenSettings={() => {}} onQueue={onQueue} queueAct={queueAct} />;
    const { rerender } = render(chat());
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    const id = api.sendChat.mock.calls[0][0];
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await userEvent.type(screen.getByLabelText("Message"), "use staging{Enter}");
    rerender(chat({ op: "remove", index: 0, n: 1 }));
    await waitFor(() => expect(screen.getByLabelText("Message")).toHaveValue("then deploy"));
    expect(onQueue).toHaveBeenLastCalledWith(id, ["use staging"]);
    rerender(chat({ op: "steer", index: 0, n: 2 }));
    await waitFor(() => expect(api.steerChat).toHaveBeenCalledWith(id, "use staging"));
    await waitFor(() => expect(onQueue).toHaveBeenLastCalledWith(id, []));
    // Steered words show as sent; nothing more goes when the reply ends.
    expect(screen.getByText("use staging")).toBeVisible();
    await act(async () => ends[0]("end_turn"));
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });

  it("a Halt puts the queue back in the box", async () => {
    const ends = turns();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "first{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    await userEvent.type(screen.getByLabelText("Message"), "then deploy{Enter}");
    await act(async () => ends[0]("halted"));
    expect(screen.getByLabelText("Message")).toHaveValue("then deploy");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
  });
});
