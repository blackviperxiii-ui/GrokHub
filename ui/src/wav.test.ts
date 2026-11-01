import { describe, expect, it } from "vitest";
import { downsample, encodeWav, toBase64, WAV_RATE } from "./wav";

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

  it("downsamples 48 kHz to 16 kHz by averaging each three samples", () => {
    const out = downsample(new Float32Array([0.3, 0.3, 0.3, -0.6, 0, 0, 1, 1]), 48000);
    expect(Array.from(out).map((x) => Math.round(x * 100) / 100)).toEqual([0.3, -0.2]);
    expect(WAV_RATE).toBe(16000);
    const same = new Float32Array([0.1, 0.2]);
    expect(downsample(same, 16000)).toBe(same);
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
