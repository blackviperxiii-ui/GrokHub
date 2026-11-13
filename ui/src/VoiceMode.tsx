// Voice mode: talk without holding anything and hear the reply. The page
// owns the mic and the speakers; the backend decides when a turn starts and
// ends, transcribes it, cuts the streamed reply into pieces and voices them.

import { useEffect, useState } from "react";
import * as api from "./api";
import type { LiveEvent, Spoken } from "./api";
import { CANT_RECORD, MIC_DENIED, NO_MIC, tap } from "./Mic";
import { Downsampler, toBase64, WAV_RATE } from "./wav";

export type VoiceState = "off" | "listening" | "thinking" | "speaking";

export type VoiceEvent =
  | "start"
  | "stop"
  /** A finished turn went off to be heard. */
  | "turn"
  /** Nothing was said, or the reply had nothing to speak. */
  | "empty"
  /** The first piece of the reply began playing. */
  | "spoke"
  /** They talked over the reply, or tapped it quiet. */
  | "barge"
  /** The reply has been spoken to the end. */
  | "done";

/** Where voice mode goes next. Anything not listed keeps the state. */
export function voiceStep(state: VoiceState, event: VoiceEvent): VoiceState {
  if (event === "stop") return "off";
  switch (state) {
    case "off":
      return event === "start" ? "listening" : "off";
    case "listening":
      return event === "turn" ? "thinking" : event === "spoke" ? "speaking" : "listening";
    case "thinking":
      if (event === "spoke") return "speaking";
      return event === "empty" || event === "done" ? "listening" : "thinking";
    case "speaking":
      if (event === "turn") return "thinking";
      return event === "barge" || event === "done" ? "listening" : "speaking";
  }
}

/** 100 ms at 16 kHz: what the page sends at a time. */
export const BLOCK = WAV_RATE / 10;
export const QUIET_NOTE = "Still listening.";
export const PAUSED_NOTE = "Voice mode paused after a quiet minute.";

/** Float samples as 16-bit little-endian PCM. */
export function toPcm16(samples: Float32Array): Uint8Array {
  const out = new Uint8Array(samples.length * 2);
  const view = new DataView(out.buffer);
  for (let i = 0; i < samples.length; i++) {
    const s = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(i * 2, s < 0 ? s * 0x8000 : s * 0x7fff, true);
  }
  return out;
}

/** A piece being played; `done` settles when it ends or is stopped. */
export interface Playing {
  done: Promise<void>;
  stop: () => void;
}

export interface VoiceDeps {
  /** Opens the mic; samples at `rate` go to `take`. Resolves to its closer. */
  openMic: (take: (samples: Float32Array, rate: number) => void) => Promise<() => void>;
  play: (spoken: Spoken) => Playing;
  /** Sends the heard words as the person's message. */
  send: (words: string) => void;
  onState: (state: VoiceState) => void;
  onNote: (note: string) => void;
}

/** One voice-mode session: mic in, turns out, reply spoken. */
export class VoiceSession {
  state: VoiceState = "off";
  /** Someone is mid-turn. */
  talking = false;
  private closeMic: (() => void) | null = null;
  private pending = new Float32Array(0);
  private pushes: Promise<unknown> = Promise.resolve();
  private reply: Promise<unknown> = Promise.resolve();
  private queue: Promise<Spoken | null>[] = [];
  private player: Playing | null = null;
  private spoke = false;
  private finished = false;
  /** A piece failed to speak: the rest of this reply stays quiet. */
  private noVoice = false;
  /** The failure was shown once this session. */
  private warned = false;
  /** The mic's rate to 16 kHz, carried from block to block. */
  private down: Downsampler | null = null;

  private deps: VoiceDeps;

  constructor(deps: VoiceDeps) {
    this.deps = deps;
  }

  /** Where heard words and notes go from now on. */
  handlers(send: VoiceDeps["send"], onNote: VoiceDeps["onNote"]) {
    this.deps = { ...this.deps, send, onNote };
  }

  private go(event: VoiceEvent) {
    const next = voiceStep(this.state, event);
    if (next === this.state) return;
    this.state = next;
    this.deps.onState(next);
  }

  private note(err: unknown) {
    this.deps.onNote(err instanceof Error ? err.message : String(err));
  }

  async start() {
    if (this.state !== "off") return;
    this.noVoice = false;
    this.warned = false;
    await api.voiceLiveStart();
    this.go("start");
    try {
      this.closeMic = await this.deps.openMic((s, rate) => this.take(s, rate));
    } catch (err) {
      await this.stop();
      throw err;
    }
    if (this.state === "off") this.closeMic?.();
  }

  async stop(note?: string) {
    const wasOn = this.state !== "off";
    this.go("stop");
    this.closeMic?.();
    this.closeMic = null;
    this.hushPlayback();
    this.talking = false;
    this.pending = new Float32Array(0);
    this.down = null;
    if (note) this.deps.onNote(note);
    if (wasOn) await api.voiceLiveStop().catch(() => {});
  }

  /** The one button: done talking, quiet the reply, or voice mode off. */
  async tap() {
    if (this.state === "listening" && this.talking) return api.voiceLiveFinish().catch((e) => this.note(e));
    if (this.state === "speaking") return this.barge();
    return this.stop();
  }

  /** Mic samples at `rate`, sent on in 100 ms blocks, one push at a time. */
  take(samples: Float32Array, rate: number) {
    if (this.state === "off") return;
    if (this.down?.fromRate !== rate) this.down = new Downsampler(rate);
    const down = this.down.push(samples);
    const all = new Float32Array(this.pending.length + down.length);
    all.set(this.pending);
    all.set(down, this.pending.length);
    let at = 0;
    for (; at + BLOCK <= all.length; at += BLOCK) {
      const block = toBase64(toPcm16(all.subarray(at, at + BLOCK)));
      this.pushes = this.pushes
        .then(() => (this.state === "off" ? [] : api.voiceLivePush(block)))
        .then((events) => this.heard(events))
        .catch((e) => this.note(e));
    }
    this.pending = all.slice(at);
  }

  private async heard(events: LiveEvent[]) {
    for (const ev of events) {
      if (this.state === "off") return;
      switch (ev.kind) {
        case "started":
          this.talking = true;
          if (this.state === "speaking") await this.barge();
          break;
        case "turn":
          this.talking = false;
          await this.hear();
          break;
        case "nothing":
          this.talking = false;
          await api.voiceLiveListen(this.state === "speaking");
          break;
        case "check_in":
          this.deps.onNote(QUIET_NOTE);
          break;
        case "pause":
          await this.stop(PAUSED_NOTE);
          break;
      }
    }
  }

  private async hear() {
    this.go("turn");
    const words = (await api.voiceLiveHear().catch((e) => (this.note(e), ""))).trim();
    if (this.state !== "thinking") return;
    if (!words) {
      this.go("empty");
      await api.voiceLiveListen(false);
      return;
    }
    this.deps.send(words);
  }

  /** Stop speaking and listen for them. */
  private async barge() {
    this.hushPlayback();
    this.go("barge");
    await api.voiceReplyHush().catch(() => {});
    await api.voiceLivePlaying(false).catch(() => {});
  }

  private hushPlayback() {
    this.queue = [];
    this.player?.stop();
    this.player = null;
  }

  /** More of the reply as it streams. */
  feed(delta: string) {
    if (this.state === "off") return;
    this.reply = this.reply
      .then(() => api.voiceReplyFeed(delta))
      .then((pieces) => this.enqueue(pieces))
      .catch((e) => this.note(e));
  }

  /** The reply is complete; `whole` is spoken if nothing streamed. */
  replyDone(whole: string) {
    if (this.state === "off") return;
    this.reply = this.reply
      .then(() => api.voiceReplyFinish(whole))
      .then((pieces) => {
        this.enqueue(pieces);
        this.finished = true;
        return this.drained();
      })
      .catch((e) => this.note(e));
  }

  private enqueue(pieces: string[]) {
    if (this.noVoice) return;
    for (const piece of pieces) {
      this.queue.push(
        api.voiceSpeak(piece).catch((e) => {
          if (!this.warned) this.note(e);
          this.warned = true;
          this.noVoice = true;
          return null;
        }),
      );
    }
    void this.pump();
  }

  private async pump(): Promise<void> {
    if (this.player || this.queue.length === 0) return;
    const next = this.queue.shift()!;
    const holder: Playing = { done: Promise.resolve(), stop: () => {} };
    this.player = holder;
    const spoken = await next;
    if (this.player !== holder) return;
    if (spoken) {
      if (!this.spoke) {
        this.spoke = true;
        this.go("spoke");
        // Listen over the reply so they can talk over it.
        await api.voiceLiveListen(true).catch((e) => this.note(e));
      }
      const playing = this.deps.play(spoken);
      this.player = playing;
      await playing.done.catch(() => {});
      if (this.player !== playing) return;
    }
    this.player = null;
    if (this.queue.length) return this.pump();
    return this.drained();
  }

  private async drained() {
    if (!this.finished || this.player || this.queue.length) return;
    this.finished = false;
    this.spoke = false;
    // The next reply tries its voice again (a failed piece may have been a blip).
    this.noVoice = false;
    if (this.state === "speaking") {
      this.go("done");
      await api.voiceLivePlaying(false).catch(() => {});
    } else if (this.state === "thinking") {
      this.go("done");
      await api.voiceLiveListen(false).catch((e) => this.note(e));
    }
  }
}

/** The webview's mic, with the browser's echo cancelling the reply. */
export async function openBrowserMic(take: (samples: Float32Array, rate: number) => void): Promise<() => void> {
  const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!navigator.mediaDevices?.getUserMedia || !Ctx) throw new Error(CANT_RECORD);
  let stream: MediaStream;
  try {
    stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
    });
  } catch (err) {
    const name = (err as { name?: string })?.name;
    if (name === "NotAllowedError" || name === "SecurityError") throw new Error(MIC_DENIED, { cause: err });
    if (name === "NotFoundError") throw new Error(NO_MIC, { cause: err });
    throw new Error(CANT_RECORD, { cause: err });
  }
  const ctx = new Ctx();
  const source = ctx.createMediaStreamSource(stream);
  const untap = await tap(ctx, source, (c) => take(c, ctx.sampleRate));
  return () => {
    untap();
    source.disconnect();
    for (const track of stream.getTracks()) track.stop();
    void ctx.close();
  };
}

/** Plays one piece through an audio element. */
export function playSpoken(spoken: Spoken): Playing {
  const audio = new Audio(`data:${spoken.mime};base64,${spoken.audio}`);
  let end = () => {};
  const done = new Promise<void>((resolve) => {
    end = resolve;
    audio.onended = () => resolve();
    audio.onerror = () => resolve();
  });
  audio.play().catch(() => end());
  return {
    done,
    stop: () => {
      audio.pause();
      end();
    },
  };
}

const LABEL: Record<VoiceState, string> = {
  off: "Voice mode",
  listening: "Listening",
  thinking: "Thinking",
  speaking: "Speaking",
};

const HINT: Record<VoiceState, string> = {
  off: "Talk hands-free and hear the reply",
  listening: "Listening. Tap when you're done, or tap to turn voice mode off",
  thinking: "Thinking. Tap to turn voice mode off",
  speaking: "Speaking. Tap to stop and talk",
};

/** The voice-mode session behind the chat's button. The chat sets where
 * heard words and notes go with `session.handlers`. */
export function useVoiceMode() {
  const [state, setState] = useState<VoiceState>("off");
  const [session] = useState(
    () =>
      new VoiceSession({
        openMic: openBrowserMic,
        play: playSpoken,
        send: () => {},
        onState: setState,
        onNote: () => {},
      }),
  );
  useEffect(
    () => () => {
      void session.stop();
    },
    [session],
  );
  return { state, session };
}

export function VoiceButton({
  state,
  session,
  disabled,
}: {
  state: VoiceState;
  session: VoiceSession;
  disabled?: boolean;
}) {
  const onClick = () => {
    if (state === "off") session.start().catch((e: unknown) => session.stop(e instanceof Error ? e.message : String(e)));
    else void session.tap();
  };
  return (
    <button
      type="button"
      className={`voice-mode voice-${state}`}
      aria-pressed={state !== "off"}
      aria-label={HINT[state]}
      title={HINT[state]}
      disabled={disabled && state === "off"}
      onClick={onClick}
    >
      {LABEL[state]}
    </button>
  );
}
