// Reply Markdown for the shell: a port of the cabin's parser in
// crates/grokhub-core/src/md.rs (md_blocks, md_spans, md_link_ok, code_tokens),
// so both apps agree on what a table, a numbered list or a link is. It returns
// plain data; Markdown.tsx turns that into React elements, never HTML strings.
// md.fixtures.json holds the Rust parser's output for the same inputs, and
// md.test.ts fails if this port drifts from it.

export type MdAlign = "left" | "center" | "right";

export type MdBlock =
  | { kind: "heading"; level: number; text: string }
  | { kind: "bullet"; depth: number; text: string }
  | { kind: "numbered"; depth: number; num: string; text: string }
  | { kind: "task"; depth: number; done: boolean; text: string }
  | { kind: "quote"; text: string }
  | { kind: "rule" }
  | { kind: "table"; header: string[]; align: MdAlign[]; rows: string[][] }
  | { kind: "code"; lang: string; body: string }
  | { kind: "para"; text: string }
  | { kind: "blank" };

export type MdSpan =
  | { kind: "text"; text: string }
  | { kind: "bold"; text: string }
  | { kind: "italic"; text: string }
  | { kind: "strike"; text: string }
  | { kind: "code"; text: string }
  /** Only http(s) that passes `mdLinkOk`. Anything else stays text. */
  | { kind: "link"; text: string; url: string };

export type CodeTok = "plain" | "keyword" | "str" | "comment" | "number";

/** The cabin's TEXT_FILE_CAP (crates/grokhub-core/src/attach.rs), in UTF-8 bytes. */
export const TEXT_FILE_CAP = 64 * 1024;

// Rust's char classes, by Unicode property, so the two parsers split the same way.
const WS = /^\p{White_Space}$/u;
const ALNUM = /^[\p{Alphabetic}\p{N}]$/u;
const ALPHA = /^\p{Alphabetic}$/u;
const CONTROL = /^\p{Cc}$/u;

const isWs = (c: string | undefined) => c !== undefined && WS.test(c);
const isAlnum = (c: string | undefined) => c !== undefined && ALNUM.test(c);
const isAsciiDigit = (c: string | undefined) => c !== undefined && c >= "0" && c <= "9";
const isAsciiAlnum = (c: string | undefined) => c !== undefined && /^[0-9A-Za-z]$/.test(c);

const trimStart = (s: string) => s.replace(/^\p{White_Space}+/u, "");
const trimEnd = (s: string) => s.replace(/\p{White_Space}+$/u, "");
const trim = (s: string) => trimEnd(trimStart(s));
const asciiLower = (s: string) => s.replace(/[A-Z]/g, (c) => c.toLowerCase());

/** The code point that starts at UTF-16 index `i`. */
function charAt(s: string, i: number): string | undefined {
  const cp = s.codePointAt(i);
  return cp === undefined ? undefined : String.fromCodePoint(cp);
}

/** The code point that ends just before UTF-16 index `i`. */
function charBefore(s: string, i: number): string | undefined {
  if (i <= 0) return undefined;
  const lo = s.charCodeAt(i - 1);
  if (i >= 2 && lo >= 0xdc00 && lo <= 0xdfff) {
    const hi = s.charCodeAt(i - 2);
    if (hi >= 0xd800 && hi <= 0xdbff) return s.slice(i - 2, i);
  }
  return s[i - 1];
}

/** Rust's `str::lines`: split on `\n`, drop a `\r` before it, no empty last line. */
export function mdLines(text: string): string[] {
  if (text === "") return [];
  const parts = text.split("\n");
  if (parts[parts.length - 1] === "") parts.pop();
  const last = parts.length - 1;
  return parts.map((p, i) => (i < last || text.endsWith("\n")) && p.endsWith("\r") ? p.slice(0, -1) : p);
}

/** The reply as the cabin shows it: at most TEXT_FILE_CAP UTF-8 bytes, cut on a character. */
export function displayText(text: string): string {
  // A UTF-16 unit is at most 3 UTF-8 bytes (a surrogate pair is 4 for 2 units).
  if (text.length * 3 <= TEXT_FILE_CAP) return text;
  let bytes = 0;
  for (let i = 0; i < text.length; ) {
    const cp = text.codePointAt(i)!;
    const width = cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
    if (bytes + width > TEXT_FILE_CAP) return text.slice(0, i);
    bytes += width;
    i += cp > 0xffff ? 2 : 1;
  }
  return text;
}

function fenceOpen(line: string): { marker: string; lang: string } | null {
  const t = trimStart(line);
  let marker: string;
  if (t.startsWith("```")) marker = "```";
  else if (t.startsWith("~~~")) marker = "~~~";
  else return null;
  const ch = marker[0];
  let info = t.slice(3);
  while (info.startsWith(ch)) info = info.slice(1);
  const word = trim(info).split(/\p{White_Space}+/u)[0] ?? "";
  const lang = asciiLower(word.replace(/^[{}.]+|[{}.]+$/g, ""));
  return { marker, lang };
}

function indentDepth(line: string): number {
  let cols = 0;
  for (const c of line) {
    if (c === " ") cols += 1;
    else if (c === "\t") cols += 4;
    else break;
  }
  return Math.min(Math.floor(cols / 2), 6);
}

function heading(line: string): { level: number; text: string } | null {
  const t = trimStart(line);
  let hashes = 0;
  while (t[hashes] === "#") hashes++;
  if (hashes === 0 || hashes > 6) return null;
  const rest = t.slice(hashes);
  if (rest !== "" && !rest.startsWith(" ")) return null;
  const text = trimEnd(trim(rest).replace(/#+$/, ""));
  return { level: hashes, text };
}

function isRule(line: string): boolean {
  const t = line.replace(/\p{White_Space}/gu, "");
  return t.length >= 3 && (/^-+$/.test(t) || /^\*+$/.test(t) || /^_+$/.test(t));
}

function bullet(line: string): { depth: number; rest: string } | null {
  const depth = indentDepth(line);
  const t = trimStart(line);
  for (const m of ["- ", "* ", "+ "]) {
    if (t.startsWith(m)) return { depth, rest: t.slice(m.length) };
  }
  if (t === "-" || t === "*" || t === "+") return { depth, rest: "" };
  return null;
}

function task(text: string): { done: boolean; text: string } | null {
  const head = asciiLower(text.slice(0, 3));
  if (head === "[ ]") return { done: false, text: trimStart(text.slice(3)) };
  if (head === "[x]") return { done: true, text: trimStart(text.slice(3)) };
  return null;
}

function numbered(line: string): { depth: number; num: string; rest: string } | null {
  const depth = indentDepth(line);
  const t = trimStart(line);
  let digits = 0;
  while (isAsciiDigit(t[digits])) digits++;
  if (digits === 0 || digits > 9) return null;
  const after = t.slice(digits);
  let rest: string;
  if (after.startsWith(". ") || after.startsWith(") ")) rest = after.slice(2);
  else if (after === "." || after === ")") rest = "";
  else return null;
  return { depth, num: t.slice(0, digits), rest };
}

/** Split one `| a | b |` row. Escaped `\|` stays in the cell. */
export function mdTableCells(line: string): string[] {
  let t = trim(line);
  if (t.startsWith("|")) t = t.slice(1);
  if (t.endsWith("|")) t = t.slice(0, -1);
  const cells: string[] = [];
  let cur = "";
  let inCode = false;
  for (let i = 0; i < t.length; i++) {
    const c = t[i];
    if (c === "\\" && t[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (c === "`") {
      inCode = !inCode;
      cur += c;
    } else if (c === "|" && !inCode) {
      cells.push(trim(cur));
      cur = "";
    } else {
      cur += c;
    }
  }
  cells.push(trim(cur));
  return cells;
}

function tableAlign(line: string): MdAlign[] | null {
  if (!line.includes("-")) return null;
  const out: MdAlign[] = [];
  for (const cell of mdTableCells(line)) {
    const c = trim(cell);
    const left = c.startsWith(":");
    const right = c.endsWith(":");
    const dashes = c.replace(/^:+|:+$/g, "");
    if (dashes === "" || !/^-+$/.test(dashes)) return null;
    out.push(left && right ? "center" : right ? "right" : "left");
  }
  return out;
}

function looksLikeRow(line: string): boolean {
  const t = trim(line);
  return t.includes("|") && t !== "";
}

/** Walk a reply into blocks. An unclosed fence runs to the end (a live stream). */
export function mdBlocks(text: string): MdBlock[] {
  const lines = mdLines(text);
  const out: MdBlock[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    const fence = fenceOpen(line);
    if (fence) {
      const body: string[] = [];
      i++;
      while (i < lines.length && !trimStart(lines[i]).startsWith(fence.marker)) {
        body.push(lines[i]);
        i++;
      }
      // Skip the closing fence when there is one.
      i++;
      out.push({ kind: "code", lang: fence.lang, body: body.join("\n") });
      continue;
    }
    if (trim(line) === "") {
      out.push({ kind: "blank" });
      i++;
      continue;
    }
    const h = heading(line);
    if (h) {
      out.push({ kind: "heading", level: h.level, text: h.text });
      i++;
      continue;
    }
    if (isRule(line)) {
      out.push({ kind: "rule" });
      i++;
      continue;
    }
    if (looksLikeRow(line) && i + 1 < lines.length) {
      const align = tableAlign(lines[i + 1]);
      const header = mdTableCells(line);
      if (align && header.length === align.length) {
        const rows: string[][] = [];
        i += 2;
        while (i < lines.length && looksLikeRow(lines[i])) {
          const cells = mdTableCells(lines[i]).slice(0, header.length);
          while (cells.length < header.length) cells.push("");
          rows.push(cells);
          i++;
        }
        out.push({ kind: "table", header, align, rows });
        continue;
      }
    }
    if (trimStart(line).startsWith(">")) {
      const quote: string[] = [];
      while (i < lines.length) {
        const t = trimStart(lines[i]);
        if (!t.startsWith(">")) break;
        const rest = t.slice(1);
        quote.push(rest.startsWith(" ") ? rest.slice(1) : rest);
        i++;
      }
      out.push({ kind: "quote", text: quote.join("\n") });
      continue;
    }
    const b = bullet(line);
    if (b) {
      const tk = task(b.rest);
      out.push(
        tk
          ? { kind: "task", depth: b.depth, done: tk.done, text: tk.text }
          : { kind: "bullet", depth: b.depth, text: b.rest },
      );
      i++;
      continue;
    }
    const n = numbered(line);
    if (n) {
      out.push({ kind: "numbered", depth: n.depth, num: n.num, text: n.rest });
      i++;
      continue;
    }
    out.push({ kind: "para", text: line });
    i++;
  }
  return out;
}

/** A link the shell may open. Only http(s), no whitespace, controls or quotes. */
export function mdLinkOk(url: string): boolean {
  const lower = asciiLower(url);
  const host = lower.startsWith("https://")
    ? lower.slice(8)
    : lower.startsWith("http://")
      ? lower.slice(7)
      : null;
  if (host === null || host === "" || host.startsWith("/")) return false;
  for (const c of url) {
    if (WS.test(c) || CONTROL.test(c) || c === '"' || c === "<" || c === ">") return false;
  }
  return true;
}

function pushText(out: MdSpan[], s: string) {
  if (s === "") return;
  const prev = out[out.length - 1];
  if (prev?.kind === "text") prev.text += s;
  else out.push({ kind: "text", text: s });
}

function count(s: string, ch: string): number {
  return s.split(ch).length - 1;
}

/** Length of a bare `https://…` run at the start of `s`, minus trailing punctuation. */
function bareUrlLen(s: string): number | null {
  const head = asciiLower(Array.from(s.slice(0, 16)).slice(0, 8).join(""));
  if (!(head.startsWith("https://") || head.startsWith("http://"))) return null;
  let end = s.length;
  for (let i = 0; i < s.length; i++) {
    const c = s[i];
    if (isWs(c) || c === "<" || c === ">" || c === '"' || c === "`") {
      end = i;
      break;
    }
  }
  // Trailing sentence punctuation is not part of the URL.
  while (end > 0) {
    const last = charBefore(s, end) ?? " ";
    const head = s.slice(0, end);
    const unbalancedParen = last === ")" && count(head, "(") < count(head, ")");
    if (".,;:!?']".includes(last) || unbalancedParen) end -= last.length;
    else break;
  }
  return mdLinkOk(s.slice(0, end)) ? end : null;
}

/** `[text](url)` at the start of `s`: the text, url and how much it used. */
function linkAt(s: string): { text: string; url: string; used: number } | null {
  if (!s.startsWith("[")) return null;
  const rest = s.slice(1);
  const close = rest.indexOf("](");
  if (close < 0) return null;
  const text = rest.slice(0, close);
  if (text.includes("\n")) return null;
  const after = rest.slice(close + 2);
  const end = after.indexOf(")");
  if (end < 0) return null;
  const url = trim(after.slice(0, end)).split(/\p{White_Space}+/u)[0] ?? "";
  if (!mdLinkOk(url)) return null;
  return { text, url, used: 1 + close + 2 + end + 1 };
}

/** Inline spans for one line. Unclosed markers stay text. */
export function mdSpans(line: string): MdSpan[] {
  const out: MdSpan[] = [];
  let i = 0;
  let plainFrom = 0;
  while (i < line.length) {
    const rest = line.slice(i);
    const prev = charBefore(line, i);
    const c = line[i];
    let hit: { span: MdSpan; used: number } | null = null;
    if (c === "`") {
      let ticks = 0;
      while (rest[ticks] === "`") ticks++;
      const fence = rest.slice(0, ticks);
      const end = rest.slice(ticks).indexOf(fence);
      if (end >= 0) {
        hit = { span: { kind: "code", text: trim(rest.slice(ticks, ticks + end)) }, used: ticks + end + ticks };
      }
    } else if ((c === "*" || c === "_") && (rest.startsWith("**") || (rest.startsWith("__") && !isAlnum(prev)))) {
      const m = rest.slice(0, 2);
      const end = rest.slice(2).indexOf(m);
      if (end > 0) hit = { span: { kind: "bold", text: rest.slice(2, 2 + end) }, used: 2 + end + 2 };
    } else if (c === "*" || c === "_") {
      // `snake_case` and `2*3*4` are not emphasis.
      const opens = !isAlnum(prev) || c === "*";
      const next = charAt(rest, 1);
      if (opens && next !== undefined && !isWs(next)) {
        const end = rest.slice(1).indexOf(c);
        if (end >= 0) {
          const inner = rest.slice(1, 1 + end);
          const after = charAt(rest, 1 + end + 1);
          const closes = !isWs(charBefore(inner, inner.length)) && (c === "*" || !isAlnum(after));
          if (end > 0 && closes) hit = { span: { kind: "italic", text: inner }, used: 1 + end + 1 };
        }
      }
    } else if (c === "~" && rest.startsWith("~~")) {
      const end = rest.slice(2).indexOf("~~");
      if (end > 0) hit = { span: { kind: "strike", text: rest.slice(2, 2 + end) }, used: 2 + end + 2 };
    } else if (c === "[") {
      const link = linkAt(rest);
      if (link) hit = { span: { kind: "link", text: link.text, url: link.url }, used: link.used };
    } else if ((c === "h" || c === "H") && !isAlnum(prev)) {
      const len = bareUrlLen(rest);
      if (len !== null) {
        const url = rest.slice(0, len);
        hit = { span: { kind: "link", text: url, url }, used: len };
      }
    }
    if (hit) {
      pushText(out, line.slice(plainFrom, i));
      out.push(hit.span);
      i += hit.used;
      plainFrom = i;
    } else {
      i++;
    }
  }
  pushText(out, line.slice(Math.min(plainFrom, line.length)));
  return out;
}

/** Plain text of a line with markers dropped. */
export function mdPlain(line: string): string {
  return mdSpans(line)
    .map((s) => s.text)
    .join("");
}

const KEYWORDS = new Set([
  // Shared across C-family, Rust, Go, JS/TS, Python, shell.
  "as", "async", "await", "break", "case", "catch", "class", "const", "continue", "def",
  "default", "del", "do", "elif", "else", "enum", "esac", "except", "export", "extends",
  "false", "fi", "finally", "fn", "for", "from", "func", "function", "go", "if", "impl",
  "import", "in", "interface", "is", "lambda", "let", "loop", "match", "mod", "mut", "new",
  "nil", "None", "not", "null", "or", "and", "package", "pass", "pub", "raise", "return",
  "self", "Self", "static", "struct", "super", "switch", "then", "this", "throw", "trait",
  "True", "False", "true", "try", "type", "typeof", "use", "var", "void", "where", "while",
  "with", "yield", "done", "echo", "local", "unsafe", "undefined", "private", "public",
  "protected", "defer", "chan", "select", "range", "map", "crate", "ref", "dyn", "move",
]);

const HASH_LANGS = new Set([
  "py", "python", "sh", "bash", "zsh", "shell", "console", "toml", "yaml", "yml", "rb", "ruby",
  "ps1", "powershell", "pwsh", "r", "dockerfile", "make", "makefile", "ini", "conf",
]);
const DASH_LANGS = new Set(["sql", "lua", "haskell", "hs"]);
const SLASH_LANGS = new Set([
  "rs", "rust", "c", "cpp", "c++", "h", "hpp", "go", "js", "javascript", "ts", "typescript",
  "jsx", "tsx", "java", "kt", "kotlin", "swift", "cs", "csharp", "zig", "dart", "scala",
  "json5", "jsonc",
]);
const NO_COMMENT_LANGS = new Set(["json", "md", "markdown", "text", "txt", "diff", "patch"]);
const PLAIN_LANGS = new Set(["text", "txt", "md", "markdown", "diff", "patch"]);

function lineComment(lang: string): string[] {
  if (HASH_LANGS.has(lang)) return ["#"];
  if (DASH_LANGS.has(lang)) return ["--"];
  if (SLASH_LANGS.has(lang)) return ["//"];
  if (NO_COMMENT_LANGS.has(lang)) return [];
  return ["//", "#"];
}

const isIdent = (c: string | undefined) => isAlnum(c) || c === "_";

/** Colour one code line. `lang` is the fence info word. Unknown languages still
 * get strings, numbers, and `//` / `#` comments. */
export function codeTokens(lang: string, line: string): [CodeTok, string][] {
  const out: [CodeTok, string][] = [];
  const push = (tok: CodeTok, s: string) => {
    if (s === "") return;
    const last = out[out.length - 1];
    if (last && last[0] === tok) last[1] += s;
    else out.push([tok, s]);
  };
  if (PLAIN_LANGS.has(lang)) {
    push("plain", line);
    return out;
  }
  const comments = lineComment(lang);
  // [UTF-16 offset, code point], like Rust's char_indices.
  const chars: [number, string][] = [];
  for (let at = 0; at < line.length; ) {
    const ch = charAt(line, at)!;
    chars.push([at, ch]);
    at += ch.length;
  }
  const offset = (k: number) => (k < chars.length ? chars[k][0] : line.length);
  let k = 0;
  while (k < chars.length) {
    const [i, c] = chars[k];
    const rest = line.slice(i);
    const prev = k > 0 ? chars[k - 1][1] : undefined;
    // `#` inside `a#b` or a shell `$#` is not a comment.
    if (comments.some((m) => rest.startsWith(m) && (m !== "#" || prev === undefined || isWs(prev)))) {
      push("comment", rest);
      return out;
    }
    if (c === '"' || c === "'" || c === "`") {
      // A Rust lifetime `'a` is not a string.
      let lifetime = false;
      if (c === "'" && (lang === "rs" || lang === "rust")) {
        let ident = 0;
        while (k + 1 + ident < chars.length && isIdent(chars[k + 1 + ident][1])) ident++;
        lifetime = ident > 0 && chars[k + 1 + ident]?.[1] !== "'";
      }
      if (!lifetime) {
        let j = k + 1;
        while (j < chars.length) {
          if (chars[j][1] === "\\") {
            j += 2;
            continue;
          }
          if (chars[j][1] === c) break;
          j++;
        }
        push("str", line.slice(i, offset(j + 1)));
        k = Math.min(j + 1, chars.length);
        continue;
      }
    }
    if (isAsciiDigit(c) && !isIdent(prev)) {
      let j = k;
      while (j < chars.length && (isAsciiAlnum(chars[j][1]) || chars[j][1] === "." || chars[j][1] === "_")) j++;
      push("number", line.slice(i, offset(j)));
      k = j;
      continue;
    }
    if (ALPHA.test(c) || c === "_") {
      let j = k;
      while (j < chars.length && isIdent(chars[j][1])) j++;
      const word = line.slice(i, offset(j));
      push(KEYWORDS.has(word) ? "keyword" : "plain", word);
      k = j;
      continue;
    }
    push("plain", c);
    k++;
  }
  return out;
}
