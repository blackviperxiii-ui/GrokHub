import { afterEach, describe, expect, it, vi } from "vitest";
import { envelopeFromPcm, envelopeOf, smoothLevelAt, talkHop } from "./lipsync";

function tone(rate: number, secs: number, amp: number): number[] {
  return Array.from({ length: Math.floor(rate * secs) }, (_, i) => amp * Math.sin((i * 440 * 2 * Math.PI) / rate));
}

describe("the envelope of a spoken reply", () => {
  it("has one level per 20 ms, scaled to the loud part", () => {
    const rate = 16_000;
    // 200 ms silence, 400 ms loud, 200 ms quiet.
    const env = envelopeFromPcm([...new Array<number>(3200).fill(0), ...tone(rate, 0.4, 0.8), ...tone(rate, 0.2, 0.2)], rate);
    expect(env).toHaveLength(40);
    expect(env[5]).toBe(0);
    expect(env[20]).toBeGreaterThanOrEqual(0.9);
    expect(env[20]).toBeLessThanOrEqual(1);
    expect(env[35]).toBeCloseTo(0.25, 1);
  });

  it("scales to the 95th-percentile frame, so a shout clips instead of muting the rest", () => {
    const rate = 1000; // 20 samples a frame
    const frames = (n: number, amp: number) => Array.from({ length: n }, () => new Array<number>(20).fill(amp)).flat();
    // 100 frames, quietest to loudest: the 95th is one of the six at 0.4.
    // A median or 90th-percentile scale would read 0.2, the loudest 0.8.
    const env = envelopeFromPcm([...frames(1, 0.1), ...frames(92, 0.2), ...frames(6, 0.4), ...frames(1, 0.8)], rate);
    expect(env).toHaveLength(100);
    expect(env[0]).toBeCloseTo(0.25, 6);
    expect(env[50]).toBeCloseTo(0.5, 6);
    expect(env[95]).toBe(1);
    // The shout clips.
    expect(env[99]).toBe(1);
  });

  it("is all zeros for silence and empty for nothing", () => {
    expect(envelopeFromPcm(new Array<number>(16_000).fill(0), 16_000)).toEqual(new Array<number>(50).fill(0));
    expect(envelopeFromPcm([], 16_000)).toEqual([]);
  });
});

describe("the smoothed level", () => {
  // 100 ms silence, 200 ms full voice, then silence.
  const levels = [...new Array<number>(5).fill(0), ...new Array<number>(10).fill(1), ...new Array<number>(40).fill(0)];

  it("rises fast and falls slowly", () => {
    expect(smoothLevelAt(levels, 80)).toBe(0);
    // One frame in: 1 - e^-1.
    expect(smoothLevelAt(levels, 100)).toBeCloseTo(0.6321, 4);
    expect(smoothLevelAt(levels, 280)).toBeGreaterThan(0.99);
    // 60 ms after the voice stops it is still letting go: about e^-0.5.
    expect(smoothLevelAt(levels, 340)).toBeCloseTo(0.6065, 2);
    expect(smoothLevelAt(levels, 1000)).toBe(0);
  });

  it("is 0 before the start, for no clip, and for a time that isn't one", () => {
    expect(smoothLevelAt(levels, -40)).toBe(0);
    expect(smoothLevelAt([], 0)).toBe(0);
    expect(smoothLevelAt(levels, Number.NaN)).toBe(0);
  });
});

describe("her talk hop", () => {
  it("is 1.5% of her height at full voice, and none under reduce motion", () => {
    expect(talkHop(400, 0, false)).toBe(0);
    expect(talkHop(400, 1, false)).toBe(6);
    expect(talkHop(400, 0.5, false)).toBe(3);
    expect(talkHop(400, 2, false)).toBe(6);
    expect(talkHop(400, 1, true)).toBe(0);
  });
});

describe("envelopeOf", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("decodes the piece, mixes its channels and rounds each level", async () => {
    // Three 20 ms frames (44.1 kHz): the left channel loud in the first,
    // the right in the second, both in the third.
    const f = (amp: number) => new Array<number>(882).fill(amp);
    const left = new Float32Array([...f(0.6), ...f(0), ...f(0.6)]);
    const right = new Float32Array([...f(0), ...f(0.2), ...f(0.6)]);
    const decoded: Uint8Array[] = [];
    vi.stubGlobal(
      "OfflineAudioContext",
      class {
        decodeAudioData(buf: ArrayBuffer) {
          decoded.push(new Uint8Array(buf));
          return Promise.resolve({
            length: 2646,
            numberOfChannels: 2,
            sampleRate: 44_100,
            getChannelData: (ch: number) => (ch === 0 ? left : right),
          });
        }
      },
    );
    // "hi" in base64.
    // Mixed: 0.3, 0.1 and 0.6; scaled to the loudest and rounded.
    expect(await envelopeOf({ mime: "audio/mpeg", audio: "aGk=" })).toEqual([0.5, 0.17, 1]);
    expect([...decoded[0]]).toEqual([104, 105]);
  });

  it("is null when the webview can't decode it", async () => {
    vi.stubGlobal(
      "OfflineAudioContext",
      class {
        decodeAudioData() {
          return Promise.reject(new Error("EncodingError"));
        }
      },
    );
    expect(await envelopeOf({ mime: "audio/mpeg", audio: "aGk=" })).toBeNull();
    vi.stubGlobal("OfflineAudioContext", undefined);
    expect(await envelopeOf({ mime: "audio/mpeg", audio: "aGk=" })).toBeNull();
  });
});
