// Her talk hop on the desk follows the spoken reply's loudness: the reply is
// decoded into one level per 20 ms before it plays, and her desk reads the
// level at the time since playback began (ported from the cabin's lip-sync).
import type { Spoken } from "./api";

/** One envelope frame. */
export const FRAME_MS = 20;
/** Frames louder than this share of the clip clip to the top, so one shout
 * doesn't make the rest of the sentence mumble. */
const LOUD_SHARE = 0.95;
/** The smoothed level follows a rising voice within about RISE_MS and lets
 * a falling one go over about FALL_MS, so 20 ms frames don't make her
 * jitter. */
export const RISE_MS = 20;
export const FALL_MS = 120;
/** How many frames back the smoothing looks; older frames have faded out. */
export const SMOOTH_FRAMES = 16;
/** At full voice she hops up by this share of her height. */
export const TALK_HOP = 0.015;

/** Loudness per FRAME_MS, 0 to 1, of mono PCM at `rate`: RMS per frame,
 * scaled so the 95th-percentile frame is 1 and louder frames clip to 1.
 * Silence, or an empty clip, is all zeros. */
export function envelopeFromPcm(samples: ArrayLike<number>, rate: number): number[] {
  const perFrame = Math.max(1, Math.floor((rate * FRAME_MS) / 1000));
  const levels: number[] = [];
  for (let from = 0; from < samples.length; from += perFrame) {
    const to = Math.min(samples.length, from + perFrame);
    let sum = 0;
    for (let i = from; i < to; i++) sum += samples[i] * samples[i];
    levels.push(Math.sqrt(sum / (to - from)));
  }
  const sorted = [...levels].sort((a, b) => a - b);
  const reference = sorted[Math.min(Math.floor(sorted.length * LOUD_SHARE), sorted.length - 1)] ?? 0;
  if (reference <= 1e-6) return levels.map(() => 0);
  return levels.map((l) => Math.min(l / reference, 1));
}

/** Loudness `atMs` into the clip, smoothed: a fast rise and a slow fall
 * over the last SMOOTH_FRAMES frames. 0 before the start and well after
 * the end. */
export function smoothLevelAt(levels: readonly number[], atMs: number): number {
  if (!(atMs >= 0)) return 0;
  const rise = 1 - Math.exp(-FRAME_MS / RISE_MS);
  const fall = 1 - Math.exp(-FRAME_MS / FALL_MS);
  const now = Math.floor(atMs / FRAME_MS);
  let y = 0;
  for (let i = Math.max(0, now - SMOOTH_FRAMES); i <= now; i++) {
    const x = levels[i] ?? 0;
    y += (x - y) * (x > y ? rise : fall);
  }
  return y;
}

/** How far she hops up while talking, in px, for her height and the
 * smoothed level; none under reduce motion. */
export function talkHop(height: number, level: number, still: boolean): number {
  if (still) return 0;
  return Math.min(Math.max(level, 0), 1) * TALK_HOP * height;
}

/** A spoken piece's envelope, levels to two places; null when this webview
 * can't decode it. */
export async function envelopeOf(spoken: Spoken): Promise<number[] | null> {
  const Ctx = globalThis.OfflineAudioContext;
  if (!Ctx) return null;
  try {
    const bytes = Uint8Array.from(atob(spoken.audio), (c) => c.charCodeAt(0));
    const clip = await new Ctx(1, 1, 44_100).decodeAudioData(bytes.buffer);
    const mono = new Float32Array(clip.length);
    for (let ch = 0; ch < clip.numberOfChannels; ch++) {
      const data = clip.getChannelData(ch);
      for (let i = 0; i < mono.length; i++) mono[i] += data[i] / clip.numberOfChannels;
    }
    return envelopeFromPcm(mono, clip.sampleRate).map((l) => Math.round(l * 100) / 100);
  } catch {
    return null;
  }
}
