import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LiveEvent, Spoken } from "./api";

const api = vi.hoisted(() => ({
  voiceLiveStart: vi.fn<() => Promise<void>>(),
  voiceLiveStop: vi.fn<() => Promise<void>>(),
  voiceLivePush: vi.fn<(block: string) => Promise<LiveEvent[]>>(),
  voiceLiveListen: vi.fn<(playing: boolean) => Promise<void>>(),
  voiceLivePlaying: vi.fn<(playing: boolean) => Promise<void>>(),
  voiceLiveFinish: vi.fn<() => Promise<void>>(),
  voiceLiveHear: vi.fn<() => Promise<string>>(),
  voiceReplyFeed: vi.fn<(delta: string) => Promise<string[]>>(),
  voiceReplyFinish: vi.fn<(whole: string) => Promise<string[]>>(),
  voiceReplyHush: vi.fn<() => Promise<void>>(),
  voiceSpeak: vi.fn<(text: string) => Promise<Spoken>>(),
  transcribe: vi.fn(),
}));
vi.mock("./api", () => api);

import {
  BLOCK,
  PAUSED_NOTE,
  QUIET_NOTE,
  toPcm16,
  VoiceButton,
  VoiceSession,
  voiceStep,
  type Playing,
  type VoiceState,
} from "./VoiceMode";

const settle = () => new Promise((r) => setTimeout(r, 0));
async function flush() {
  for (let i = 0; i < 8; i++) await settle();
}

interface Rig {
  session: VoiceSession;
  states: VoiceState[];
  notes: string[];
  sent: string[];
  played: string[];
  ends: (() => void)[];
  stops: number;
  closed: number;
  /** The mic hears `events` in reply to the next block. */
  hear: (...events: LiveEvent[]) => Promise<void>;
}

function rig(): Rig {
  const r = { states: [], notes: [], sent: [], played: [], ends: [], stops: 0, closed: 0 } as unknown as Rig;
  let take: ((s: Float32Array, rate: number) => void) | null = null;
  r.session = new VoiceSession({
    openMic: async (t) => {
      take = t;
      return () => r.closed++;
    },
    play: (spoken): Playing => {
      r.played.push(spoken.audio);
      let end = () => {};
      const done = new Promise<void>((res) => (end = res));
      r.ends.push(end);
      return {
        done,
        stop: () => {
          r.stops++;
          end();
        },
      };
    },
    send: (w) => r.sent.push(w),
    onState: (s) => r.states.push(s),
    onNote: (n) => r.notes.push(n),
  });
  r.hear = async (...events) => {
    api.voiceLivePush.mockResolvedValueOnce(events);
    take!(new Float32Array(BLOCK), 16000);
    await flush();
  };
  return r;
}

beforeEach(() => {
  vi.clearAllMocks();
  for (const fn of [
    api.voiceLiveStart,
    api.voiceLiveStop,
    api.voiceLiveListen,
    api.voiceLivePlaying,
    api.voiceLiveFinish,
    api.voiceReplyHush,
  ])
    fn.mockResolvedValue(undefined);
  api.voiceLivePush.mockResolvedValue([]);
  api.voiceLiveHear.mockResolvedValue("what's on tonight");
  api.voiceReplyFeed.mockResolvedValue([]);
  api.voiceReplyFinish.mockResolvedValue([]);
  api.voiceSpeak.mockImplementation(async (text) => ({ mime: "audio/mpeg", audio: text }));
});

describe("voiceStep", () => {
  it("walks listening, thinking, speaking and back", () => {
    let s: VoiceState = "off";
    for (const [ev, want] of [
      ["start", "listening"],
      ["turn", "thinking"],
      ["spoke", "speaking"],
      ["done", "listening"],
      ["turn", "thinking"],
      ["empty", "listening"],
      ["stop", "off"],
    ] as const) {
      s = voiceStep(s, ev);
      expect(s).toBe(want);
    }
  });

  it("talking over a reply goes back to listening, and off stays off", () => {
    expect(voiceStep("speaking", "barge")).toBe("listening");
    expect(voiceStep("off", "turn")).toBe("off");
    expect(voiceStep("thinking", "barge")).toBe("thinking");
  });
});

it("turns float samples into 16-bit little-endian PCM", () => {
  const pcm = toPcm16(new Float32Array([0, 1, -1, 2]));
  const view = new DataView(pcm.buffer);
  expect([0, 1, 2, 3].map((i) => view.getInt16(i * 2, true))).toEqual([0, 32767, -32768, 32767]);
});

describe("VoiceSession", () => {
  it("sends the mic in 100 ms blocks, one at a time", async () => {
    const r = rig();
    await r.session.start();
    expect(r.states).toEqual(["listening"]);
    await r.hear();
    expect(api.voiceLivePush).toHaveBeenCalledTimes(1);
    // 48 kHz audio is sent at 16 kHz: 4800 samples make one block.
    api.voiceLivePush.mockClear();
    const take = (r.session as unknown as { take: (s: Float32Array, rate: number) => void }).take.bind(r.session);
    take(new Float32Array(4800 * 2 + 100), 48000);
    await flush();
    expect(api.voiceLivePush).toHaveBeenCalledTimes(2);
    expect(atob(api.voiceLivePush.mock.calls[0][0]).length).toBe(BLOCK * 2);
  });

  it("a finished turn is heard and sent as the message", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "started" });
    expect(r.session.talking).toBe(true);
    await r.hear({ kind: "turn", ms: 1800 });
    expect(r.sent).toEqual(["what's on tonight"]);
    expect(r.states).toEqual(["listening", "thinking"]);
  });

  it("speaks the reply as it streams, listens over it, then listens again", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValueOnce(["Sure, the first one starts at eight."]);
    r.session.feed("Sure, the first one starts at eight. ");
    await flush();
    expect(r.played).toEqual(["Sure, the first one starts at eight."]);
    expect(r.states.at(-1)).toBe("speaking");
    expect(api.voiceLiveListen).toHaveBeenLastCalledWith(true);
    api.voiceReplyFinish.mockResolvedValueOnce(["Have fun."]);
    r.session.replyDone("");
    await flush();
    r.ends[0]();
    await flush();
    expect(r.played).toEqual(["Sure, the first one starts at eight.", "Have fun."]);
    expect(r.states.at(-1)).toBe("speaking");
    r.ends[1]();
    await flush();
    expect(r.states.at(-1)).toBe("listening");
    expect(api.voiceLivePlaying).toHaveBeenLastCalledWith(false);
  });

  it("talking over the reply stops it and hushes the rest", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValueOnce(["One.", "Two."]);
    r.session.feed("One. Two. ");
    await flush();
    expect(r.states.at(-1)).toBe("speaking");
    await r.hear({ kind: "started" });
    expect(r.stops).toBe(1);
    expect(api.voiceReplyHush).toHaveBeenCalled();
    expect(r.states.at(-1)).toBe("listening");
    expect(r.played).toEqual(["One."]);
  });

  it("a turn with no words listens again and sends nothing", async () => {
    const r = rig();
    api.voiceLiveHear.mockResolvedValueOnce("  ");
    await r.session.start();
    await r.hear({ kind: "turn", ms: 400 });
    expect(r.sent).toEqual([]);
    expect(r.states).toEqual(["listening", "thinking", "listening"]);
    expect(api.voiceLiveListen).toHaveBeenLastCalledWith(false);
  });

  it("a reply with nothing to speak goes back to listening", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    r.session.replyDone("");
    await flush();
    expect(r.states.at(-1)).toBe("listening");
    expect(api.voiceLiveListen).toHaveBeenLastCalledWith(false);
  });

  it("without a voice it says so once and the chat goes on", async () => {
    const r = rig();
    api.voiceSpeak.mockRejectedValue(new Error("No voice is set up."));
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValueOnce(["One."]);
    r.session.feed("One. ");
    api.voiceReplyFinish.mockResolvedValueOnce(["Two."]);
    r.session.replyDone("");
    await flush();
    expect(r.notes).toEqual(["No voice is set up."]);
    expect(r.played).toEqual([]);
    expect(r.states.at(-1)).toBe("listening");
  });

  it("a quiet minute checks in, then turns voice mode off", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "check_in" }, { kind: "pause" });
    expect(r.notes).toEqual([QUIET_NOTE, PAUSED_NOTE]);
    expect(r.states.at(-1)).toBe("off");
    expect(r.closed).toBe(1);
    expect(api.voiceLiveStop).toHaveBeenCalled();
  });

  it("a tap ends the turn while talking, and turns voice mode off otherwise", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "started" });
    await r.session.tap();
    expect(api.voiceLiveFinish).toHaveBeenCalled();
    await r.hear({ kind: "nothing" });
    await r.session.tap();
    expect(r.states.at(-1)).toBe("off");
    expect(r.closed).toBe(1);
  });

  it("a mic that can't open leaves voice mode off", async () => {
    const r = rig();
    const s = new VoiceSession({
      openMic: () => Promise.reject(new Error("No microphone found.")),
      play: () => ({ done: Promise.resolve(), stop: () => {} }),
      send: () => {},
      onState: (st) => r.states.push(st),
      onNote: () => {},
    });
    await expect(s.start()).rejects.toThrow("No microphone found.");
    expect(s.state).toBe("off");
    expect(api.voiceLiveStop).toHaveBeenCalled();
  });
});

describe("VoiceButton", () => {
  it("shows each state", () => {
    const r = rig();
    const { rerender } = render(<VoiceButton state="off" session={r.session} />);
    expect(screen.getByRole("button", { name: /talk hands-free/i })).toHaveAttribute("aria-pressed", "false");
    for (const [state, text] of [
      ["listening", "Listening"],
      ["thinking", "Thinking"],
      ["speaking", "Speaking"],
    ] as const) {
      rerender(<VoiceButton state={state} session={r.session} />);
      expect(screen.getByRole("button")).toHaveTextContent(text);
      expect(screen.getByRole("button")).toHaveAttribute("aria-pressed", "true");
    }
  });

  it("a press turns voice mode on", async () => {
    const r = rig();
    render(<VoiceButton state="off" session={r.session} />);
    await userEvent.click(screen.getByRole("button"));
    await flush();
    expect(api.voiceLiveStart).toHaveBeenCalled();
    expect(r.session.state).toBe("listening");
  });
});
