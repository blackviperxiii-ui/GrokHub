import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Answer, Approvals, ChatEnvelope, ChatEvent, ElicitAction, SessionMode, Settings } from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
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
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  answerElicit: api.answerElicit,
  getSettings: api.getSettings,
  saveSettings: api.saveSettings,
  answerPermission: api.answerPermission,
  cancelChat: api.cancelChat,
  closeChat: api.closeChat,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
}));

import { applyEvent, Chat, MISSION_BUSY, type Item } from "./Chat";

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
    ]) {
      fn.mockReset();
    }
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.handler = null;
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
    expect(screen.getByText("Hi from Grok")).toHaveClass("streaming");
    expect(screen.queryByText(/nope/)).toBeNull();

    finish();
    await waitFor(() => expect(screen.getByText("Hi from Grok")).not.toHaveClass("streaming"));
    expect(screen.getByText("Hi from Grok")).toHaveAttribute("data-role", "assistant");
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
    expect(card).toHaveTextContent("GrokHub wants to run write");
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
        "Set in GrokHub on your computer.",
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
    const card = screen.getByRole("group", { name: "Grok's plan" });
    expect(card).toHaveTextContent("1. Read build.rs");
    expect(screen.getAllByText("1. Read build.rs")).toHaveLength(1);
    expect(screen.queryByText("…")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Approve plan" }));
    expect(api.answerElicit).toHaveBeenCalledWith(id, "plan-1", "accept", null, "");
    expect(card).toHaveTextContent("Approved. Grok can make the changes now.");
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
    expect(screen.getByRole("group", { name: "Grok asks" })).toHaveTextContent("Sent.");
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
        "GrokHub chats with your xAI API key. It's kept in this device's keychain, never in a file." +
        "Add your API key" +
        "No key yet? Make one at console.x.ai.",
    );
    expect(screen.queryByText("Ask Grok anything.")).toBeNull();
    await userEvent.click(within(card).getByRole("button", { name: "Add your API key" }));
    expect(onSignIn).toHaveBeenCalledTimes(1);
  });

  it("signed out on the web app, it says to add the key on the computer", async () => {
    api.getSettings.mockResolvedValue({ ...SETTINGS, hasApiKey: false });
    render(<Chat onOpenSettings={() => {}} remote />);
    expect(
      await screen.findByText(
        "GrokHub on your computer has no API key yet. Add one there, then chat here.",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("region", { name: "Sign in" })).toBeNull();
  });
});
