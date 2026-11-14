import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

/**
 * Every color in styles.css comes from a theme token: a hex, rgb() or hsl()
 * literal may only sit in a `:root` theme block. Outside them, two kinds are
 * allowed: plain black (shadows, scrims and masks, which carry no hue) and the
 * generated cover art below.
 */

/** Rules whose literals are art, not chrome: the placeholder covers mix hues from --art-hue. */
const ART = new Set([".library-art.placeholder"]);

/** A theme block's own selector: `:root`, maybe with attribute or :not() filters, no descendants. */
const THEME_HEAD = /^:root(\[[^\]]*\]|:not\([^)]*\))*$/;
const LITERAL = /#[0-9a-fA-F]{3,8}\b|rgba?\([^)]*\)|hsla?\(/g;
const BLACK = /^(#000|#000000|rgba?\(\s*0[\s,]+0[\s,]+0\s*([,/][^)]*)?\))$/;

type Hit = { line: number; literal: string; selector: string };

/** Color literals outside the `:root` blocks, with the line and selector each sits in. */
export function strayColors(source: string): Hit[] {
  // Blank out comments but keep their newlines, so line numbers stay right.
  const text = source.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
  const stack: string[] = [];
  const hits: Hit[] = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      stack.push(text.slice(start, i).split(";").pop()!.trim());
      start = i + 1;
    } else if (ch === "}") {
      const theme = stack.some((head) => head.split(",").every((s) => THEME_HEAD.test(s.trim())));
      const selector = stack[stack.length - 1] ?? "";
      if (!theme && !ART.has(selector)) {
        const body = text.slice(start, i);
        for (const m of body.matchAll(LITERAL)) {
          if (BLACK.test(m[0])) continue;
          hits.push({ line: text.slice(0, start + m.index).split("\n").length, literal: m[0], selector });
        }
      }
      stack.pop();
      start = i + 1;
    }
  }
  return hits;
}

describe("color tokens", () => {
  it("styles.css keeps color literals in the theme blocks", () => {
    expect(strayColors(css)).toEqual([]);
  });

  it("flags a literal in a component rule and lets theme blocks, black and art through", () => {
    const sample = [
      ":root { --red: #d7263d; }",
      ':root[data-theme="oled-black"] { --bg: #000000; }',
      ".card { box-shadow: 0 1px 2px rgb(0 0 0 / 0.3); }",
      ".library-art.placeholder { background: hsl(var(--art-hue) 40% 20%); }",
      ".badge {",
      "  color: #f0b429;",
      "}",
      '@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { --red: #d7263d; } }',
      ':root[data-theme="soft-charcoal"] .queued { color: #9a9a9a; }',
    ].join("\n");
    expect(strayColors(sample)).toEqual([
      { line: 6, literal: "#f0b429", selector: ".badge" },
      { line: 9, literal: "#9a9a9a", selector: ':root[data-theme="soft-charcoal"] .queued' },
    ]);
  });
});
