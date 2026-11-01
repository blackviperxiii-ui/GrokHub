import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

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
      rule(css, "::view-transition-group(*),\n::view-transition-old(*),\n::view-transition-new(*) {"),
    ).toBe(
      "::view-transition-group(*), ::view-transition-old(*), ::view-transition-new(*) { animation-duration: 150ms; animation-timing-function: var(--ease-out); }",
    );
  });

  it("turns them off for reduced motion", () => {
    const reduced = mediaBlocks(css, "(prefers-reduced-motion: reduce)").find((b) =>
      b.includes("::view-transition-group(*)"),
    );
    expect(rule(reduced ?? "", "::view-transition-group(*)")).toBe(
      "::view-transition-group(*), ::view-transition-old(*), ::view-transition-new(*) { animation: none !important; }",
    );
  });
});
