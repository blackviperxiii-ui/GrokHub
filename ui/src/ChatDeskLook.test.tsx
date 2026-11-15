import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";

// The chat tells her desktop pedestal what it is doing: idle, then thinking
// while a reply runs here, and nothing while the chat is off screen.
const replies = vi.hoisted(() => ({ ends: [] as ((stop: string) => void)[] }));
vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
    ...chatTabApiStubs(),
    quickChips: () => Promise.resolve([]),
    chipOutcome: () => Promise.resolve(true),
    getStartChat: () => Promise.resolve(null),
    loadChat: vi.fn(),
    sendChat: () => new Promise<string>((resolve) => replies.ends.push(resolve)),
    getSettings: () => Promise.resolve({ hasApiKey: true, approvals: "ask" } as Settings),
    closeChat: () => Promise.resolve(),
    onChatEvent: () => Promise.resolve(() => {}),
    helperQuestions: () => Promise.resolve([]),
    onEventGap: () => Promise.resolve(() => {}),
    onFileDrop: () => Promise.resolve(() => {}),
    removeAttachment: () => Promise.resolve(),
  };
});

const desk = vi.hoisted(() => ({
  desktopNyxLook: vi.fn<(look: string, mood: string | null) => Promise<void>>(),
}));
vi.mock("./desktopNyxApi", async (load) => ({
  ...(await load<typeof import("./desktopNyxApi")>()),
  desktopNyxLook: desk.desktopNyxLook,
}));
vi.mock("./transport", async (load) => ({
  ...(await load<typeof import("./transport")>()),
  inShell: () => true,
}));

import { Chat } from "./Chat";

describe("Chat and her desktop pedestal", () => {
  beforeEach(() => {
    desk.desktopNyxLook.mockReset().mockResolvedValue();
    replies.ends.length = 0;
  });

  it("says idle, then thinking while a reply runs here", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("idle", "hanging-out"));
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", "thinking"));
  });

  it("says nothing from a chat that was never on screen", async () => {
    render(<Chat onOpenSettings={() => {}} active={false} />);
    await screen.findByLabelText("Message");
    expect(desk.desktopNyxLook).not.toHaveBeenCalled();
  });

  it("speaks from the chat last on screen, and keeps it while no chat is on screen", async () => {
    const both = (a: boolean, b: boolean) => (
      <>
        <div data-testid="a">
          <Chat onOpenSettings={() => {}} active={a} />
        </div>
        <div data-testid="b">
          <Chat onOpenSettings={() => {}} active={b} />
        </div>
      </>
    );
    const { rerender } = render(both(true, false));
    const a = within(screen.getByTestId("a"));
    await userEvent.type(a.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", "thinking"));
    // The other chat comes on screen: it speaks now.
    rerender(both(false, true));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("idle", "hanging-out"));
    // Neither on screen (Settings): the hidden chat still running a reply
    // doesn't take her back; the last one on screen still speaks.
    const calls = desk.desktopNyxLook.mock.calls.length;
    rerender(both(false, false));
    await new Promise((r) => setTimeout(r, 50));
    expect(desk.desktopNyxLook.mock.calls.slice(calls - 1)).toEqual(
      desk.desktopNyxLook.mock.calls.slice(calls - 1).map(() => ["idle", "hanging-out"]),
    );
  });

  it("settles her from a chat that went off screen when its reply ends", async () => {
    const one = (active: boolean) => <Chat onOpenSettings={() => {}} active={active} />;
    const { rerender } = render(one(true));
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", "thinking"));
    // Settings opens; the reply ends there.
    rerender(one(false));
    act(() => replies.ends[0]("end_turn"));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("idle", "hanging-out"));
  });

  it("settles her when the chat speaking for her goes away mid-reply", async () => {
    const { unmount } = render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", "thinking"));
    const calls = desk.desktopNyxLook.mock.calls.length;
    unmount();
    expect(desk.desktopNyxLook.mock.calls.slice(calls)).toEqual([["idle", "hanging-out"]]);
  });
});
