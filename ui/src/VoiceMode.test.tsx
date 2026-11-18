import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { HeardWords, LibraryStatus, LiveEvent, Spoken } from "./api";

const libraryState = vi.hoisted(() => ({
  handler: null as ((status: LibraryStatus) => void) | null,
  unlisten: vi.fn<() => void>(),
}));

const api = vi.hoisted(() => ({
  voiceLiveStart: vi.fn<(conversationId?: string | null) => Promise<void>>(),
  voiceLiveChat: vi.fn<(conversationId?: string | null) => Promise<void>>(),
  voiceLiveStop: vi.fn<() => Promise<void>>(),
  voiceLivePush: vi.fn<(block: string) => Promise<LiveEvent[]>>(),
  voiceLiveListen: vi.fn<(playing: boolean) => Promise<void>>(),
  voiceLivePlaying: vi.fn<(playing: boolean) => Promise<void>>(),
  voiceLiveFinish: vi.fn<() => Promise<void>>(),
  voiceLiveHear: vi.fn<(conversationId?: string | null) => Promise<HeardWords>>(),
  voiceReplyFeed: vi.fn<(delta: string) => Promise<string[]>>(),
  voiceReplyFinish: vi.fn<(whole: string) => Promise<string[]>>(),
  voiceReplyHush: vi.fn<() => Promise<void>>(),
  voiceSpeak: vi.fn<(text: string, conversationId?: string | null) => Promise<Spoken>>(),
  transcribe: vi.fn(),
  onModelLibraryEvent: vi.fn(async (cb: (status: LibraryStatus) => void) => {
    libraryState.handler = cb;
    return libraryState.unlisten;
  }),
}));
vi.mock("./api", () => api);

import {
  BLOCK,
  captionLine,
  PAUSED_NOTE,
  QUIET_NOTE,
  toPcm16,
  VoiceButton,
  VoiceCaption,
  VoiceLine,
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
  /** Every live caption, "" when one was cleared. */
  captions: string[];
  /** Every voice line, "" when it was cleared. */
  lines: string[];
  played: string[];
  /** What each piece's loudness went to. */
  talks: unknown[];
  ends: (() => void)[];
  stops: number;
  closed: number;
  /** The mic hears `events` in reply to the next block. */
  hear: (...events: LiveEvent[]) => Promise<void>;
}

/** Every session a test made, stopped after it: voice mode is on in one
 * session at a time, so a session left on would change the next test. */
const made: VoiceSession[] = [];
afterEach(async () => {
  for (const s of made.splice(0)) await s.stop();
});

function rig(): Rig {
  const r = { states: [], notes: [], sent: [], captions: [], lines: [], played: [], talks: [], ends: [], stops: 0, closed: 0 } as unknown as Rig;
  let take: ((s: Float32Array, rate: number) => void) | null = null;
  r.session = new VoiceSession({
    openMic: async (t) => {
      take = t;
      return () => r.closed++;
    },
    play: (spoken, talk): Playing => {
      r.played.push(spoken.audio);
      r.talks.push(talk);
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
    onCaption: (c) => r.captions.push(c),
    onLine: (l) => r.lines.push(l),
  });
  made.push(r.session);
  r.hear = async (...events) => {
    api.voiceLivePush.mockResolvedValueOnce(events);
    take!(new Float32Array(BLOCK), 16000);
    await flush();
  };
  return r;
}

beforeEach(() => {
  vi.clearAllMocks();
  libraryState.handler = null;
  api.onModelLibraryEvent.mockImplementation(async (cb) => {
    libraryState.handler = cb;
    return libraryState.unlisten;
  });
  for (const fn of [
    api.voiceLiveStart,
    api.voiceLiveChat,
    api.voiceLiveStop,
    api.voiceLiveListen,
    api.voiceLivePlaying,
    api.voiceLiveFinish,
    api.voiceReplyHush,
  ])
    fn.mockResolvedValue(undefined);
  api.voiceLivePush.mockResolvedValue([]);
  api.voiceLiveHear.mockResolvedValue({ words: "what's on tonight", line: null });
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

  it("hands each piece to where its loudness goes, once the chat says where", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValueOnce(["One."]);
    r.session.feed("One. ");
    await flush();
    const talk = () => {};
    r.session.talkTo(talk);
    api.voiceReplyFinish.mockResolvedValueOnce(["Two."]);
    r.session.replyDone("");
    await flush();
    r.ends[0]();
    await flush();
    expect(r.played).toEqual(["One.", "Two."]);
    expect(r.talks).toEqual([undefined, talk]);
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
    api.voiceLiveHear.mockResolvedValueOnce({ words: "  ", line: null });
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

  it("a piece that fails to speak quiets only that reply; the next reply and the next session speak", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceSpeak.mockRejectedValueOnce(new Error("Text to speech failed (HTTP 503)."));
    api.voiceReplyFinish.mockResolvedValueOnce(["One.", "Two."]);
    r.session.replyDone("One. Two.");
    await flush();
    expect(r.notes).toEqual(["Text to speech failed (HTTP 503)."]);
    // "Two." was already on its way when "One." failed.
    expect(r.played).toEqual(["Two."]);
    r.ends[0]();
    await flush();
    expect(r.states.at(-1)).toBe("listening");
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFinish.mockResolvedValueOnce(["Three."]);
    r.session.replyDone("Three.");
    await flush();
    expect(r.played.at(-1)).toBe("Three.");
    r.ends.at(-1)!();
    await flush();
    // Off and on again: a fresh session says a new failure again.
    await r.session.stop();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceSpeak.mockRejectedValueOnce(new Error("The on-device voice timed out."));
    api.voiceReplyFinish.mockResolvedValueOnce(["Four."]);
    r.session.replyDone("Four.");
    await flush();
    expect(r.notes).toEqual(["Text to speech failed (HTTP 503).", "The on-device voice timed out."]);
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
    made.push(s);
    await expect(s.start()).rejects.toThrow("No microphone found.");
    expect(s.state).toBe("off");
    expect(api.voiceLiveStop).toHaveBeenCalled();
  });
});

describe("voice mode in two chats", () => {
  it("sends nothing more of a reply once voice mode is off in that chat", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValue(["Sure."]);
    r.session.feed("Sure. ");
    r.session.replyDone("");
    await r.session.stop();
    await flush();
    expect(api.voiceReplyFeed).not.toHaveBeenCalled();
    expect(api.voiceReplyFinish).not.toHaveBeenCalled();
    expect(api.voiceSpeak).not.toHaveBeenCalled();
  });

  it("turns off in one chat before it starts in another, since the backend has one mic", async () => {
    const order: string[] = [];
    api.voiceLiveStart.mockImplementation(async () => void order.push("start"));
    api.voiceLiveStop.mockImplementation(async () => void order.push("stop"));
    const main = rig();
    const code = rig();
    await main.session.start();
    await code.session.start();
    expect(main.session.state).toBe("off");
    expect(main.states.at(-1)).toBe("off");
    expect(main.closed).toBe(1);
    expect(code.session.state).toBe("listening");
    expect(order).toEqual(["start", "stop", "start"]);
    // Off in the chat it moved to: nothing else is stopped.
    await code.session.stop();
    await main.session.start();
    expect(order).toEqual(["start", "stop", "start", "stop", "start"]);
    await main.session.stop();
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

describe("live captions", () => {
  it("shows live captions while they talk and clears them when the turn is heard", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "caption", text: "remind me" });
    expect(r.captions).toEqual(["remind me"]);
    api.voiceLiveHear.mockResolvedValueOnce({ words: "Remind me.", line: null });
    await r.hear({ kind: "turn", ms: 1200 });
    expect(r.sent).toEqual(["Remind me."]);
    expect(r.captions).toEqual(["remind me", ""]);
  });

  it("clears the caption when the mic heard only noise", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "caption", text: "uh" }, { kind: "nothing" });
    expect(r.captions).toEqual(["uh", ""]);
  });

  it("a reply that starts while they talk takes the caption with it", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "started" }, { kind: "caption", text: "remind me to call" });
    // A queued message's reply speaks over them.
    api.voiceReplyFeed.mockResolvedValueOnce(["Done."]);
    r.session.feed("Done. ");
    await flush();
    expect(r.states.at(-1)).toBe("speaking");
    expect(r.captions).toEqual(["remind me to call", ""]);
    // It ends before their turn does: no old caption comes back.
    r.session.replyDone("");
    await flush();
    r.ends[0]();
    await flush();
    expect(r.states.at(-1)).toBe("listening");
    expect(r.captions).toEqual(["remind me to call", ""]);
  });

  it("a caption still on its way when a reply starts never shows", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "started" });
    api.voiceReplyFeed.mockResolvedValueOnce(["Done."]);
    r.session.feed("Done. ");
    await flush();
    expect(r.states.at(-1)).toBe("speaking");
    // The answer to a block sent just before the reply started.
    await r.hear({ kind: "caption", text: "remind me to call" });
    // They talk over the reply: listening again, with no old caption.
    await r.hear({ kind: "started" });
    expect(r.states.at(-1)).toBe("listening");
    expect(r.captions).toEqual([]);
  });

  it("stopping clears the caption", async () => {
    const r = rig();
    await r.session.start();
    await r.hear({ kind: "caption", text: "remind me" });
    await r.session.stop();
    expect(r.captions.at(-1)).toBe("");
  });

  it("a long caption shows only its end", () => {
    expect(captionLine("hi there")).toBe("“hi there”");
    const long = captionLine("word ".repeat(40) + "end");
    expect(long.startsWith("“…")).toBe(true);
    expect(long.endsWith("word end”")).toBe(true);
    expect(Array.from(long)).toHaveLength(83);
    // Characters, not UTF-16 units: an emoji is never cut in half.
    expect(captionLine("🎉".repeat(81))).toBe(`“…${"🎉".repeat(80)}”`);
  });

  it("the caption shows only while listening", () => {
    const { rerender } = render(<VoiceCaption text="remind me" state="listening" />);
    expect(screen.getByText("“remind me”")).toHaveAttribute("aria-live", "off");
    rerender(<VoiceCaption text="remind me" state="thinking" />);
    expect(screen.queryByText("“remind me”")).toBeNull();
    rerender(<VoiceCaption text="" state="listening" />);
    expect(document.querySelector(".voice-caption")).toBeNull();
  });
});

describe("voice model downloads", () => {
  it("reports voice model download progress without spamming and unlistens on stop", async () => {
    const r = rig();
    await r.session.start();
    await flush();
    expect(api.onModelLibraryEvent).toHaveBeenCalledTimes(1);

    libraryState.handler?.({
      job: { id: "parakeet-tdt-0.6b-v3-int8", line: "Downloading Parakeet: 10%", pct: 10 },
      queued: 0,
      message: null,
      errors: {},
      running: true,
    });
    libraryState.handler?.({
      job: { id: "parakeet-tdt-0.6b-v3-int8", line: "Downloading Parakeet: 60%", pct: 60 },
      queued: 0,
      message: null,
      errors: {},
      running: true,
    });
    libraryState.handler?.({
      job: null,
      queued: 0,
      message: "Installed Parakeet speech model.",
      errors: {},
      running: false,
    });

    expect(r.notes).toEqual([
      "Downloading Parakeet: 10%",
      "Installed Parakeet speech model.",
    ]);

    await r.session.stop();
    expect(libraryState.unlisten).toHaveBeenCalledTimes(1);
  });
});

describe("the voice by need", () => {
  const KOKORO = "Spoken on this computer with Kokoro text to speech: this chat runs here.";
  const PARAKEET = "Heard on this computer with Parakeet v3 speech to text: this chat runs here.";

  it("starts, hears and speaks for the chat on screen, and shows the on-device line", async () => {
    const r = rig();
    r.session.forChat("chat-here");
    await r.session.start();
    expect(api.voiceLiveStart).toHaveBeenCalledWith("chat-here");
    api.voiceLiveHear.mockResolvedValueOnce({ words: "what's on tonight", line: PARAKEET });
    await r.hear({ kind: "turn", ms: 1500 });
    expect(api.voiceLiveHear).toHaveBeenCalledWith("chat-here");
    expect(r.sent).toEqual(["what's on tonight"]);
    expect(r.lines).toEqual([PARAKEET]);
    api.voiceSpeak.mockImplementationOnce(async (text) => ({ mime: "audio/wav", audio: text, line: KOKORO }));
    api.voiceReplyFeed.mockResolvedValueOnce(["Nothing much."]);
    r.session.feed("Nothing much. ");
    await flush();
    expect(api.voiceSpeak).toHaveBeenCalledWith("Nothing much.", "chat-here");
    expect(r.played).toEqual(["Nothing much."]);
    expect(r.lines).toEqual([PARAKEET, KOKORO]);
    // Voice mode off: the line goes with it.
    await r.session.stop();
    expect(r.lines).toEqual([PARAKEET, KOKORO, ""]);
  });

  it("a plain cloud chat shows no line", async () => {
    const r = rig();
    r.session.forChat("chat-online");
    await r.session.start();
    await r.hear({ kind: "turn", ms: 1500 });
    api.voiceReplyFeed.mockResolvedValueOnce(["Sure."]);
    r.session.feed("Sure. ");
    await flush();
    expect(r.played).toEqual(["Sure."]);
    expect(r.lines).toEqual([]);
  });

  it("tells the backend when voice mode moves to another chat, only while it is on", async () => {
    const r = rig();
    r.session.forChat("chat-a");
    expect(api.voiceLiveChat).not.toHaveBeenCalled();
    await r.session.start();
    r.session.forChat("chat-a");
    expect(api.voiceLiveChat).not.toHaveBeenCalled();
    r.session.forChat("chat-b");
    expect(api.voiceLiveChat).toHaveBeenCalledWith("chat-b");
    await r.hear({ kind: "turn", ms: 900 });
    expect(api.voiceLiveHear).toHaveBeenLastCalledWith("chat-b");
  });

  it("a chat opened while voice mode starts is the one the backend hears for", async () => {
    const r = rig();
    r.session.forChat("chat-a");
    let started = () => {};
    api.voiceLiveStart.mockImplementationOnce(() => new Promise<void>((res) => (started = res)));
    const on = r.session.start();
    await flush();
    r.session.forChat("chat-b");
    started();
    await on;
    r.session.forChat("chat-b");
    expect(api.voiceLiveStart).toHaveBeenCalledWith("chat-a");
    expect(api.voiceLiveChat.mock.calls).toEqual([["chat-b"]]);
  });

  it("a cloud turn after a local one, or another chat, clears the line", async () => {
    const r = rig();
    r.session.forChat("chat-here");
    await r.session.start();
    api.voiceLiveHear.mockResolvedValueOnce({ words: "what's on tonight", line: PARAKEET });
    await r.hear({ kind: "turn", ms: 1500 });
    expect(r.lines).toEqual([PARAKEET]);
    // The next step went online: xAI heard this turn.
    api.voiceLiveHear.mockResolvedValueOnce({ words: "and tomorrow", line: null });
    await r.hear({ kind: "turn", ms: 900 });
    expect(r.lines).toEqual([PARAKEET, ""]);
    // A piece spoken here, then voice mode follows a cloud chat.
    api.voiceSpeak.mockImplementationOnce(async (text) => ({ mime: "audio/wav", audio: text, line: KOKORO }));
    api.voiceReplyFeed.mockResolvedValueOnce(["Rain."]);
    r.session.feed("Rain. ");
    await flush();
    expect(r.lines).toEqual([PARAKEET, "", KOKORO]);
    r.session.forChat("chat-online");
    expect(r.lines).toEqual([PARAKEET, "", KOKORO, ""]);
  });

  it("a turn heard after voice mode went off shows no line, then or next time", async () => {
    const r = rig();
    r.session.forChat("chat-here");
    await r.session.start();
    let heard = (_: { words: string; line: string | null }) => {};
    api.voiceLiveHear.mockImplementationOnce(() => new Promise((res) => (heard = res)));
    await r.hear({ kind: "turn", ms: 1500 });
    await r.session.stop();
    heard({ words: "what's on tonight", line: PARAKEET });
    await flush();
    expect(r.lines).toEqual([]);
    expect(r.sent).toEqual([]);
    await r.session.start();
    expect(r.lines).toEqual([]);
  });

  it("the end of a reply is spoken for its own chat after another chat opens", async () => {
    const r = rig();
    r.session.forChat("chat-here");
    await r.session.start();
    api.voiceReplyFinish.mockResolvedValueOnce(["See you."]);
    r.session.replyDone("");
    // The chat picked while the reply wrapped up opens now.
    r.session.forChat("chat-online");
    await flush();
    expect(api.voiceSpeak).toHaveBeenCalledWith("See you.", "chat-here");
    expect(api.voiceSpeak).not.toHaveBeenCalledWith("See you.", "chat-online");
  });

  it("the line shows on its own, whatever voice mode is doing", () => {
    const { rerender } = render(<VoiceLine text={KOKORO} />);
    expect(screen.getByText(KOKORO)).toHaveClass("voice-line");
    rerender(<VoiceLine text="" />);
    expect(document.querySelector(".voice-line")).toBeNull();
  });
});
