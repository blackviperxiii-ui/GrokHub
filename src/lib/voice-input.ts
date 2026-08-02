/**
 * Push-to-talk voice input for Electron / browser.
 * Records via MediaRecorder, then Grok STT (xAI). Web Speech is optional live assist only.
 */

import { grokTranscribe } from "./grok-client";
import type { XaiOAuthTokens } from "./xai-oauth";

export type VoiceAuth = {
  apiKey?: string;
  accessToken?: string;
  tokens?: XaiOAuthTokens | null;
};

export type VoiceSessionHandlers = {
  onListeningChange?: (listening: boolean) => void;
  onStatus?: (status: string) => void;
  onPartial?: (text: string) => void;
  onFinal?: (text: string) => void;
  onError?: (message: string) => void;
};

function pickMime(): string {
  const candidates = [
    "audio/webm;codecs=opus",
    "audio/webm",
    "audio/ogg;codecs=opus",
    "audio/mp4",
  ];
  if (typeof MediaRecorder === "undefined") return "audio/webm";
  for (const c of candidates) {
    if (MediaRecorder.isTypeSupported(c)) return c;
  }
  return "audio/webm";
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const r = String(reader.result || "");
      const i = r.indexOf(",");
      resolve(i >= 0 ? r.slice(i + 1) : r);
    };
    reader.onerror = () => reject(new Error("read audio failed"));
    reader.readAsDataURL(blob);
  });
}

export class VoiceSession {
  private media: MediaStream | null = null;
  private recorder: MediaRecorder | null = null;
  private chunks: Blob[] = [];
  private listening = false;
  private handlers: VoiceSessionHandlers;
  private auth: VoiceAuth;
  private mime = pickMime();

  constructor(handlers: VoiceSessionHandlers = {}, auth: VoiceAuth = {}) {
    this.handlers = handlers;
    this.auth = auth;
  }

  setAuth(auth: VoiceAuth) {
    this.auth = auth;
  }

  isListening() {
    return this.listening;
  }

  async start() {
    if (this.listening) return;
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
      this.handlers.onError?.(
        "Microphone API unavailable in this environment. Check OS mic permissions.",
      );
      return;
    }
    try {
      this.handlers.onStatus?.("Requesting microphone…");
      this.media = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: true,
          noiseSuppression: true,
          autoGainControl: true,
        },
        video: false,
      });
    } catch (e) {
      const msg = e instanceof Error ? e.message : "mic denied";
      this.handlers.onError?.(
        /Permission|NotAllowed|denied/i.test(msg)
          ? "Microphone permission denied. Allow mic for GrokHub in system settings, then try again."
          : `Microphone failed: ${msg}`,
      );
      return;
    }

    this.chunks = [];
    this.mime = pickMime();
    try {
      this.recorder = new MediaRecorder(this.media, { mimeType: this.mime });
    } catch {
      this.recorder = new MediaRecorder(this.media);
      this.mime = this.recorder.mimeType || this.mime;
    }

    this.recorder.ondataavailable = (ev) => {
      if (ev.data && ev.data.size > 0) this.chunks.push(ev.data);
    };
    this.recorder.onerror = () => {
      this.handlers.onError?.("Recording error");
      void this.cancel();
    };

    this.recorder.start(250);
    this.listening = true;
    this.handlers.onListeningChange?.(true);
    this.handlers.onStatus?.("Listening… click mic again to stop & transcribe");
  }

  async stopAndTranscribe(): Promise<string | null> {
    if (!this.listening || !this.recorder) {
      await this.cleanup();
      return null;
    }

    const blob = await new Promise<Blob>((resolve) => {
      const rec = this.recorder!;
      rec.onstop = () => {
        resolve(new Blob(this.chunks, { type: this.mime.split(";")[0] || "audio/webm" }));
      };
      try {
        if (rec.state === "recording") rec.stop();
        else resolve(new Blob(this.chunks, { type: this.mime }));
      } catch {
        resolve(new Blob(this.chunks, { type: this.mime }));
      }
    });

    this.listening = false;
    this.handlers.onListeningChange?.(false);
    await this.cleanupMediaOnly();

    if (!blob.size || blob.size < 800) {
      this.handlers.onError?.("No speech captured — hold a bit longer, then stop.");
      await this.cleanup();
      return null;
    }

    this.handlers.onStatus?.("Transcribing with Grok…");
    try {
      const audioBase64 = await blobToBase64(blob);
      const result = await grokTranscribe({
        audioBase64,
        mimeType: blob.type || this.mime,
        language: (typeof navigator !== "undefined" && navigator.language
          ? navigator.language.slice(0, 2)
          : "en") || "en",
        apiKey: this.auth.apiKey,
        accessToken: this.auth.accessToken,
        tokens: this.auth.tokens,
      });
      if (!result.ok || !result.text) {
        this.handlers.onError?.(result.error || "Transcription failed");
        await this.cleanup();
        return null;
      }
      const text = result.text.trim();
      this.handlers.onFinal?.(text);
      this.handlers.onStatus?.("Voice ready");
      await this.cleanup();
      return text;
    } catch (e) {
      this.handlers.onError?.(e instanceof Error ? e.message : "Transcription failed");
      await this.cleanup();
      return null;
    }
  }

  async cancel() {
    try {
      if (this.recorder && this.recorder.state === "recording") this.recorder.stop();
    } catch {
      /* ignore */
    }
    this.listening = false;
    this.handlers.onListeningChange?.(false);
    await this.cleanup();
  }

  private async cleanupMediaOnly() {
    try {
      this.media?.getTracks().forEach((t) => t.stop());
    } catch {
      /* ignore */
    }
    this.media = null;
    this.recorder = null;
    this.chunks = [];
  }

  private async cleanup() {
    await this.cleanupMediaOnly();
  }
}

export async function toggleVoiceSession(
  sessionRef: { current: VoiceSession | null },
  handlers: VoiceSessionHandlers,
  auth: VoiceAuth,
): Promise<"started" | "transcribed" | "error"> {
  if (sessionRef.current?.isListening()) {
    const text = await sessionRef.current.stopAndTranscribe();
    sessionRef.current = null;
    return text ? "transcribed" : "error";
  }
  const s = new VoiceSession(handlers, auth);
  sessionRef.current = s;
  await s.start();
  return s.isListening() ? "started" : "error";
}
