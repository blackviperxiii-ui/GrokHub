/**
 * Client helpers: persist Imagine media to Electron userData (survives updates).
 */

function api() {
  return typeof window !== "undefined" ? window.grokhubDesktop?.imagineMedia : undefined;
}

export async function persistImagineMedia(
  jobId: string,
  dataUrl: string | undefined,
  kind: "image" | "video",
): Promise<{ relPath?: string; dataUrl?: string } | null> {
  if (!dataUrl) return null;
  const m = api();
  if (!m?.save) {
    // Browser preview: keep data URL in memory only
    return { dataUrl };
  }
  try {
    const r = await m.save(jobId, dataUrl, kind);
    if (r?.ok && r.relPath) {
      // For remote URL pointers, surface the url as dataUrl for immediate display
      return { relPath: r.relPath, dataUrl: r.url || dataUrl };
    }
    return { dataUrl };
  } catch {
    return { dataUrl };
  }
}

export async function loadImagineMedia(relPath?: string | null): Promise<string | undefined> {
  if (!relPath) return undefined;
  // Already a data/http URL stored by mistake
  if (/^(data:|https?:)/i.test(relPath)) return relPath;
  const m = api();
  if (!m?.load) return undefined;
  try {
    const r = await m.load(relPath);
    if (r?.ok && r.dataUrl) return r.dataUrl;
  } catch {
    /* ignore */
  }
  return undefined;
}

export async function deleteImagineMedia(jobId: string): Promise<void> {
  const m = api();
  if (!m?.delete) return;
  try {
    await m.delete(jobId);
  } catch {
    /* ignore */
  }
}

export async function clearImagineMedia(): Promise<void> {
  const m = api();
  if (!m?.clear) return;
  try {
    await m.clear();
  } catch {
    /* ignore */
  }
}

/** Rehydrate jobs that only have disk paths after restart/update */
export async function rehydrateImagineJobs<
  T extends {
    id: string;
    imageDataUrl?: string;
    videoDataUrl?: string;
    imageRelPath?: string;
    videoRelPath?: string;
  },
>(jobs: T[]): Promise<T[]> {
  const out: T[] = [];
  for (const j of jobs) {
    let imageDataUrl = j.imageDataUrl;
    let videoDataUrl = j.videoDataUrl;
    if (!imageDataUrl && j.imageRelPath) {
      imageDataUrl = await loadImagineMedia(j.imageRelPath);
    }
    if (!videoDataUrl && j.videoRelPath) {
      videoDataUrl = await loadImagineMedia(j.videoRelPath);
    }
    out.push({ ...j, imageDataUrl, videoDataUrl });
  }
  return out;
}
