// Push to talk (LO-17j): the mic's samples become a 16 kHz mono 16-bit WAV,
// the shape xAI speech-to-text and the cabin use.

/** The rate the recording is sent at. */
export const WAV_RATE = 16000;

/** Where the low-pass cuts, as a share of the new rate: just under its
 * Nyquist, so speech keeps its sibilants and nothing folds back. */
const CUTOFF = 0.42;
/** Sinc zero crossings kept on each side of the filter's center. */
const ZEROS = 12;
/** Filter table points per input sample. */
const STEPS = 256;

/** A Blackman-windowed sinc low-pass for `fromRate` → `toRate`, tabulated
 * from its center out to `half` input samples. */
function lowPass(fromRate: number, toRate: number) {
  const fc = (CUTOFF * toRate) / fromRate;
  const half = ZEROS / (2 * fc);
  const table = new Float32Array(Math.ceil(half * STEPS) + 2);
  for (let i = 0; i < table.length; i++) {
    const t = i / STEPS;
    if (t > half) break;
    const x = 2 * fc * t;
    const sinc = x === 0 ? 1 : Math.sin(Math.PI * x) / (Math.PI * x);
    const w = 0.42 + 0.5 * Math.cos((Math.PI * t) / half) + 0.08 * Math.cos((2 * Math.PI * t) / half);
    table[i] = sinc * w;
  }
  return { half, table };
}

/** The filtered sample at input position `at`, from `x` whose first sample
 * is input index `first`. Samples outside `x` are left out (and the weights
 * renormalized), so a level holds right up to the edges. */
function tapAt(x: Float32Array, first: number, at: number, half: number, table: Float32Array): number {
  const lo = Math.max(first, Math.ceil(at - half));
  const hi = Math.min(first + x.length - 1, Math.floor(at + half));
  let sum = 0;
  let weight = 0;
  for (let k = lo; k <= hi; k++) {
    const pos = Math.abs(at - k) * STEPS;
    const i = Math.floor(pos);
    const h = table[i] + (table[i + 1] - table[i]) * (pos - i);
    sum += x[k - first] * h;
    weight += h;
  }
  return weight > 0 ? sum / weight : 0;
}

/** `input` (at `fromRate`) at `toRate`, low-passed first so sounds above the
 * new Nyquist are removed instead of folding back into speech. A rate at or
 * below the target is kept as it is. */
export function downsample(input: Float32Array, fromRate: number, toRate = WAV_RATE): Float32Array {
  if (fromRate <= toRate) return input;
  const { half, table } = lowPass(fromRate, toRate);
  const ratio = fromRate / toRate;
  const out = new Float32Array(Math.floor(input.length / ratio));
  for (let n = 0; n < out.length; n++) out[n] = tapAt(input, 0, n * ratio, half, table);
  return out;
}

/** [`downsample`] for a stream of blocks: the filter and the position carry
 * over from block to block, so joining the outputs matches downsampling the
 * whole sound at once (no sample dropped at a block edge). */
export class Downsampler {
  readonly fromRate: number;
  readonly toRate: number;
  private readonly half: number;
  private readonly table: Float32Array;
  private readonly ratio: number;
  /** Input not yet behind the filter, starting at input index `first`. */
  private held = new Float32Array(0);
  private first = 0;
  /** Input position of the next output sample. */
  private next = 0;

  constructor(fromRate: number, toRate = WAV_RATE) {
    this.fromRate = fromRate;
    this.toRate = toRate;
    ({ half: this.half, table: this.table } = lowPass(Math.max(fromRate, toRate), toRate));
    this.ratio = fromRate / toRate;
  }

  /** The output this block completes. The last few input samples wait for
   * the next block (under a millisecond). */
  push(input: Float32Array): Float32Array {
    if (this.fromRate <= this.toRate) return input;
    const x = new Float32Array(this.held.length + input.length);
    x.set(this.held);
    x.set(input, this.held.length);
    const end = this.first + x.length;
    const out: number[] = [];
    while (this.next + this.half < end) {
      out.push(tapAt(x, this.first, this.next, this.half, this.table));
      this.next += this.ratio;
    }
    const keep = Math.max(this.first, Math.floor(this.next - this.half));
    this.held = x.slice(keep - this.first);
    this.first = keep;
    return Float32Array.from(out);
  }
}

/** A PCM WAV: the 44-byte header, then each sample clamped to 16 bits. */
export function encodeWav(samples: Float32Array, rate = WAV_RATE): Uint8Array {
  const bytes = new Uint8Array(44 + samples.length * 2);
  const view = new DataView(bytes.buffer);
  const text = (at: number, s: string) => {
    for (let i = 0; i < s.length; i++) view.setUint8(at + i, s.charCodeAt(i));
  };
  text(0, "RIFF");
  view.setUint32(4, 36 + samples.length * 2, true);
  text(8, "WAVE");
  text(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true); // byte rate
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  text(36, "data");
  view.setUint32(40, samples.length * 2, true);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(44 + i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return bytes;
}

/** Bytes as base64, in chunks so a minute of sound doesn't overflow the stack. */
export function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}
