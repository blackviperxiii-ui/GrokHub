import "@testing-library/jest-dom/vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "./api";
import type { DesktopNyxTalk } from "./desktopNyxApi";
import type { VoiceState } from "./VoiceMode";

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
  desktopNyxTalk: vi.fn<(talk: DesktopNyxTalk | null) => Promise<void>>(),
}));
vi.mock("./desktopNyxApi", async (load) => ({
  ...(await load<typeof import("./desktopNyxApi")>()),
  desktopNyxLook: desk.desktopNyxLook,
  desktopNyxTalk: desk.desktopNyxTalk,
}));
// Each chat's voice mode, driven by the test: `voices.set[i]` is the i-th
// chat's.
const voices = vi.hoisted(() => ({
  set: [] as ((state: VoiceState) => void)[],
  talk: [] as ((talk: DesktopNyxTalk | null) => void)[],
  /** What every chat's voice mode heard so far. */
  caption: "",
}));
vi.mock("./VoiceMode", async (load) => {
  const real = await load<typeof import("./VoiceMode")>();
  const { useEffect, useState } = await import("react");
  return {
    ...real,
    useVoiceMode: () => {
      const [state, setState] = useState<VoiceState>("off");
      const [session] = useState(
        () =>
          new real.VoiceSession({
            openMic: () => Promise.resolve(() => {}),
            play: real.playSpoken,
            send: () => {},
            onState: () => {},
            onNote: () => {},
          }),
      );
      useEffect(() => {
        voices.set.push(setState);
      }, []);
      // Where the chat sends a spoken piece's loudness.
      session.talkTo = (talk) => void voices.talk.push(talk);
      return { state, session, caption: voices.caption, line: "" };
    },
  };
});
vi.mock("./transport", async (load) => ({
  ...(await load<typeof import("./transport")>()),
  inShell: () => true,
}));

import { Chat } from "./Chat";

describe("Chat and her desktop pedestal", () => {
  beforeEach(() => {
    desk.desktopNyxLook.mockReset().mockResolvedValue();
    desk.desktopNyxTalk.mockReset().mockResolvedValue();
    voices.talk.length = 0;
    replies.ends.length = 0;
    voices.set.length = 0;
    voices.caption = "";
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

  it("keeps her desk with a chat whose voice talks on while another chat is on screen", async () => {
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
    await waitFor(() => expect(voices.set).toHaveLength(2));
    act(() => voices.set[0]("speaking"));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("speaking", "hanging-out"));
    // The other chat comes on screen and starts a reply: the voice still speaks for her.
    rerender(both(false, true));
    const b = within(screen.getByTestId("b"));
    await userEvent.type(b.getByLabelText("Message"), "hello{Enter}");
    await new Promise((r) => setTimeout(r, 50));
    expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("speaking", "hanging-out");
    // The voice stops: the chat on screen sends its own look.
    act(() => voices.set[0]("off"));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("thinking", "thinking"));
  });

  it("sends a spoken reply's loudness to her desk from the chat that speaks for her alone", async () => {
    const both = (
      <>
        <Chat onOpenSettings={() => {}} active />
        <Chat onOpenSettings={() => {}} active={false} />
      </>
    );
    render(both);
    await waitFor(() => expect(voices.talk).toHaveLength(2));
    const talk = { levels: [0.5, 1], startedAt: 1_000 };
    voices.talk[1](talk);
    expect(desk.desktopNyxTalk).not.toHaveBeenCalled();
    // A stop with nothing out sends nothing.
    voices.talk[1](null);
    voices.talk[0](talk);
    voices.talk[0](null);
    expect(desk.desktopNyxTalk.mock.calls).toEqual([[talk], [null]]);
  });

  it("lets a live voice off screen talk for her, and a piece always gets its stop", async () => {
    const both = (
      <>
        <Chat onOpenSettings={() => {}} active />
        <Chat onOpenSettings={() => {}} active={false} />
      </>
    );
    render(both);
    await waitFor(() => expect(voices.talk).toHaveLength(2));
    // Voice mode is on in the chat off screen: it speaks for her.
    act(() => voices.set[1]("speaking"));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("speaking", "hanging-out"));
    const talk = { levels: [1], startedAt: 2_000 };
    voices.talk[0](talk);
    expect(desk.desktopNyxTalk).not.toHaveBeenCalled();
    voices.talk[1](talk);
    expect(desk.desktopNyxTalk.mock.calls).toEqual([[talk]]);
    // Its voice turns off mid-piece: the desk goes back to the chat on
    // screen, and the piece's stop still reaches her.
    act(() => voices.set[1]("off"));
    await waitFor(() => expect(desk.desktopNyxLook).toHaveBeenLastCalledWith("idle", "hanging-out"));
    voices.talk[1](null);
    expect(desk.desktopNyxTalk.mock.calls).toEqual([[talk], [null]]);
  });

  it("shows what voice mode hears above the composer while it listens", async () => {
    voices.caption = "remind me to call";
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(voices.set).toHaveLength(1));
    expect(screen.queryByText("“remind me to call”")).toBeNull();
    act(() => voices.set[0]("listening"));
    const line = await screen.findByText("“remind me to call”");
    expect(line.nextElementSibling).toHaveClass("composer");
  });

  it("keeps the log at its end while the caption grows, unless they scrolled up", async () => {
    const scrolls = vi.fn();
    const was = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrolls;
    try {
      const { rerender } = render(<Chat onOpenSettings={() => {}} />);
      await waitFor(() => expect(voices.set).toHaveLength(1));
      act(() => voices.set[0]("listening"));
      scrolls.mockClear();
      voices.caption = "remind me";
      rerender(<Chat onOpenSettings={() => {}} />);
      expect(scrolls).toHaveBeenCalledWith({ block: "end" });
      // They scroll up to read: the caption no longer moves the log.
      const log = screen.getByRole("log");
      Object.defineProperty(log, "scrollHeight", { configurable: true, value: 1000 });
      fireEvent.scroll(log);
      scrolls.mockClear();
      voices.caption = "remind me to call";
      rerender(<Chat onOpenSettings={() => {}} />);
      expect(scrolls).not.toHaveBeenCalled();
    } finally {
      Element.prototype.scrollIntoView = was;
    }
  });

  it("keeps the find match in view, not the log's end, while the caption grows", async () => {
    const scrolls = vi.fn();
    const was = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scrolls;
    try {
      const { rerender } = render(<Chat onOpenSettings={() => {}} />);
      await waitFor(() => expect(voices.set).toHaveLength(1));
      await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
      act(() => replies.ends[0]("end_turn"));
      act(() => voices.set[0]("listening"));
      await userEvent.keyboard("{Control>}f{/Control}");
      await userEvent.type(screen.getByRole("textbox", { name: "Find in this chat" }), "hello");
      const match = screen.getByText("hello").closest("[data-role]");
      expect(match).toHaveAttribute("data-find", "current");
      scrolls.mockClear();
      voices.caption = "remind me";
      rerender(<Chat onOpenSettings={() => {}} />);
      expect(scrolls.mock.calls).toEqual([[{ block: "nearest" }]]);
      expect(scrolls.mock.contexts[0]).toBe(match);
      // They scroll away to read: the match stays where they left it.
      vi.spyOn(match!, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 2000, 100, 40));
      scrolls.mockClear();
      voices.caption = "remind me to call";
      rerender(<Chat onOpenSettings={() => {}} />);
      expect(scrolls).not.toHaveBeenCalled();
    } finally {
      Element.prototype.scrollIntoView = was;
    }
  });
});
