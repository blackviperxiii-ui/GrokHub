import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  Answer,
  Approvals,
  ChatEnvelope,
  ChatEvent,
  ElicitAction,
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
      (model: string, tray: boolean, ws: string, approvals: Approvals, desk: boolean) => Promise<Settings>
    >(),
  answerPermission: vi.fn<(id: string, callId: string, answer: Answer) => Promise<void>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
  deleteChat: vi.fn<(id: string) => Promise<void>>(),
  getLastChat: vi.fn<() => Promise<Transcript | null>>(),
  loadChat: vi.fn<(id: string, limit?: number) => Promise<Transcript>>(),
  setModel: vi.fn<(id: string) => Promise<Settings>>(),
  listModels: vi.fn(),
  slashHits: vi.fn(() => Promise.resolve([])),
  runSlash: vi.fn(),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  answerElicit: api.answerElicit,
  getSettings: api.getSettings,
  saveSettings: api.saveSettings,
  answerPermission: api.answerPermission,
  cancelChat: api.cancelChat,
  closeChat: api.closeChat,
  deleteChat: api.deleteChat,
  getLastChat: api.getLastChat,
  loadChat: api.loadChat,
  setModel: api.setModel,
  listModels: api.listModels,
  slashHits: api.slashHits,
  runSlash: api.runSlash,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
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
  type Item,
} from "./Chat";

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
  model: "grok-4.7",
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
      expect(api.sendChat).toHaveBeenLastCalledWith("shell-m1", "Mission from my decision journal: Q", "chat"),
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
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith(id, "hello", "chat"));
  });

  it("shows at most three suggestions under a reply, and a tap sends one", async () => {
    let finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("the build failed");
    emit(id, { kind: "text", text: "TypeError in parse_row." });
    emit(id, {
      kind: "suggestions",
      items: [
        { label: "Fix the cause", prompt: "Fix the root cause in parse_row." },
        { label: "Root cause + fix", prompt: "Diagnose the error." },
        { label: "Add a test", prompt: "Add a test for parse_row." },
        { label: "Fourth", prompt: "Never shown." },
      ],
    });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    const card = await screen.findByRole("group", { name: "Suggestions" });
    expect(within(card).getAllByRole("button").map((b) => b.textContent)).toEqual([
      "Fix the cause",
      "Root cause + fix",
      "Add a test",
    ]);
    await waitFor(() => expect(within(card).getByRole("button", { name: "Fix the cause" })).toBeEnabled());
    await userEvent.type(screen.getByLabelText("Message"), "half typed");
    finish = holdTurn();
    await userEvent.click(within(card).getByRole("button", { name: "Fix the cause" }));
    await waitFor(() =>
      expect(api.sendChat).toHaveBeenLastCalledWith(id, "Fix the root cause in parse_row.", "chat"),
    );
    expect(screen.getByText("Fix the root cause in parse_row.")).toHaveAttribute("data-role", "user");
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
    expect(screen.getByLabelText("Message")).toHaveValue("half typed");
    finish();
  });

  it("posts a notice when an automation finishes, with a one-tap follow-up", async () => {
    const onOpenAutomations = vi.fn();
    const long = `${"x".repeat(300)} tail`;
    const view = render(<Chat onOpenSettings={() => {}} onOpenAutomations={onOpenAutomations} />);
    view.rerender(
      <Chat
        onOpenSettings={() => {}}
        onOpenAutomations={onOpenAutomations}
        notice={{ id: "auto-1", name: "summarize my inbox", ok: true, text: "3 new mails, none urgent.", n: 1 }}
      />,
    );
    const done = screen.getByRole("status", { name: "summarize my inbox finished" });
    expect(within(done).getByText("Finished")).toBeVisible();
    expect(within(done).getByText("3 new mails, none urgent.")).toBeVisible();
    await userEvent.click(within(done).getByRole("button", { name: "Ask about it" }));
    expect(screen.getByLabelText("Message")).toHaveValue('About the "summarize my inbox" automation run: ');
    await userEvent.click(within(done).getByRole("button", { name: "Open Automations" }));
    expect(onOpenAutomations).toHaveBeenCalledTimes(1);

    view.rerender(
      <Chat
        onOpenSettings={() => {}}
        onOpenAutomations={onOpenAutomations}
        notice={{ id: "auto-2", name: "board", ok: false, text: long, n: 2 }}
      />,
    );
    const failed = screen.getByRole("status", { name: "board finished" });
    expect(within(failed).getByText("Failed")).toBeVisible();
    expect(within(failed).getByText(`${"x".repeat(280)}…`)).toBeVisible();
    await userEvent.click(within(failed).getByRole("button", { name: "Find the cause" }));
    expect(screen.getByLabelText("Message")).toHaveValue(
      `The "board" automation failed: ${long}. What went wrong and how do I fix it?`,
    );
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("a reply without suggestions shows no suggestion card", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("list the files");
    emit(id, { kind: "text", text: "There are 12 files." });
    emit(id, { kind: "suggestions", items: [] });
    emit(id, { kind: "done", stop: "end_turn" });
    finish();
    await waitFor(() => expect(screen.getByLabelText("Message")).toBeEnabled());
    expect(screen.getByText("There are 12 files.")).toBeVisible();
    expect(screen.queryByRole("group", { name: "Suggestions" })).toBeNull();
  });

  it("sends the text and streams its own conversation's reply", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");

    expect(id).toMatch(/^shell-/);
    expect(api.sendChat).toHaveBeenCalledWith(id, "hello", "chat");
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

  it("sends with the picked chip, and the chips wait while a reply runs", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "btw" }));
    expect(screen.getByRole("button", { name: "btw" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByLabelText("Message")).toHaveAttribute("placeholder", "Ask on the side");
    const id = await startTurn("what does main.rs do");
    expect(api.sendChat).toHaveBeenCalledWith(id, "what does main.rs do", "btw");
    expect(screen.getByRole("button", { name: "Chat" })).toBeDisabled();
    finish();
    await waitFor(() => expect(screen.getByRole("button", { name: "Chat" })).toBeEnabled());
  });

  it("the approvals chips show and save Settings' approvals", async () => {
    api.saveSettings.mockResolvedValue({ ...SETTINGS, approvals: "auto" });
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Ask" })).toHaveAttribute("aria-pressed", "true"),
    );
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute(
      "title",
      "A quick check approves routine steps and asks you about the rest.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Auto" }));
    expect(api.saveSettings).toHaveBeenCalledWith("grok-4.7", true, "", "auto", false);
    expect(screen.getByRole("button", { name: "Auto" })).toHaveAttribute("aria-pressed", "true");
  });

  it("on the web app the approvals chips only show the computer's", async () => {
    render(<Chat onOpenSettings={() => {}} remote />);
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Ask" })).toHaveAttribute("aria-pressed", "true"),
    );
    for (const name of ["Ask", "Auto", "Always"]) {
      expect(screen.getByRole("button", { name })).toBeDisabled();
      expect(screen.getByRole("button", { name })).toHaveAttribute(
        "title",
        "Set in Loadout on your computer.",
      );
    }
  });

  it("a plan card approves, then the chip goes back to Chat", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "Plan" }));
    const id = await startTurn("tidy the build");
    expect(api.sendChat).toHaveBeenCalledWith(id, "tidy the build", "plan");
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
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true"),
    );
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
    expect(api.sendChat).toHaveBeenLastCalledWith("shell-saved", "and the trim?", "chat");
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
