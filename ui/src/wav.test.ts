import { describe, expect, it } from "vitest";
import { Downsampler, downsample, encodeWav, toBase64, WAV_RATE } from "./wav";

describe("wav", () => {
  it("writes a 16 kHz mono 16-bit PCM header and every sample", () => {
    const wav = encodeWav(new Float32Array([0, 1, -1, 0.5]));
    const view = new DataView(wav.buffer);
    const text = (at: number) => String.fromCharCode(...wav.subarray(at, at + 4));
    expect(wav.length).toBe(52);
    expect([text(0), text(8), text(12), text(36)]).toEqual(["RIFF", "WAVE", "fmt ", "data"]);
    expect(view.getUint32(4, true)).toBe(44);
    expect(view.getUint16(20, true)).toBe(1);
    expect(view.getUint16(22, true)).toBe(1);
    expect(view.getUint32(24, true)).toBe(16000);
    expect(view.getUint32(28, true)).toBe(32000);
    expect(view.getUint16(32, true)).toBe(2);
    expect(view.getUint16(34, true)).toBe(16);
    expect(view.getUint32(40, true)).toBe(8);
    expect([0, 1, 2, 3].map((i) => view.getInt16(44 + i * 2, true))).toEqual([0, 32767, -32768, 16383]);
  });

  it("clamps samples past full scale", () => {
    const view = new DataView(encodeWav(new Float32Array([2, -3])).buffer);
    expect([view.getInt16(44, true), view.getInt16(46, true)]).toEqual([32767, -32768]);
  });

  /** RMS of `x` after its first and last `edge` samples. */
  const rms = (x: Float32Array, edge = 64) => {
    const mid = x.subarray(edge, x.length - edge);
    return Math.sqrt(mid.reduce((n, v) => n + v * v, 0) / mid.length);
  };
  const tone = (hz: number, rate: number, seconds = 0.5) =>
    Float32Array.from({ length: rate * seconds }, (_, i) => 0.5 * Math.sin((2 * Math.PI * hz * i) / rate));

  it("keeps speech and a steady level when it goes down to 16 kHz", () => {
    expect(WAV_RATE).toBe(16000);
    for (const rate of [48000, 44100]) {
      const voice = downsample(tone(1000, rate), rate);
      expect(voice.length).toBe(8000);
      // A 0.5 sine is 0.354 RMS; the speech band passes within 1%.
      expect(rms(voice)).toBeCloseTo(0.354, 2);
      const level = downsample(new Float32Array(rate / 10).fill(0.3), rate);
      expect(Math.min(...level)).toBeCloseTo(0.3, 4);
      expect(Math.max(...level)).toBeCloseTo(0.3, 4);
    }
    const same = new Float32Array([0.1, 0.2]);
    expect(downsample(same, 16000)).toBe(same);
  });

  it("removes sound above 8 kHz instead of folding it back into speech", () => {
    for (const rate of [48000, 44100]) {
      for (const hz of [10000, 12000, 15000]) {
        // At least 40 dB down (a plain average let a 10 kHz tone through at -6 dB).
        expect(rms(downsample(tone(hz, rate), rate))).toBeLessThan(0.354 / 100);
      }
    }
  });

  it("a stream in blocks matches the whole sound, with no sample lost at a block edge", () => {
    for (const rate of [48000, 44100]) {
      for (const size of [128, 2048, 4096]) {
        const sound = tone(440, rate, 1);
        const whole = downsample(sound, rate);
        const stream = new Downsampler(rate);
        const parts: number[] = [];
        for (let at = 0; at < sound.length; at += size) parts.push(...stream.push(sound.subarray(at, at + size)));
        // Only the last few samples wait for a next block.
        expect(whole.length - parts.length).toBeLessThanOrEqual(15);
        expect(whole.length - parts.length).toBeGreaterThanOrEqual(0);
        let worst = 0;
        for (let i = 0; i < parts.length; i++) worst = Math.max(worst, Math.abs(parts[i] - whole[i]));
        expect(worst).toBeLessThan(1e-6);
      }
    }
    const same = new Float32Array([0.1, 0.2]);
    expect(new Downsampler(16000).push(same)).toBe(same);
  });

  it("a second of 48 kHz sound is 16000 samples in the WAV", () => {
    const wav = encodeWav(downsample(new Float32Array(48000), 48000));
    expect(new DataView(wav.buffer).getUint32(40, true)).toBe(32000);
    expect(wav.length).toBe(44 + 32000);
  });

  it("base64 matches btoa for a small buffer", () => {
    expect(toBase64(new Uint8Array([104, 105]))).toBe("aGk=");
  });
});
