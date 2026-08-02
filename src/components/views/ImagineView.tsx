import {
  ArrowUp,
  Download,
  ImageIcon,
  Loader2,
  Mic,
  MicOff,
  Plus,
  Ratio,
  Sparkles,
  Trash2,
  Video,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { IMAGINE_PRESETS } from "@/lib/imagine";
import { useGrokHub } from "@/lib/store";
import type { ImagineAspect, ImagineQuality } from "@/lib/types";
import { cn } from "@/lib/utils";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Textarea } from "../ui/textarea";

const ASPECTS: { id: ImagineAspect; label: string }[] = [
  { id: "auto", label: "Auto" },
  { id: "1:1", label: "1:1" },
  { id: "3:2", label: "3:2" },
  { id: "2:3", label: "2:3" },
  { id: "16:9", label: "16:9" },
  { id: "9:16", label: "9:16" },
  { id: "4:3", label: "4:3" },
];

function Pill({
  active,
  onClick,
  children,
  disabled,
  title,
}: {
  active?: boolean;
  onClick?: () => void;
  children: React.ReactNode;
  disabled?: boolean;
  title?: string;
}) {
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "inline-flex h-8 shrink-0 items-center gap-1.5 rounded-full border px-3 text-xs font-medium transition-colors",
        active
          ? "border-[var(--color-border-strong)] bg-[var(--color-elevated)] text-[var(--color-fg)]"
          : "border-[var(--color-border)] text-[var(--color-muted)] hover:border-[var(--color-border-strong)] hover:text-[var(--color-fg)]",
        disabled && "opacity-50",
      )}
    >
      {children}
    </button>
  );
}

export function ImagineView() {
  const prompt = useGrokHub((s) => s.imaginePrompt ?? "");
  const aspect = useGrokHub((s) => s.imagineAspect ?? "auto");
  const mediaKind = useGrokHub((s) => s.imagineMediaKind ?? "image");
  const quality = useGrokHub((s) => s.imagineQuality ?? "speed");
  const reference = useGrokHub((s) => s.imagineReference ?? null);
  const jobs = useGrokHub((s) => s.imagineJobs ?? []);
  const busy = useGrokHub((s) => Boolean(s.imagineBusy));
  const err = useGrokHub((s) => s.imagineError);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const setImaginePrompt = useGrokHub((s) => s.setImaginePrompt);
  const setImagineAspect = useGrokHub((s) => s.setImagineAspect);
  const setImagineMediaKind = useGrokHub((s) => s.setImagineMediaKind);
  const setImagineQuality = useGrokHub((s) => s.setImagineQuality);
  const setImagineReference = useGrokHub((s) => s.setImagineReference);
  const runImagine = useGrokHub((s) => s.runImagine);
  const removeImagineJob = useGrokHub((s) => s.removeImagineJob);
  const clearImagineJobs = useGrokHub((s) => s.clearImagineJobs);

  const [listening, setListening] = useState(false);
  const [voiceStatus, setVoiceStatus] = useState<string | null>(null);
  const voiceRef = useRef<import("@/lib/voice-input").VoiceSession | null>(null);
  const [aspectOpen, setAspectOpen] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);
  const taRef = useRef<HTMLTextAreaElement>(null);
  const latest = jobs[0];

  useEffect(() => {
    return () => {
      // stop speech if unmount
      try {
        const SR = (window as unknown as { webkitSpeechRecognition?: new () => SpeechRecognition }).webkitSpeechRecognition
          || (window as unknown as { SpeechRecognition?: new () => SpeechRecognition }).SpeechRecognition;
        void SR;
      } catch {
        /* ignore */
      }
    };
  }, []);

  function applyPreset(prefix: string) {
    const body = prompt.trim();
    if (body.toLowerCase().startsWith(prefix.toLowerCase())) return;
    setImaginePrompt(prefix + (body || ""));
    taRef.current?.focus();
  }

  function onPickFile(file: File | null) {
    if (!file) return;
    if (!file.type.startsWith("image/")) return;
    const reader = new FileReader();
    reader.onload = () => {
      const url = String(reader.result || "");
      if (url.startsWith("data:image")) setImagineReference(url);
    };
    reader.readAsDataURL(file);
  }

  function toggleMic() {
    void (async () => {
      const { toggleVoiceSession } = await import("@/lib/voice-input");
      const st = useGrokHub.getState();
      if (listening && voiceRef.current) {
        setVoiceStatus("Transcribing…");
        const text = await voiceRef.current.stopAndTranscribe();
        voiceRef.current = null;
        setListening(false);
        if (text) {
          setImaginePrompt((prompt ? prompt.replace(/\s+$/, "") + " " : "") + text);
          setVoiceStatus(null);
          taRef.current?.focus();
        } else {
          setVoiceStatus(null);
        }
        return;
      }
      await toggleVoiceSession(
        voiceRef,
        {
          onListeningChange: setListening,
          onStatus: setVoiceStatus,
          onError: (message) => {
            setListening(false);
            setVoiceStatus(message);
          },
          onFinal: () => setVoiceStatus(null),
        },
        {
          apiKey: st.apiKey || undefined,
          accessToken: st.oauth?.accessToken,
          tokens: st.oauth,
        },
      );
    })();
  }

  async function onSubmit() {
    if (busy || !prompt.trim()) return;
    await runImagine();
  }

  return (
    <div className="flex h-full min-h-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-2 px-0.5">
        <div>
          <h2 className="flex items-center gap-2 text-sm font-semibold">
            <ImageIcon className="h-4 w-4" />
            Imagine
          </h2>
          <p className="text-xs text-[var(--color-muted)]">
            Website-style composer · image & video · speed / quality · aspect · reference
          </p>
        </div>
        <div className="flex gap-1.5">
          {jobs.length > 0 && (
            <Button
              size="sm"
              variant="ghost"
              className="text-[var(--color-muted)] hover:text-[var(--color-danger)]"
              title="Delete all generated images and videos"
              onClick={() => {
                if (
                  typeof window !== "undefined" &&
                  !window.confirm(`Delete all ${jobs.length} Imagine items?`)
                ) {
                  return;
                }
                clearImagineJobs();
              }}
            >
              <Trash2 className="h-3.5 w-3.5" />
              Clear all
            </Button>
          )}
          <Badge variant={grokConnected ? "success" : "default"}>
            {grokConnected ? "Grok live" : "Local preview"}
          </Badge>
          <Badge className="font-mono capitalize">{mediaKind}</Badge>
        </div>
      </div>

      {err && (
        <div className="rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-warn)]">
          {err}
        </div>
      )}

      {/* Gallery */}
      <div className="scroll-panel min-h-0 flex-1 space-y-4">
        {latest && (latest.imageDataUrl || latest.videoDataUrl) && latest.status === "ready" && (
          <Card className="overflow-hidden">
            <CardHeader className="flex-row items-center justify-between gap-3 space-y-0">
              <div className="min-w-0">
                <CardTitle className="text-sm">Latest</CardTitle>
                <CardDescription className="line-clamp-1">{latest.prompt}</CardDescription>
              </div>
              <div className="flex shrink-0 gap-2">
                {latest.imageDataUrl && (
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => setImagineReference(latest.imageDataUrl || null)}
                  >
                    Use as ref
                  </Button>
                )}
                {(latest.videoDataUrl || latest.imageDataUrl) && (
                  <a
                    href={latest.videoDataUrl || latest.imageDataUrl}
                    download={`grokhub-imagine-${latest.id}.${latest.videoDataUrl ? "mp4" : latest.imageDataUrl?.startsWith("data:image/svg") ? "svg" : "png"}`}
                    className="inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]"
                  >
                    <Download className="h-3.5 w-3.5" />
                    Save
                  </a>
                )}
                <Button
                  size="sm"
                  variant="ghost"
                  className="text-[var(--color-muted)] hover:text-[var(--color-danger)]"
                  title="Delete this generation"
                  onClick={() => removeImagineJob(latest.id)}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                  Delete
                </Button>
              </div>
            </CardHeader>
            <CardContent>
              <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)]">
                {latest.videoDataUrl ? (
                  <video
                    src={latest.videoDataUrl}
                    controls
                    className="mx-auto max-h-[min(70vh,640px)] w-full bg-black"
                  />
                ) : (
                  <img
                    src={latest.imageDataUrl}
                    alt={latest.prompt}
                    className="mx-auto max-h-[min(70vh,640px)] w-full object-contain"
                  />
                )}
              </div>
              <p className="mt-2 font-mono text-[10px] text-[var(--color-subtle)]">
                {latest.aspect} · {latest.quality || "speed"} · {latest.mediaKind || "image"}
                {latest.model ? ` · ${latest.model}` : ""}
                {latest.source ? ` · ${latest.source}` : ""}
              </p>
            </CardContent>
          </Card>
        )}

        {/* Presets row */}
        <div className="flex flex-wrap gap-1.5">
          {IMAGINE_PRESETS.map((pr) => (
            <Pill key={pr.id} onClick={() => applyPreset(pr.prefix)} title={pr.prefix}>
              <Sparkles className="h-3 w-3 opacity-70" />
              {pr.label}
            </Pill>
          ))}
        </div>

        <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
          {jobs.map((job) => (
            <Card key={job.id} className="group relative overflow-hidden">
              <div className="absolute right-2 top-2 z-20 flex gap-1">
                {(job.imageDataUrl || job.videoDataUrl) && (
                  <a
                    href={job.videoDataUrl || job.imageDataUrl}
                    download={`grokhub-${job.id}.${job.videoDataUrl ? "mp4" : "png"}`}
                    className="inline-flex h-8 w-8 items-center justify-center rounded-full border border-[var(--color-border)] bg-[color-mix(in_oklab,var(--color-surface)_92%,transparent)] text-[var(--color-muted)] shadow-sm backdrop-blur hover:text-[var(--color-fg)]"
                    title="Save"
                    onClick={(e) => e.stopPropagation()}
                  >
                    <Download className="h-3.5 w-3.5" />
                  </a>
                )}
                <button
                  type="button"
                  title="Delete"
                  aria-label="Delete Imagine item"
                  className="inline-flex h-8 w-8 items-center justify-center rounded-full border border-[color-mix(in_oklab,var(--color-danger)_40%,var(--color-border))] bg-[color-mix(in_oklab,var(--color-surface)_92%,transparent)] text-[var(--color-danger)] shadow-sm backdrop-blur hover:bg-[color-mix(in_oklab,var(--color-danger)_12%,var(--color-surface))]"
                  onClick={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    removeImagineJob(job.id);
                  }}
                >
                  <Trash2 className="h-3.5 w-3.5" />
                </button>
              </div>
              <div className="aspect-video bg-[var(--color-surface)]">
                {job.videoDataUrl ? (
                  <video src={job.videoDataUrl} className="h-full w-full object-cover" muted />
                ) : job.imageDataUrl ? (
                  <img
                    src={job.imageDataUrl}
                    alt={job.prompt}
                    className="h-full w-full object-cover"
                  />
                ) : (
                  <div className="flex h-full flex-col items-center justify-center gap-1 px-3 text-center text-xs text-[var(--color-subtle)]">
                    {job.status === "rendering" ? (
                      <span className="inline-flex items-center gap-2">
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                        Rendering…
                      </span>
                    ) : job.imageRelPath || job.videoRelPath ? (
                      <span>Media on disk — reloading…</span>
                    ) : (
                      "No media"
                    )}
                  </div>
                )}
              </div>
              <CardContent className="space-y-1 p-3">
                <div className="flex items-center justify-between gap-2">
                  <Badge variant={job.status === "ready" ? "success" : "info"}>
                    {job.mediaKind || "image"} · {job.status}
                  </Badge>
                  <RelativeTime ts={job.ts} className="text-[10px] text-[var(--color-subtle)]" />
                </div>
                <p className="line-clamp-2 text-xs text-[var(--color-muted)]">{job.prompt}</p>
                <p className="font-mono text-[10px] text-[var(--color-subtle)]">
                  {job.aspect} · {job.quality || "speed"}
                  {job.model ? ` · ${job.model}` : ""}
                </p>
                <div className="flex flex-wrap items-center gap-3 pt-0.5">
                  {job.status === "ready" && (job.imageDataUrl || job.videoDataUrl) && (
                    <button
                      type="button"
                      className="text-[10px] text-[var(--color-info)] hover:underline"
                      onClick={() => {
                        setImaginePrompt(job.prompt);
                        setImagineAspect(job.aspect);
                        if (job.quality) setImagineQuality(job.quality);
                        if (job.mediaKind) setImagineMediaKind(job.mediaKind);
                      }}
                    >
                      Reuse settings
                    </button>
                  )}
                  <button
                    type="button"
                    className="text-[10px] font-medium text-[var(--color-danger)] hover:underline"
                    onClick={() => removeImagineJob(job.id)}
                  >
                    Delete
                  </button>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>

        {jobs.length === 0 && (
          <Card>
            <CardContent className="py-12 text-center text-sm text-[var(--color-muted)]">
              Type to imagine — pick Image or Video, Speed or Quality, and an aspect ratio.
            </CardContent>
          </Card>
        )}
      </div>

      {/* Website-style bottom composer */}
      <div className="shrink-0 rounded-2xl border border-[var(--color-border)] bg-[var(--color-surface)] p-3 shadow-[0_8px_40px_rgba(0,0,0,0.35)]">
        {reference && (
          <div className="mb-2 flex items-center gap-2">
            <img
              src={reference}
              alt="Reference"
              className="h-12 w-12 rounded-lg border border-[var(--color-border)] object-cover"
            />
            <span className="text-xs text-[var(--color-muted)]">Reference attached</span>
            <button
              type="button"
              className="ml-auto rounded-full p-1 text-[var(--color-muted)] hover:bg-[var(--color-elevated)]"
              onClick={() => setImagineReference(null)}
              aria-label="Remove reference"
            >
              <X className="h-4 w-4" />
            </button>
          </div>
        )}
        <Textarea
          ref={taRef}
          value={prompt}
          onChange={(e) => setImaginePrompt(e.target.value)}
          placeholder="Type to imagine"
          disabled={busy}
          rows={2}
          className="min-h-[52px] resize-none border-0 bg-transparent px-1 py-1 text-sm shadow-none focus-visible:ring-0"
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void onSubmit();
            }
          }}
        />
        <div className="mt-2 flex flex-wrap items-center gap-1.5">
          <input
            ref={fileRef}
            type="file"
            accept="image/*"
            className="hidden"
            onChange={(e) => onPickFile(e.target.files?.[0] || null)}
          />
          <Pill
            title="Attach reference image"
            onClick={() => fileRef.current?.click()}
            disabled={busy}
          >
            <Plus className="h-3.5 w-3.5" />
          </Pill>

          <Pill
            active={mediaKind === "image"}
            onClick={() => setImagineMediaKind("image")}
            disabled={busy}
          >
            <ImageIcon className="h-3.5 w-3.5" />
            Image
          </Pill>
          <Pill
            active={mediaKind === "video"}
            onClick={() => setImagineMediaKind("video")}
            disabled={busy}
          >
            <Video className="h-3.5 w-3.5" />
            Video
          </Pill>

          <Pill
            active={quality === "speed"}
            onClick={() => setImagineQuality("speed" as ImagineQuality)}
            disabled={busy}
            title="Faster draft"
          >
            Speed
          </Pill>
          <Pill
            active={quality === "quality"}
            onClick={() => setImagineQuality("quality")}
            disabled={busy}
            title="Higher fidelity"
          >
            Quality
          </Pill>

          <div className="relative">
            <Pill
              active={aspectOpen || aspect !== "auto"}
              onClick={() => setAspectOpen((v) => !v)}
              disabled={busy}
              title="Aspect ratio"
            >
              <Ratio className="h-3.5 w-3.5" />
              {aspect === "auto" ? "Auto" : aspect}
            </Pill>
            {aspectOpen && (
              <div className="absolute bottom-full left-0 z-20 mb-2 flex min-w-[10rem] flex-col gap-0.5 rounded-xl border border-[var(--color-border)] bg-[var(--color-elevated)] p-1.5 shadow-xl">
                {ASPECTS.map((a) => (
                  <button
                    key={a.id}
                    type="button"
                    className={cn(
                      "rounded-lg px-3 py-1.5 text-left text-xs",
                      a.id === aspect
                        ? "bg-[var(--color-surface)] font-medium"
                        : "text-[var(--color-muted)] hover:bg-[var(--color-surface)]",
                    )}
                    onClick={() => {
                      setImagineAspect(a.id);
                      setAspectOpen(false);
                    }}
                  >
                    {a.label}
                  </button>
                ))}
              </div>
            )}
          </div>

          <div className="ml-auto flex items-center gap-1.5">
            <Pill
              active={listening}
              onClick={toggleMic}
              disabled={busy}
              title={listening ? "Stop & transcribe" : "Voice prompt"}
            >
              {listening ? <MicOff className="h-3.5 w-3.5" /> : <Mic className="h-3.5 w-3.5" />}
            </Pill>
            {voiceStatus && (
              <span className="max-w-[10rem] truncate text-[10px] text-[var(--color-info)]">
                {voiceStatus}
              </span>
            )}
            <button
              type="button"
              disabled={busy || !prompt.trim()}
              onClick={() => void onSubmit()}
              className={cn(
                "inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors",
                busy || !prompt.trim()
                  ? "bg-[var(--color-elevated)] text-[var(--color-subtle)]"
                  : "bg-[var(--color-fg)] text-[var(--color-bg)] hover:opacity-90",
              )}
              aria-label="Generate"
            >
              {busy ? (
                <Loader2 className="h-4 w-4 animate-spin" />
              ) : (
                <ArrowUp className="h-4 w-4" />
              )}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// Minimal speech types for environments without DOM lib extras
type SpeechRecognition = {
  lang: string;
  interimResults: boolean;
  continuous: boolean;
  onresult: ((ev: SpeechRecognitionEvent) => void) | null;
  onerror: (() => void) | null;
  onend: (() => void) | null;
  start: () => void;
  stop: () => void;
};
type SpeechRecognitionEvent = {
  resultIndex: number;
  results: ArrayLike<{ 0: { transcript: string }; isFinal?: boolean }>;
};
