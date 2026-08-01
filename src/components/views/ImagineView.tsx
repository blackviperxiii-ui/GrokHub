import { Download, ImageIcon, Loader2, Sparkles } from "lucide-react";
import { useGrokClaw } from "@/lib/store";
import type { ImagineAspect } from "@/lib/types";
import { RelativeTime } from "../RelativeTime";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";

const ASPECTS: ImagineAspect[] = ["1:1", "16:9", "9:16", "3:2", "2:3"];

export function ImagineView() {
  const prompt = useGrokClaw((s) => s.imaginePrompt);
  const aspect = useGrokClaw((s) => s.imagineAspect);
  const jobs = useGrokClaw((s) => s.imagineJobs);
  const running = useGrokClaw((s) => s.running);
  const mode = useGrokClaw((s) => s.mode);
  const setImaginePrompt = useGrokClaw((s) => s.setImaginePrompt);
  const setImagineAspect = useGrokClaw((s) => s.setImagineAspect);
  const runImagine = useGrokClaw((s) => s.runImagine);

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
            Baked into GrokClaw desktop — local preview renderer for Arch offline use.
            Pair with Expert/Heavy modes for stronger art direction in chat.
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
          </div>
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

              disabled={running}
            />
            <Button type="submit" disabled={running || !prompt.trim()} className="sm:w-36">
              {running ? (
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
              download={`grokclaw-imagine-${latest.id}.svg`}
              className="inline-flex h-9 items-center gap-2 rounded-[var(--radius-sm)] border border-[var(--color-border)] bg-[var(--color-elevated)] px-3 text-xs font-medium hover:border-[var(--color-border-strong)]"
            >
              <Download className="h-3.5 w-3.5" />
              Save SVG
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
