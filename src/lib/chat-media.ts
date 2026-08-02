/**
 * Offload large chat image data-URLs to Electron userData (via imagine media store)
 * so thread JSON stays small. Protocol: grokhub-media:<relPath>
 */
import { loadImagineMedia, persistImagineMedia } from "./imagine-media";

export const MEDIA_PROTO = "grokhub-media:";
const DATA_URL_RE = /!\[([^\]]*)\]\((data:image\/[^;]+;base64,[A-Za-z0-9+/=\s]+)\)/g;
const MEDIA_REF_RE = /!\[([^\]]*)\]\(grokhub-media:([^)\s]+)\)/g;
/** Offload when base64 payload is roughly > 24KB */
const OFFLOAD_MIN_CHARS = 32_000;

export function isMediaRef(src: string | undefined | null): boolean {
  return Boolean(src && src.startsWith(MEDIA_PROTO));
}

export function mediaRelPath(src: string): string {
  return src.slice(MEDIA_PROTO.length);
}

/** Replace large embedded images with disk refs (desktop). No-op in browser. */
export async function compactMessageMedia(content: string): Promise<string> {
  if (!content || content.length < OFFLOAD_MIN_CHARS) return content;
  if (typeof window === "undefined" || !window.grokhubDesktop?.imagineMedia?.save) {
    return content;
  }
  let out = content;
  const matches = [...content.matchAll(DATA_URL_RE)];
  for (let i = 0; i < matches.length; i++) {
    const m = matches[i]!;
    const alt = m[1] || "image";
    const dataUrl = m[2]!.replace(/\s+/g, "");
    if (dataUrl.length < OFFLOAD_MIN_CHARS) continue;
    const id = `chat-att-${Date.now()}-${i}-${Math.random().toString(36).slice(2, 7)}`;
    const saved = await persistImagineMedia(id, dataUrl, "image");
    if (saved?.relPath) {
      out = out.replace(m[0], `![${alt}](${MEDIA_PROTO}${saved.relPath})`);
    }
  }
  return out;
}

/** Expand media refs back to data URLs for model / export. */
export async function expandMessageMedia(content: string): Promise<string> {
  if (!content || !content.includes(MEDIA_PROTO)) return content;
  let out = content;
  const matches = [...content.matchAll(MEDIA_REF_RE)];
  for (const m of matches) {
    const alt = m[1] || "image";
    const rel = m[2]!;
    const dataUrl = await loadImagineMedia(rel);
    if (dataUrl) {
      out = out.replace(m[0], `![${alt}](${dataUrl})`);
    }
  }
  return out;
}

/** Resolve a single img src for display (async). */
export async function resolveMediaSrc(src: string | undefined): Promise<string | undefined> {
  if (!src) return undefined;
  if (isMediaRef(src)) {
    return (await loadImagineMedia(mediaRelPath(src))) || src;
  }
  return src;
}
