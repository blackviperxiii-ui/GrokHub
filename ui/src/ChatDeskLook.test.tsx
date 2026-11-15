import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";

// The chat tells her desktop pedestal what it is doing: idle, then thinking
// while a reply runs here, and nothing while the chat is off screen.
vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
    ...chatTabApiStubs(),
    quickChips: () => Promise.resolve([]),
    chipOutcome: () => Promise.resolve(true),
    getStartChat: () => Promise.resolve(null),
    loadChat: vi.fn(),
    sendChat: () => new Promise<string>(() => {}),
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
  });

  it("says idle, then thinking while a reply runs here", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("idle", null));
    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", null));
  });

  it("says nothing while the chat is off screen", async () => {
    render(<Chat onOpenSettings={() => {}} active={false} />);
    await screen.findByLabelText("Message");
    expect(desk.desktopNyxLook).not.toHaveBeenCalled();
  });
});
