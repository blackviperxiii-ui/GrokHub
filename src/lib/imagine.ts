import type { ImagineAspect } from "./types";

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

function dims(aspect: ImagineAspect): { w: number; h: number } {
  switch (aspect) {
    case "16:9":
      return { w: 960, h: 540 };
    case "9:16":
      return { w: 540, h: 960 };
    case "3:2":
      return { w: 900, h: 600 };
    case "2:3":
      return { w: 600, h: 900 };
    default:
      return { w: 768, h: 768 };
  }
}

function escapeXml(s: string): string {
  return Array.from(s)
    .map((ch) => {
      if (ch === "&") return "&" + "amp;";
      if (ch === "<") return "&" + "lt;";
      if (ch === ">") return "&" + "gt;";
      if (ch === '"') return "&" + "quot;";
      return ch;
    })
    .join("");
}

/** Local offline Imagine preview (SVG) — desktop-ready without API keys. */
export function renderImaginePreview(prompt: string, aspect: ImagineAspect): string {
  const { w, h } = dims(aspect);
  const h1 = hash(prompt);
  const h2 = hash(prompt + "::b");
  const h3 = hash(prompt + "::c");
  const c1 = `hsl(${h1 % 360} 18% 12%)`;
  const c2 = `hsl(${h2 % 360} 22% 18%)`;
  const c3 = `hsl(${h3 % 360} 28% 42%)`;
  const accent = `hsl(${(h1 + 40) % 360} 35% 68%)`;
  const title = escapeXml(prompt.slice(0, 72) || "Imagine");
  const sub = "GrokHub · Imagine · local preview";

  const blobs = Array.from({ length: 5 }, (_, i) => {
    const hx = hash(`${prompt}-blob-${i}`);
    const cx = (hx % 80) + 10;
    const cy = ((hx >> 8) % 80) + 10;
    const r = ((hx >> 16) % 28) + 12;
    const op = 0.12 + ((hx >> 24) % 20) / 100;
    return `<circle cx="${(cx / 100) * w}" cy="${(cy / 100) * h}" r="${(r / 100) * Math.min(w, h)}" fill="${accent}" opacity="${op}"/>`;
  }).join("");

  const svg = [
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">`,
    `<defs>`,
    `<linearGradient id="g" x1="0" y1="0" x2="1" y2="1">`,
    `<stop offset="0%" stop-color="${c1}"/>`,
    `<stop offset="55%" stop-color="${c2}"/>`,
    `<stop offset="100%" stop-color="${c3}"/>`,
    `</linearGradient>`,
    `<radialGradient id="v" cx="50%" cy="40%" r="65%">`,
    `<stop offset="0%" stop-color="${accent}" stop-opacity="0.35"/>`,
    `<stop offset="100%" stop-color="${c1}" stop-opacity="0"/>`,
    `</radialGradient>`,
    `</defs>`,
    `<rect width="100%" height="100%" fill="url(#g)"/>`,
    `<rect width="100%" height="100%" fill="url(#v)"/>`,
    blobs,
    `<rect x="24" y="${h - 108}" width="${w - 48}" height="72" rx="14" fill="rgba(10,10,11,0.55)" stroke="rgba(244,244,245,0.12)"/>`,
    `<text x="44" y="${h - 68}" fill="#f4f4f5" font-family="Segoe UI, system-ui, sans-serif" font-size="18" font-weight="600">${title}</text>`,
    `<text x="44" y="${h - 42}" fill="#a1a1aa" font-family="ui-monospace, SF Mono, Menlo, monospace" font-size="12">${sub}</text>`,
    `<text x="${w - 44}" y="40" text-anchor="end" fill="#a1a1aa" font-family="ui-monospace, SF Mono, Menlo, monospace" font-size="11">IMAGINE</text>`,
    `</svg>`,
  ].join("");

  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
