import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatDelta, ChatMessage } from "./api";

const api = vi.hoisted(() => ({
  deltaHandler: null as ((delta: ChatDelta) => void) | null,
  sendChat: vi.fn<(id: string, messages: ChatMessage[]) => Promise<string>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  cancelChat: api.cancelChat,
  onChatDelta: (handler: (delta: ChatDelta) => void) => {
    api.deltaHandler = handler;
    return Promise.resolve(() => {});
  },
}));

import { Chat } from "./Chat";

describe("Chat", () => {
  beforeEach(() => {
    api.sendChat.mockReset();
    api.cancelChat.mockReset();
    api.deltaHandler = null;
  });

  it("sends the history and shows the reply", async () => {
    api.sendChat.mockResolvedValue("Hi from Grok");
    render(<Chat onOpenSettings={() => {}} />);

    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");

    expect(await screen.findByText("Hi from Grok")).toHaveAttribute("data-role", "assistant");
    expect(screen.getByText("hello")).toHaveAttribute("data-role", "user");
    expect(api.sendChat).toHaveBeenCalledTimes(1);
    const [id, history] = api.sendChat.mock.calls[0];
    expect(id).toMatch(/^shell-/);
    expect(history).toEqual([{ role: "user", text: "hello" }]);
    expect(screen.getByLabelText("Message")).toHaveValue("");
  });

  it("streams deltas for its own conversation only, and Stop cancels it", async () => {
    let finish: (reply: string) => void = () => {};
    api.sendChat.mockImplementation(
      () => new Promise<string>((resolve) => (finish = resolve)),
    );
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "count");
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(api.deltaHandler).not.toBeNull());

    const id = api.sendChat.mock.calls[0][0];
    api.deltaHandler?.({ conversationId: "someone-else", text: "nope" });
    api.deltaHandler?.({ conversationId: id, text: "one, " });
    api.deltaHandler?.({ conversationId: id, text: "two" });
    expect(await screen.findByText("one, two")).toHaveClass("streaming");
    expect(screen.queryByText(/nope/)).toBeNull();

    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledWith(id);

    finish("one, two");
    await waitFor(() => expect(screen.getByText("one, two")).not.toHaveClass("streaming"));
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("ignores a delta that arrives after the reply resolved", async () => {
    api.sendChat.mockResolvedValue("done");
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "hi{Enter}");
    expect(await screen.findByText("done")).not.toHaveClass("streaming");

    const id = api.sendChat.mock.calls[0][0];
    act(() => api.deltaHandler?.({ conversationId: id, text: "late" }));

    expect(screen.queryByText("late")).toBeNull();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
    await userEvent.type(screen.getByLabelText("Message"), "again");
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("shows the error and links to Settings", async () => {
    api.sendChat.mockRejectedValue("Sign in with Grok or add an API key.");
    const openSettings = vi.fn();
    render(<Chat onOpenSettings={openSettings} />);

    await userEvent.type(screen.getByLabelText("Message"), "hi{Enter}");

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Sign in with Grok or add an API key.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Open Settings" }));
    expect(openSettings).toHaveBeenCalledTimes(1);
  });

  it("Shift+Enter adds a line instead of sending", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "a{Shift>}{Enter}{/Shift}b");
    expect(screen.getByLabelText("Message")).toHaveValue("a\nb");
    expect(api.sendChat).not.toHaveBeenCalled();
  });
});
