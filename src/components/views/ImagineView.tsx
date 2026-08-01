import { Download, ImageIcon, Loader2, Sparkles } from "lucide-react";
import { useGrokHub } from "@/lib/store";
import type { ImagineAspect } from "@/lib/types";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

const ASPECTS: ImagineAspect[] = ["1:1", "16:9", "9:16", "3:2", "2:3"];

export function ImagineView() {
  const prompt = useGrokHub((s) => s.imaginePrompt);
  const aspect = useGrokHub((s) => s.imagineAspect);
  const jobs = useGrokHub((s) => s.imagineJobs);
  const busy = useGrokHub((s) => s.imagineBusy);
  const err = useGrokHub((s) => s.imagineError);
  const grokConnected = useGrokHub((s) => s.grokConnected);
  const mode = useGrokHub((s) => s.mode);
  const setImaginePrompt = useGrokHub((s) => s.setImaginePrompt);
  const setImagineAspect = useGrokHub((s) => s.setImagineAspect);
  const runImagine = useGrokHub((s) => s.runImagine);

  const latest = jobs[0];

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-sm">
            <ImageIcon className="h-4 w-4" />
            Imagine
          </CardTitle>
          <CardDescription>
            Live Grok image generation when OAuth/API is connected; local SVG preview as fallback.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-wrap gap-2">
            {ASPECTS.map((a) => (
              <button
                key={a}
                type="button"
                onClick={() => setImagineAspect(a)}
                className={
                  a === aspect
                    ? "rounded-full border border-[var(--color-border-strong)] bg-[var(--color-elevated)] px-3 py-1.5 text-xs font-medium"
                    : "rounded-full border border-[var(--color-border)] px-3 py-1.5 text-xs text-[var(--color-muted)] hover:border-[var(--color-border-strong)]"
                }
              >
                {a}
              </button>
            ))}
            <Badge className="ml-auto font-mono">{mode}</Badge>
            <Badge variant={grokConnected ? "success" : "default"}>
              {grokConnected ? "live ready" : "local only"}
            </Badge>
          </div>
          {err && (
            <div className="rounded-[var(--radius-sm)] border border-[color-mix(in_oklab,var(--color-warn)_40%,transparent)] bg-[color-mix(in_oklab,var(--color-warn)_10%,transparent)] px-3 py-2 text-xs text-[var(--color-warn)]">
              Live Imagine: {err} — showing local preview.
            </div>
          )}
          <form
            className="flex flex-col gap-2 sm:flex-row"
            onSubmit={(e) => {
              e.preventDefault();
              void runImagine();
            }}
          >
            <Input
              value={prompt}
              onChange={(e) => setImaginePrompt(e.target.value)}
              placeholder="Moody night desk, dual monitors, soft amber lamp, film still…"
              disabled={busy}
            />
            <Button type="submit" disabled={busy || !prompt.trim()} className="sm:w-36">
              {busy ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Render
                </>
              ) : (
                <>
                  <Sparkles className="h-4 w-4" />
                  Generate
                </>
              )}
            </Button>
          </form>
        </CardContent>
      </Card>

      {latest?.imageDataUrl && latest.status === "ready" && (
        <Card className="overflow-hidden">
          <CardHeader className="flex-row items-center justify-between gap-3 space-y-0">
            <div>
              <CardTitle className="text-sm">Latest</CardTitle>
              <CardDescription className="line-clamp-1">{latest.prompt}</CardDescription>
            </div>
            <a
              href={latest.imageDataUrl}
              download={`grokhub-imagine-${latest.id}.${latest.imageDataUrl.startsWith("data:image/svg") ? "svg" : "png"}`}
              className="inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]"
            >
              <Download className="h-3.5 w-3.5" />
              Save
            </a>
          </CardHeader>
          <CardContent>
            <div className="overflow-hidden rounded-[var(--radius-md)] border border-[var(--color-border)] bg-[var(--color-surface)]">
              <img
                src={latest.imageDataUrl}
                alt={latest.prompt}
                className="mx-auto max-h-[min(70vh,640px)] w-full object-contain"
              />
            </div>
          </CardContent>
        </Card>
      )}

      <div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {jobs.map((job) => (
          <Card key={job.id} className="overflow-hidden">
            <div className="aspect-video bg-[var(--color-surface)]">
              {job.imageDataUrl ? (
                <img
                  src={job.imageDataUrl}
                  alt={job.prompt}
                  className="h-full w-full object-cover"
                />
              ) : (
                <div className="flex h-full items-center justify-center text-xs text-[var(--color-subtle)]">
                  {job.status === "rendering" ? "Rendering…" : "Queued"}
                </div>
              )}
            </div>
            <CardContent className="space-y-1 p-3">
              <div className="flex items-center justify-between gap-2">
                <Badge variant={job.status === "ready" ? "success" : "info"}>
                  {job.status}
                </Badge>
                <RelativeTime
                  ts={job.ts}
                  className="text-[10px] text-[var(--color-subtle)]"
                />
              </div>
              <p className="line-clamp-2 text-xs text-[var(--color-muted)]">{job.prompt}</p>
              <p className="font-mono text-[10px] text-[var(--color-subtle)]">
                {job.aspect} · {job.mode}
              </p>
            </CardContent>
          </Card>
        ))}
      </div>

      {jobs.length === 0 && (
        <Card>
          <CardContent className="py-12 text-center text-sm text-[var(--color-muted)]">
            No renders yet. Describe a scene and hit Generate.
          </CardContent>
        </Card>
      )}
    </div>
  );
}
