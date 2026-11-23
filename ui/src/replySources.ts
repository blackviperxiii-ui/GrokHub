// The "Sources" list at the end of a web-search reply, ported from
// grokhub_core::reply_sources (split_sources), which stays the source of
// truth; replySources.fixtures.json holds what the Rust code returns. The
// model ends the answer with a `Sources:` heading and one link per line (or
// the links on the heading's own line); the chat shows them as chips with the
// site name instead of a column of raw URLs.

/** One link from the list: the full URL and the site name its chip shows. */
export interface ReplySource {
  url: string;
  site: string;
}

// Rust's whitespace and trims, by Unicode property, so both split the same way.
// The trims scan code units instead of an anchored regex: `\p{White_Space}+$`
// retries at every spot in an inner run of spaces, which is quadratic on a
// long reply. Every White_Space code point is in the BMP, so one unit is one.
const WS = /^\p{White_Space}$/u;
const isWs = (c: string) => WS.test(c);
const WS_START = /^\p{White_Space}/u;
function trimStart(s: string): string {
  let i = 0;
  while (i < s.length && isWs(s[i])) i++;
  return s.slice(i);
}
function trimEnd(s: string): string {
  let i = s.length;
  while (i > 0 && isWs(s[i - 1])) i--;
  return s.slice(0, i);
}
const trim = (s: string) => trimEnd(trimStart(s));
const asciiLower = (s: string) => s.replace(/[A-Z]/g, (c) => c.toLowerCase());
const utf8Len = (s: string) => new TextEncoder().encode(s).length;

function trimStartChars(s: string, chars: string): string {
  let i = 0;
  while (i < s.length && chars.includes(s[i])) i++;
  return s.slice(i);
}

function trimEndChars(s: string, chars: string): string {
  let i = s.length;
  while (i > 0 && chars.includes(s[i - 1])) i--;
  return s.slice(0, i);
}

/** Split `body` into the answer and its trailing Sources list. `null` when the
 * reply doesn't end in one, so the bubble shows exactly as before. */
export function splitSources(body: string): { answer: string; sources: ReplySource[] } | null {
  const trimmed = trimEnd(body);
  const lines = lineStarts(trimmed);
  const urls: string[] = [];
  let i = lines.length;
  while (i > 0) {
    const line = trim(lines[i - 1][1]);
    if (line) {
      const url = listUrl(line);
      if (url === null) break;
      urls.push(url);
    }
    i -= 1;
  }
  if (i === 0) return null;
  const [at, line] = lines[i - 1];
  let cut: number;
  let inline: string;
  if (urls.length === 0) {
    // "… Sources: url url" on the last line, after the answer or alone.
    const idx = asciiLower(line).lastIndexOf("sources");
    if (idx < 0) return null;
    let lead = line.slice(0, idx);
    if (!(lead === "" || " *_#".includes(lead[lead.length - 1]))) return null;
    const rest = headingRest(line.slice(idx));
    if (rest === null) return null;
    lead = trimEnd(trimEndChars(trimEnd(lead), "*_#"));
    cut = at + lead.length;
    inline = rest;
  } else {
    urls.reverse();
    const rest = headingRest(trimStart(trimStartChars(trim(line), "#")));
    if (rest === null) return null;
    cut = at;
    inline = rest;
  }
  if (inline) {
    if (urls.length > 0) return null;
    for (const token of inline.split(/[ ,;]/).filter((t) => t !== "")) {
      const url = listUrl(token);
      if (url === null) return null;
      urls.push(url);
    }
  }
  const sources: ReplySource[] = [];
  for (const url of urls) {
    if (sources.some((s) => s.url === url)) continue;
    const site = sourceHost(url);
    if (site === null) return null;
    sources.push({ url, site });
  }
  if (sources.length === 0) return null;
  return { answer: trimEnd(trimmed.slice(0, cut)), sources };
}

/** Each line with where it starts; a line's trailing `\r`s go. */
function lineStarts(text: string): [number, string][] {
  let at = 0;
  return text.split("\n").map((line) => {
    const start = at;
    at += line.length + 1;
    return [start, trimEndChars(line, "\r")];
  });
}

/** The text after a `Sources:` heading (`Sources`, `**Sources:**`), or `null`
 * when the line isn't one. */
function headingRest(text: string): string | null {
  const line = trimStartChars(text, "*_");
  if (line.length < 7 || asciiLower(line.slice(0, 7)) !== "sources") return null;
  let rest = trimStartChars(line.slice(7), "*_");
  if (rest.startsWith(":")) rest = rest.slice(1);
  rest = trimStartChars(rest, "*_");
  return rest === "" || WS_START.test(rest) ? trim(rest) : null;
}

/** The link on one list line: `1. url`, `- [Title](url)`, `<url>`, or a bare
 * URL, optionally followed by a title. `null` for anything else. */
function listUrl(text: string): string | null {
  let rest = trim(text);
  if (rest.startsWith("-") || rest.startsWith("*") || rest.startsWith("•")) {
    rest = trimStart(rest.slice(1));
  } else {
    const digits = /^[0-9]*/.exec(rest)?.[0].length ?? 0;
    if (digits > 0) {
      const after = rest.slice(digits);
      if (after.startsWith(".") || after.startsWith(")")) rest = trimStart(after.slice(1));
    }
  }
  let url: string;
  if (rest.startsWith("[")) {
    const close = rest.indexOf("](", 1);
    if (close < 0) return null;
    url = rest.slice(close + 2).split(")")[0];
  } else {
    url = (rest.startsWith("<") ? rest.slice(1) : rest).split(/[ >\t]/)[0];
  }
  url = trimEndChars(url, ".,;");
  const ok = (url.startsWith("https://") || url.startsWith("http://")) && utf8Len(url) > 8;
  return ok ? url : null;
}

/** grokhub_core::pulse::source_host: the lowercase host without `www.`. */
export function sourceHost(url: string): string | null {
  const rest = url.startsWith("https://") ? url.slice(8) : url.startsWith("http://") ? url.slice(7) : null;
  if (rest === null) return null;
  const lower = asciiLower(rest.split(/[/?#:]/)[0]);
  const host = lower.startsWith("www.") ? lower.slice(4) : lower;
  return host ? host : null;
}
