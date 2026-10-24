import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Answer, ChatEnvelope, ChatEvent } from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  sendChat: vi.fn<(id: string, text: string) => Promise<string>>(),
  answerPermission: vi.fn<(id: string, callId: string, answer: Answer) => Promise<void>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  closeChat: vi.fn<(id: string) => Promise<void>>(),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  answerPermission: api.answerPermission,
  cancelChat: api.cancelChat,
  closeChat: api.closeChat,
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
}));

import { applyEvent, Chat, type Item } from "./Chat";

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
    for (const fn of [api.sendChat, api.answerPermission, api.cancelChat, api.closeChat]) {
      fn.mockReset();
    }
    api.handler = null;
  });

  it("sends the text and streams its own conversation's reply", async () => {
    const finish = holdTurn();
    render(<Chat onOpenSettings={() => {}} />);
    const id = await startTurn("hello");

    expect(id).toMatch(/^shell-/);
    expect(api.sendChat).toHaveBeenCalledWith(id, "hello");
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
    expect(screen.queryByRole("group")).toBeNull();
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
});
