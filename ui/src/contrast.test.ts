import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

/**
 * Locks in the WCAG contrast of the theme colors in styles.css, so a palette
 * tweak that makes text or a field border hard to see fails here, not in review.
 */

type Rule = { selectors: string[]; decls: Record<string, string> };

function parse(css: string): Rule[] {
  const rules: Rule[] = [];
  const text = css.replace(/\/\*[\s\S]*?\*\//g, "");
  let depth = 0;
  let start = 0;
  let head = "";
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      if (depth === 0) {
        head = text.slice(start, i).trim();
        start = i + 1;
      }
      depth++;
    } else if (ch === "}") {
      depth--;
      if (depth === 0) {
        // Only top-level rules; @media and @font-face blocks are skipped.
        if (!head.startsWith("@")) {
          const decls: Record<string, string> = {};
          for (const m of text.slice(start, i).matchAll(/(--[\w-]+)\s*:\s*([^;]+);/g)) {
            decls[m[1]] = m[2].trim();
          }
          rules.push({ selectors: head.split(",").map((s) => s.trim()), decls });
        }
        start = i + 1;
      }
    }
  }
  return rules;
}

function theme(rules: Rule[], id: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const rule of rules) {
    const hit = rule.selectors.some((s) => s === ":root" || s === `:root[data-theme="${id}"]`);
    if (hit) Object.assign(out, rule.decls);
  }
  return out;
}

function resolve(vars: Record<string, string>, name: string): string {
  let value = vars[`--${name}`];
  for (let n = 0; n < 8 && value?.startsWith("var("); n++) {
    value = vars[value.slice(4, -1).trim()];
  }
  if (!/^#[0-9a-f]{6}$/i.test(value ?? "")) throw new Error(`--${name} is not a plain hex color: ${value}`);
  return value;
}

function luminance(hex: string): number {
  const [r, g, b] = [1, 3, 5].map((i) => {
    const c = parseInt(hex.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

const rules = parse(css);
const THEMES = ["ops-dark", "oled-black", "soft-charcoal", "light"];

// [foreground, background, minimum ratio]: 4.5 for text, 3 for the edge of a field or switch.
const PAIRS: [string, string, number][] = [
  ["text", "bg", 4.5],
  ["text", "panel", 4.5],
  ["muted", "bg", 4.5],
  ["muted", "panel", 4.5],
  ["muted", "raised", 4.5],
  ["steel", "panel", 4.5],
  // Code in a reply: strings and numbers on the code well.
  ["steel", "bg", 4.5],
  ["accent-text", "accent", 4.5],
  ["danger-text", "bg", 4.5],
  ["danger-text", "panel", 4.5],
  ["danger-text", "raised", 4.5],
  ["accent", "bg", 3],
  ["accent", "panel", 3],
  ["field-border", "bg", 3],
  ["field-border", "panel", 3],
];

describe("theme contrast", () => {
  for (const id of THEMES) {
    const vars = theme(rules, id);
    for (const [fg, bg, min] of PAIRS) {
      it(`${id}: ${fg} on ${bg} is at least ${min}:1`, () => {
        expect(contrast(resolve(vars, fg), resolve(vars, bg))).toBeGreaterThanOrEqual(min);
      });
    }
  }

  it("catches a failing pair (the old danger red on the Soft Charcoal panel)", () => {
    expect(contrast("#c8304a", "#1d1e22")).toBeLessThan(4.5);
  });

  it("reads the real themes, not an empty table", () => {
    const vars = theme(rules, "ops-dark");
    expect(resolve(vars, "bg")).toBe("#0b0b0d");
    expect(resolve(vars, "accent")).toBe("#ffb000");
  });
});
