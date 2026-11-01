// Push to talk (LO-17j): hold the mic (or Ctrl+G) to record, let go to turn
// it into words in the draft. It never sends; the person presses Send.
import { useEffect, useRef, useState } from "react";
import { transcribe } from "./api";
import { PRODUCT_NAME } from "./brand";
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

interface Recording {
  /** Stops the mic and hands back the WAV. */
  stop: () => Promise<Uint8Array>;
}

type AudioCtor = typeof AudioContext;

async function record(): Promise<Recording> {
  const Ctx: AudioCtor | undefined =
    window.AudioContext ?? (window as unknown as { webkitAudioContext?: AudioCtor }).webkitAudioContext;
  if (!navigator.mediaDevices?.getUserMedia || !Ctx) throw new Error(CANT_RECORD);
  const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1 } });
  const ctx = new Ctx();
  const source = ctx.createMediaStreamSource(stream);
  const node = ctx.createScriptProcessor(4096, 1, 1);
  const chunks: Float32Array[] = [];
  node.onaudioprocess = (event) => {
    chunks.push(new Float32Array(event.inputBuffer.getChannelData(0)));
  };
  source.connect(node);
  node.connect(ctx.destination);
  return {
    stop: async () => {
      node.onaudioprocess = null;
      node.disconnect();
      source.disconnect();
      for (const track of stream.getTracks()) track.stop();
      const rate = ctx.sampleRate;
      await ctx.close();
      const all = new Float32Array(chunks.reduce((n, c) => n + c.length, 0));
      let at = 0;
      for (const c of chunks) {
        all.set(c, at);
        at += c.length;
      }
      return encodeWav(downsample(all, rate));
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
    window.addEventListener("keydown", down);
    window.addEventListener("keyup", up);
    return () => {
      window.removeEventListener("keydown", down);
      window.removeEventListener("keyup", up);
    };
  }, []);

  useEffect(() => () => clearTimeout(timer.current), []);

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
        start();
      }}
      onPointerUp={() => void finish()}
      onPointerLeave={() => void finish()}
      onPointerCancel={() => void finish()}
    >
      {state === "transcribing" ? (
        "…"
      ) : (
        <svg viewBox="0 0 24 24" width="18" height="18" aria-hidden="true" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round">
          <rect x="9" y="3" width="6" height="11" rx="3" />
          <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
        </svg>
      )}
    </button>
  );
}
