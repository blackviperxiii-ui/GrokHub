/**
 * Convert chat messages (string content + optional images[]) into xAI vision parts.
 * Caps to the most recent N images so computer-use screenshots do not blow the context.
 */

const DATA_URL_RE = /!\[[^\]]*\]\((data:image\/[a-zA-Z0-9.+-]+;base64,[A-Za-z0-9+/=\s]+)\)/g;
const BARE_DATA_RE = /(data:image\/[a-zA-Z0-9.+-]+;base64,[A-Za-z0-9+/=\s]{80,})/g;

function clipDataUrl(url) {
  const s = String(url || "").replace(/\s+/g, "");
  if (!s.startsWith("data:image/")) return "";
  // ~4MB decoded is far too large for chat; keep a hard cap on the string
  if (s.length > 2_500_000) return s.slice(0, 2_500_000);
  return s;
}

function extractMarkdownImages(content) {
  const text = String(content || "");
  const urls = [];
  for (const m of text.matchAll(DATA_URL_RE)) {
    const u = clipDataUrl(m[1]);
    if (u) urls.push(u);
  }
  if (!urls.length) {
    for (const m of text.matchAll(BARE_DATA_RE)) {
      const u = clipDataUrl(m[1]);
      if (u) urls.push(u);
    }
  }
  const stripped = text
    .replace(DATA_URL_RE, "[screenshot attached]")
    .replace(BARE_DATA_RE, "[screenshot attached]");
  return { text: stripped, urls };
}

function asParts(msg) {
  if (Array.isArray(msg && msg.content)) return { role: msg.role, content: msg.content };
  const fromField = Array.isArray(msg && msg.images)
    ? msg.images.map(clipDataUrl).filter(Boolean)
    : [];
  const extracted = extractMarkdownImages(msg && msg.content);
  const urls = [...fromField, ...extracted.urls];
  const text = extracted.text || (typeof msg.content === "string" ? msg.content : "");
  if (!urls.length) {
    return { role: msg.role, content: text };
  }
  const parts = [];
  if (String(text).trim()) {
    parts.push({ type: "text", text: String(text) });
  } else {
    parts.push({ type: "text", text: "Screenshot of the user's desktop." });
  }
  for (const url of urls) {
    parts.push({ type: "image_url", image_url: { url, detail: "low" } });
  }
  return { role: msg.role, content: parts };
}

function imageCount(msg) {
  if (Array.isArray(msg && msg.content)) {
    return msg.content.filter((p) => p && p.type === "image_url").length;
  }
  return 0;
}

function stripImages(msg) {
  if (!Array.isArray(msg && msg.content)) return msg;
  const textParts = msg.content.filter((p) => p && p.type === "text");
  const text =
    textParts.map((p) => p.text || "").join("\n").trim() ||
    "[previous screenshot omitted]";
  return { role: msg.role, content: text };
}

/**
 * @param {Array<{role:string, content?: unknown, images?: string[]}>} messages
 * @param {{ maxImages?: number }} [opts]
 */
function hydrateForXai(messages, opts = {}) {
  const maxImages = Number.isFinite(opts.maxImages) ? Number(opts.maxImages) : 2;
  const mapped = (messages || []).map((m) => asParts(m || { role: "user", content: "" }));
  let remaining = 0;
  for (const m of mapped) remaining += imageCount(m);
  if (remaining <= maxImages) return mapped;
  const out = mapped.slice();
  for (let i = 0; i < out.length && remaining > maxImages; i++) {
    const n = imageCount(out[i]);
    if (!n) continue;
    out[i] = stripImages(out[i]);
    remaining -= n;
  }
  return out;
}

function messageHasImages(messages) {
  for (const m of messages || []) {
    if (Array.isArray(m && m.images) && m.images.length) return true;
    if (Array.isArray(m && m.content) && m.content.some((p) => p && p.type === "image_url")) {
      return true;
    }
    if (typeof (m && m.content) === "string" && /data:image\//.test(m.content)) return true;
  }
  return false;
}

module.exports = {
  hydrateForXai,
  messageHasImages,
  clipDataUrl,
};
