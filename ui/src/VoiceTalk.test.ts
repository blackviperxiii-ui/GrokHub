import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopNyxTalk } from "./desktopNyxApi";

// A spoken piece tells her desk its loudness and when it began each time it
// plays, and null when it pauses or ends.
const lips = vi.hoisted(() => ({ envelopeOf: vi.fn<() => Promise<number[] | null>>() }));
vi.mock("./lipsync", () => lips);

import { playSpoken } from "./VoiceMode";

class FakeAudio {
  static last: FakeAudio | null = null;
  paused = true;
  currentTime = 0;
  onplaying: (() => void) | null = null;
  onpause: (() => void) | null = null;
  onended: (() => void) | null = null;
  onerror: (() => void) | null = null;
  readonly src: string;
  constructor(src: string) {
    this.src = src;
    FakeAudio.last = this;
  }
  play() {
    this.paused = false;
    this.onplaying?.();
    return Promise.resolve();
  }
  pause() {
    if (this.paused) return;
    this.paused = true;
    this.onpause?.();
  }
  end() {
    this.pause();
    this.onended?.();
  }
}

const flush = () => new Promise((r) => setTimeout(r, 0));

describe("playSpoken and her talk hop", () => {
  let talks: (DesktopNyxTalk | null)[];
  const spoken = { mime: "audio/mpeg", audio: "aGk=" };
  beforeEach(() => {
    talks = [];
    vi.stubGlobal("Audio", FakeAudio);
    vi.spyOn(Date, "now").mockReturnValue(50_000);
    lips.envelopeOf.mockReset().mockResolvedValue([0.2, 1, 0.5]);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("sends the envelope when it plays, again from where it resumes, and null when it pauses or ends", async () => {
    const playing = playSpoken(spoken, (t) => talks.push(t));
    await flush();
    expect(talks).toEqual([{ levels: [0.2, 1, 0.5], startedAt: 50_000 }]);
    const audio = FakeAudio.last!;
    audio.pause();
    expect(talks.at(-1)).toBeNull();
    // Resumed 1.5 s in: it began 1.5 s before now.
    audio.currentTime = 1.5;
    audio.play();
    await flush();
    expect(talks.at(-1)).toEqual({ levels: [0.2, 1, 0.5], startedAt: 48_500 });
    audio.end();
    await playing.done;
    await flush();
    expect(talks).toHaveLength(4);
    expect(talks.at(-1)).toBeNull();
    // The piece is decoded once.
    expect(lips.envelopeOf).toHaveBeenCalledTimes(1);
  });

  it("sends nothing for a piece it can't decode, or one stopped before its envelope is ready", async () => {
    lips.envelopeOf.mockResolvedValue(null);
    const quiet = playSpoken(spoken, (t) => talks.push(t));
    await flush();
    FakeAudio.last!.end();
    await quiet.done;
    await flush();
    expect(talks).toEqual([]);

    lips.envelopeOf.mockResolvedValue([1]);
    const stopped = playSpoken(spoken, (t) => talks.push(t));
    stopped.stop();
    await stopped.done;
    await flush();
    expect(talks).toEqual([]);
  });

  it("decodes nothing without a listener", async () => {
    const playing = playSpoken(spoken);
    await flush();
    FakeAudio.last!.end();
    await playing.done;
    expect(lips.envelopeOf).not.toHaveBeenCalled();
  });
});
