import { describe, expect, it } from "vitest";
import CSS from "./styles.css?raw";

// Error text (`--danger-text`) must read at WCAG AA (4.5:1) on every
// theme's page (`--bg`) and card (`--panel`) backgrounds. `--danger` stays for
// borders and fills, which only need 3:1.

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

/** A hex colour, following `var(--x)` through the Ops palette. */
function resolve(vars: Record<string, string>, name: string): string {
  const value = vars[name] ?? PALETTE[name];
  if (!value) throw new Error(`${name} is not defined`);
  const ref = /^var\((--[\w-]+)\)$/.exec(value);
  return ref ? resolve(vars, ref[1]) : value;
}

function luminance(hex: string): number {
  const m = /^#([0-9a-f]{6})$/i.exec(hex);
  if (!m) throw new Error(`${hex} is not a 6-digit hex colour`);
  const [r, g, b] = [0, 2, 4].map((i) => {
    const c = Number.parseInt(m[1].slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio, to one decimal. */
function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return Math.round(((hi + 0.05) / (lo + 0.05)) * 10) / 10;
}

const THEMES: Record<string, string> = {
  light: ":root",
  "ops-dark": ':root[data-theme="dark"],\n:root[data-theme="ops-dark"]',
  "oled-black": ':root[data-theme="oled-black"]',
  "soft-charcoal": ':root[data-theme="soft-charcoal"]',
};

describe("styles.css error text contrast", () => {
  it("every theme's --danger-text reads at 4.5:1 or better on --bg and --panel", () => {
    const seen: Record<string, [number, number]> = {};
    for (const [theme, selector] of Object.entries(THEMES)) {
      const vars = block(selector);
      const text = resolve(vars, "--danger-text");
      seen[theme] = [contrast(text, resolve(vars, "--bg")), contrast(text, resolve(vars, "--panel"))];
      for (const ratio of seen[theme]) expect(ratio, theme).toBeGreaterThanOrEqual(4.5);
    }
    expect(seen).toEqual({
      light: [6.8, 7.4],
      "ops-dark": [7.2, 6.8],
      "oled-black": [7.6, 7.4],
      "soft-charcoal": [6.5, 6.1],
    });
  });

  it("the OS-dark fallback matches the Ops Dark theme", () => {
    const fallback = block(':root:not([data-theme="light"])');
    const ops = block(THEMES["ops-dark"]);
    for (const name of ["--bg", "--panel", "--danger", "--danger-text"]) {
      expect(resolve(fallback, name), name).toBe(resolve(ops, name));
    }
  });

  it("contrast() follows WCAG 2 (black on white is 21:1)", () => {
    expect(contrast("#000000", "#ffffff")).toBe(21);
    // The old error text: `--danger` on the Ops Dark page misses AA.
    expect(contrast("#d7263d", "#0b0b0d")).toBe(4);
  });
});
