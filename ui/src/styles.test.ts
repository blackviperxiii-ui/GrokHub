import { describe, expect, it } from "vitest";
import RAW_CSS from "./styles.css?raw";

// A Windows checkout reads the file with CRLF; the multi-line selectors
// below are matched with LF.
const CSS = RAW_CSS.replace(/\r\n/g, "\n");

/** The body of every top-level `@media` block whose query is exactly `query`. */
function mediaBlocks(sheet: string, query: string): string[] {
  const text = sheet.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: string[] = [];
  let from = text.indexOf(`@media ${query} {`);
  while (from >= 0) {
    const open = text.indexOf("{", from);
    let depth = 0;
    for (let i = open; i < text.length; i++) {
      if (text[i] === "{") depth++;
      else if (text[i] === "}" && --depth === 0) {
        out.push(text.slice(open + 1, i));
        break;
      }
    }
    from = text.indexOf(`@media ${query} {`, open);
  }
  return out;
}

/** `selector { … }` inside `block`, normalised to one line, or "" if absent. */
function rule(block: string, selector: string): string {
  const at = block.indexOf(selector);
  if (at < 0) return "";
  return block
    .slice(at, block.indexOf("}", at) + 1)
    .replace(/\s+/g, " ")
    .trim();
}

describe("styles.css view transitions", () => {
  it("eases cards' view transitions over 150ms", () => {
    expect(
      rule(CSS, "::view-transition-group(*),\n::view-transition-old(*),\n::view-transition-new(*) {"),
    ).toBe(
      "::view-transition-group(*), ::view-transition-old(*), ::view-transition-new(*) { animation-duration: 150ms; animation-timing-function: var(--ease-out); }",
    );
  });

  it("turns them off for reduced motion", () => {
    const reduced = mediaBlocks(CSS, "(prefers-reduced-motion: reduce)").find((b) =>
      b.includes("::view-transition-group(*)"),
    );
    expect(rule(reduced ?? "", "::view-transition-group(*)")).toBe(
      "::view-transition-group(*), ::view-transition-old(*), ::view-transition-new(*) { animation: none !important; }",
    );
  });
});

// The OS-dark default (`prefers-color-scheme: dark` with no theme picked) has
// its own copy of the Ops Dark colours, which contrast.test.ts doesn't read
// (it skips @media blocks); it must not drift from the theme.

/** The `--name: value;` pairs declared in the last block whose selector list is `selector`. */
function block(selector: string): Record<string, string> {
  const start = CSS.lastIndexOf(`${selector} {`);
  if (start < 0) throw new Error(`no block for ${selector}`);
  const body = CSS.slice(start, CSS.indexOf("}", start));
  const vars: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) vars[m[1]] = m[2].trim();
  return vars;
}

const PALETTE = block(':root,\n:root[data-theme="dark"],\n:root[data-theme="ops-dark"]');

/** A colour, following `var(--x)` through the Ops palette. */
function resolve(vars: Record<string, string>, name: string): string {
  const value = vars[name] ?? PALETTE[name];
  if (!value) throw new Error(`${name} is not defined`);
  const ref = /^var\((--[\w-]+)\)$/.exec(value);
  return ref ? resolve(vars, ref[1]) : value;
}

describe("styles.css OS-dark fallback", () => {
  it("matches the Ops Dark theme", () => {
    const fallback = block(':root:not([data-theme="light"])');
    const ops = block(':root[data-theme="dark"],\n:root[data-theme="ops-dark"]');
    for (const name of ["--bg", "--panel", "--text", "--muted", "--danger", "--danger-text"]) {
      expect(resolve(fallback, name), name).toBe(resolve(ops, name));
    }
  });
});
