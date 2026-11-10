// Push to talk (LO-17j): hold the mic (or Ctrl+G) to record, let go to turn
// it into words in the draft. It never sends; the person presses Send. The mic
// stays open for a couple of minutes after a hold, so the next one starts at
// once and keeps the half second before the press; letting go keeps a short
// tail so the last word is not cut off.
import { useEffect, useRef, useState } from "react";
import { transcribe } from "./api";
import { PRODUCT_NAME } from "./brand";
import { Orb } from "./Orb";
import { downsample, encodeWav, toBase64 } from "./wav";

/** The desktop refuses more (src-tauri/src/voice.rs). */
export const MAX_MS = 60_000;
export const MIC_DENIED = `${PRODUCT_NAME} isn't allowed to use the microphone. Allow it in your system settings, then hold the mic again.`;
export const NO_MIC = "No microphone found.";
export const CANT_RECORD = "This device can't record here.";
export const NOTHING_HEARD = "Didn't catch any sound. Hold the mic and speak, then let go.";

/** The words go after what's typed, with one space between. */
export function appendDraft(draft: string, words: string): string {
  const said = words.trim();
  if (!said) return draft;
  const typed = draft.trimEnd();
  return typed ? `${typed} ${said}` : said;
}

/** Kept from just before the press, so a word that starts with it isn't clipped. */
export const PRE_ROLL_MS = 500;
/** Kept after letting go, so the last word isn't cut off. */
export const TAIL_MS = 300;
/** How long the mic stays open after a hold, so the next one starts at once. */
export const WARM_MS = 120_000;

type AudioCtor = typeof AudioContext;

/** One open mic, shared by every hold until it has been idle for a while. */
interface Warm {
  stream: MediaStream;
  ctx: AudioContext;
  stop: () => void;
  /** The last moment before a press. */
  ring: Float32Array[];
  /** The hold under way, if any. */
  taking: Float32Array[] | null;
  idle?: ReturnType<typeof setTimeout>;
}

let warm: Promise<Warm> | null = null;

function onChunk(w: Warm, chunk: Float32Array) {
  if (w.taking) {
    w.taking.push(chunk);
    return;
  }
  w.ring.push(chunk);
  const keep = (w.ctx.sampleRate * PRE_ROLL_MS) / 1000;
  let total = w.ring.reduce((n, c) => n + c.length, 0);
  while (w.ring.length > 1 && total - w.ring[0].length >= keep) total -= w.ring.shift()!.length;
}

/** Copy the mic's samples off the audio thread when the webview can; the
 * older processor otherwise (and in tests). */
async function tap(ctx: AudioContext, source: MediaStreamAudioSourceNode, take: (c: Float32Array) => void) {
  if (ctx.audioWorklet) {
    try {
      await ctx.audioWorklet.addModule("/mic-worklet.js");
      const node = new AudioWorkletNode(ctx, "mic-tap", { numberOfOutputs: 0 });
      node.port.onmessage = (e: MessageEvent<Float32Array>) => take(e.data);
      source.connect(node);
      return () => {
        node.port.onmessage = null;
        node.disconnect();
      };
    } catch {
      // Fall through to the processor.
    }
  }
  const node = ctx.createScriptProcessor(4096, 1, 1);
  node.onaudioprocess = (event) => take(new Float32Array(event.inputBuffer.getChannelData(0)));
  source.connect(node);
  node.connect(ctx.destination);
  return () => {
    node.onaudioprocess = null;
    node.disconnect();
  };
}

async function openWarm(): Promise<Warm> {
  const Ctx: AudioCtor | undefined =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: AudioCtor }).webkitAudioContext;
  if (!navigator.mediaDevices?.getUserMedia || !Ctx) throw new Error(CANT_RECORD);
  const stream = await navigator.mediaDevices.getUserMedia({
    audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: true },
  });
  const ctx = new Ctx();
  const source = ctx.createMediaStreamSource(stream);
  const w: Warm = { stream, ctx, stop: () => {}, ring: [], taking: null };
  const untap = await tap(ctx, source, (c) => onChunk(w, c));
  w.stop = () => {
    untap();
    source.disconnect();
  };
  return w;
}

/** Close the mic now (idle long enough, or the chat went away). */
export async function closeMic() {
  const open = warm;
  warm = null;
  if (!open) return;
  const w = await open.catch(() => null);
  if (!w) return;
  clearTimeout(w.idle);
  w.stop();
  for (const track of w.stream.getTracks()) track.stop();
  await w.ctx.close();
}

interface Recording {
  /** Stops taking (after the tail) and hands back the WAV. */
  stop: () => Promise<Uint8Array>;
}

async function record(): Promise<Recording> {
  if (!warm) {
    warm = openWarm();
    warm.catch(() => {
      warm = null;
    });
  }
  const w = await warm;
  clearTimeout(w.idle);
  w.taking = w.ring;
  w.ring = [];
  return {
    stop: async () => {
      await new Promise((done) => setTimeout(done, TAIL_MS));
      const chunks = w.taking ?? [];
      w.taking = null;
      w.idle = setTimeout(() => void closeMic(), WARM_MS);
      const all = new Float32Array(chunks.reduce((n, c) => n + c.length, 0));
      let at = 0;
      for (const c of chunks) {
        all.set(c, at);
        at += c.length;
      }
      return encodeWav(downsample(all, w.ctx.sampleRate));
    },
  };
}

function micNote(err: unknown): string {
  const name = err instanceof Error || err instanceof DOMException ? err.name : "";
  if (name === "NotAllowedError" || name === "SecurityError") return MIC_DENIED;
  if (name === "NotFoundError" || name === "OverconstrainedError") return NO_MIC;
  if (err instanceof Error && err.message === CANT_RECORD) return CANT_RECORD;
  return String(err);
}

type State = "idle" | "recording" | "transcribing";

/** The hold-to-talk button. `active`: the Chat tab shows, so Ctrl+G is for it. */
export function Mic({
  onText,
  onError,
  busy,
  active,
}: {
  onText: (words: string) => void;
  /** A note for the chat's error line (no mic, permission, the desktop's refusal). */
  onError: (note: string) => void;
  busy: boolean;
  active: boolean;
}) {
  const [state, setState] = useState<State>("idle");
  const setNote = (note: string) => onError(note);
  const pending = useRef<Promise<Recording | null> | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  function start() {
    if (pending.current || busy) return;
    setState("recording");
    pending.current = record().catch((err: unknown) => {
      setNote(micNote(err));
      return null;
    });
    timer.current = setTimeout(() => void finish(), MAX_MS);
  }

  async function finish() {
    const started = pending.current;
    if (!started) return;
    pending.current = null;
    clearTimeout(timer.current);
    const rec = await started;
    if (!rec) {
      setState("idle");
      return;
    }
    setState("transcribing");
    try {
      const wav = await rec.stop();
      if (wav.length <= 44) {
        setNote(NOTHING_HEARD);
        return;
      }
      onText(await transcribe(toBase64(wav)));
    } catch (err) {
      setNote(String(err));
    } finally {
      setState("idle");
    }
  }

  // Ctrl+G holds to talk while the chat shows, as in the cabin.
  const live = useRef({ start, finish, active });
  live.current = { start, finish, active };
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (!live.current.active || !e.ctrlKey || e.key.toLowerCase() !== "g") return;
      e.preventDefault();
      if (!e.repeat) live.current.start();
    };
    const up = (e: KeyboardEvent) => {
      if (e.key.toLowerCase() === "g" || e.key === "Control") void live.current.finish();
    };
    // A window that loses focus never gets the keyup: end the hold as letting go does.
    const end = () => void live.current.finish();
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    window.addEventListener("blur", end);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
      window.removeEventListener("blur", end);
    };
  }, []);

  useEffect(
    () => () => {
      clearTimeout(timer.current);
      void closeMic();
    },
    [],
  );

  const label =
    state === "recording" ? "Listening… let go to stop" : state === "transcribing" ? "Turning speech into text" : "Hold to talk (Ctrl+G)";
  return (
    <button
      type="button"
      className={state === "recording" ? "quiet mic recording" : "quiet mic"}
      aria-label={label}
      title={label}
      aria-pressed={state === "recording"}
      disabled={busy || state === "transcribing"}
      onPointerDown={(e) => {
        e.preventDefault();
        // A finger that slides off the button while talking keeps the hold.
        e.currentTarget.setPointerCapture?.(e.pointerId);
        start();
      }}
      onPointerUp={() => void finish()}
      onPointerCancel={() => void finish()}
      onLostPointerCapture={() => void finish()}
    >
      {state === "transcribing" ? (
        <Orb state="composing" />
      ) : state === "recording" ? (
        <Orb state="listening" />
      ) : (
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
        </svg>
      )}
    </button>
  );
}
