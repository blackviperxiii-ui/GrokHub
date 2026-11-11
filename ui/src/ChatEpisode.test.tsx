// The chat's side of a supervised desktop session: an allowed step goes on
// through the chat (`Continue`), at once when nothing runs, or as soon as
// the running reply ends. Episode.test.tsx covers the panel itself.
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatEnvelope, SessionMode, Settings } from "./api";
import type { EpisodePanel } from "./episodeApi";

const api = vi.hoisted(() => ({
  sendChat: vi.fn<(id: string, text: string, mode?: SessionMode) => Promise<string>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
}));

const episode = vi.hoisted(() => ({
  handler: null as ((panel: EpisodePanel) => void) | null,
  state: vi.fn<(id: string) => Promise<EpisodePanel | null>>(),
  // The command answers whether the window should send the resume now.
  answer: vi.fn<(id: string, parkId: string, approve: boolean) => Promise<boolean>>(),
}));

vi.mock("./autoSetupApi", () => ({ settingAsk: () => Promise.resolve(null) }));

vi.mock("./episodeApi", () => ({
  onEpisode: (h: (panel: EpisodePanel) => void) => {
    episode.handler = h;
    return Promise.resolve(() => {});
  },
  episodeState: episode.state,
  episodeAnswer: episode.answer,
  episodePause: vi.fn(() => Promise.resolve(true)),
  episodeStop: vi.fn(() => Promise.resolve(true)),
  episodeArm: vi.fn((_id: string, on: boolean) => Promise.resolve(on)),
  episodeArmed: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  getSettings: api.getSettings,
  answerElicit: vi.fn(),
  saveSettings: vi.fn(),
  answerPermission: vi.fn(),
  cancelChat: vi.fn(() => Promise.resolve()),
  steerChat: vi.fn(() => Promise.resolve(false)),
  readTurn: vi.fn((text: string) => Promise.resolve({ route: "queue", text, ack: "" })),
  sideChat: vi.fn(),
  tellMain: vi.fn(),
  closeChat: vi.fn(() => Promise.resolve()),
  deleteChat: vi.fn(),
  forgetMessage: vi.fn(),
  getLastChat: vi.fn(() => Promise.resolve(null)),
  loadChat: vi.fn(),
  slashHits: vi.fn(() => Promise.resolve([])),
  runSlash: vi.fn(),
  exportChat: vi.fn(),
  openSaved: vi.fn(),
  revealSaved: vi.fn(),
  quickChips: vi.fn(() => Promise.resolve([])),
  chipOutcome: vi.fn(() => Promise.resolve(true)),
  goalAct: vi.fn(),
  haltDesktop: vi.fn(() => Promise.resolve([])),
  onChatEvent: (_handler: (envelope: ChatEnvelope) => void) => Promise.resolve(() => {}),
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  transcribe: vi.fn(),
  onFileDrop: () => Promise.resolve(() => {}),
  stageAttachment: vi.fn(),
  stageAttachmentPaths: vi.fn(),
  pickAttachments: vi.fn(),
  removeAttachment: vi.fn(),
}));

import { Chat } from "./Chat";

const SETTINGS: Settings = {
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/user",
  approvals: "always",
  desktopControl: true,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
};

function panel(conversationId: string, over: Partial<EpisodePanel> & { resume?: boolean } = {}): EpisodePanel {
  return {
    conversationId,
    episodeId: "ep-1",
    header: "Desktop session · 1 step · 0 min",
    steps: [{ n: 1, tool: "click", decision: "park", result: "", goalStep: "Reply to the thread" }],
    parks: [{ id: "epk-ep-1-1", tool: "click", action: "Loadout wants to click Send in Mail", class: "send" }],
    frame: null,
    state: "running",
    ended: null,
    cua: false,
    ...over,
  };
}

/** Holds sendChat open until the returned function is called, like a reply in flight. */
function holdTurn() {
  let finish: (stop: string) => void = () => {};
  api.sendChat.mockImplementationOnce(() => new Promise<string>((resolve) => (finish = resolve)));
  return (stop = "end_turn") => finish(stop);
}

/** Renders the chat and waits for its session panel to listen. Returns the chat's id. */
async function openChat(): Promise<string> {
  render(<Chat onOpenSettings={() => {}} />);
  await waitFor(() => expect(episode.handler).not.toBeNull());
  await waitFor(() => expect(episode.state).toHaveBeenCalled());
  return episode.state.mock.calls[0][0];
}

const texts = () => api.sendChat.mock.calls.map((call) => call[1]);

beforeEach(() => {
  episode.handler = null;
  episode.state.mockReset().mockResolvedValue(null);
  episode.answer.mockReset().mockResolvedValue(false);
  api.sendChat.mockReset().mockResolvedValue("end_turn");
  api.getSettings.mockReset().mockResolvedValue(SETTINGS);
});

describe("Chat with a desktop session", () => {
  it("Allow with no reply running sends Continue through the chat", async () => {
    const id = await openChat();
    act(() => episode.handler?.(panel(id)));
    episode.answer.mockResolvedValue(true);
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(episode.answer).toHaveBeenCalledWith(id, "epk-ep-1-1", true);
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    expect(api.sendChat.mock.calls[0].slice(0, 2)).toEqual([id, "Continue"]);
  });

  it("an Allow the running reply takes, and a Deny, send nothing", async () => {
    const id = await openChat();
    act(() => episode.handler?.(panel(id)));
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(episode.answer).toHaveBeenCalledTimes(2));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(api.sendChat).not.toHaveBeenCalled();
  });

  it("a resume that is due while the reply is still ending waits for it, then sends once", async () => {
    const id = await openChat();
    const finish = holdTurn();
    await userEvent.type(screen.getByLabelText("Message"), "Reply to the thread{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    act(() => episode.handler?.(panel(id)));
    // The shell's reply has ended (it answers true) but the page's has not yet.
    episode.answer.mockResolvedValue(true);
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    await waitFor(() => expect(episode.answer).toHaveBeenCalledTimes(1));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(texts()).toEqual(["Reply to the thread"]);
    await act(async () => finish());
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(texts()).toEqual(["Reply to the thread", "Continue"]);
  });

  it("the shell's word that an allowed step was left behind sends Continue when the reply ends", async () => {
    const id = await openChat();
    const finish = holdTurn();
    await userEvent.type(screen.getByLabelText("Message"), "Reply to the thread{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    // The Allow landed after the session's last look; the shell says so as the reply ends.
    act(() => episode.handler?.(panel(id, { parks: [], resume: true })));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(texts()).toEqual(["Reply to the thread"]);
    await act(async () => finish());
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(2));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(texts()).toEqual(["Reply to the thread", "Continue"]);
  });

  it("a reply that was stopped sends no resume by itself: it goes back to the box", async () => {
    const id = await openChat();
    const finish = holdTurn();
    await userEvent.type(screen.getByLabelText("Message"), "Reply to the thread{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledTimes(1));
    act(() => episode.handler?.(panel(id, { parks: [], resume: true })));
    await act(async () => finish("cancelled"));
    await act(() => new Promise((r) => setTimeout(r, 20)));
    expect(texts()).toEqual(["Reply to the thread"]);
    expect(screen.getByLabelText("Message")).toHaveValue("Continue");
  });
});
