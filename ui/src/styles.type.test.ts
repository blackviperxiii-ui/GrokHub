import { describe, expect, it } from "vitest";
import RAW_CSS from "./styles.css?raw";

// One type scale for both apps (docs/design/type.md). Fix a failure with
// `scripts/type_snap.py css ui/src/styles.css`.
const CSS = RAW_CSS.replace(/\r\n/g, "\n").replace(/\/\*[\s\S]*?\*\//g, "");
const STEPS = ["badge", "caption", "body", "prose", "section", "headline", "display"];
const LINES = ["caption", "body", "prose", "section", "headline", "display"];

/** Every font-size, line-height or font-weight that is off the type scale. */
function offScale(sheet: string): string[] {
  const bad: string[] = [];
  for (const m of sheet.matchAll(/(?<![\w-])(font-size|line-height|font-weight)\s*:\s*([^;{}]+)/g)) {
    const [prop, raw] = [m[1], m[2].trim()];
    const token = /^var\(--(font|lh)-([a-z]+)\)$/.exec(raw);
    const ok =
      prop === "font-size"
        ? raw === "inherit" || (token?.[1] === "font" && STEPS.includes(token[2]))
        : prop === "line-height"
          ? raw === "1" || (token?.[1] === "lh" && LINES.includes(token[2]))
          : ["400", "500", "600"].includes(raw);
    if (!ok) bad.push(`${prop}: ${raw}`);
  }
  return bad;
}

describe("styles.css type", () => {
  it("keeps every size, line height and weight on the type scale", () => {
    expect(offScale(CSS)).toEqual([]);
  });

  it("catches off-scale type", () => {
    expect(
      offScale(
        ".a { font-size: 0.85em; line-height: 1.45; font-weight: 700 } .b { font-size: var(--font-body); line-height: var(--lh-body); font-weight: 600 }",
      ),
    ).toEqual(["font-size: 0.85em", "line-height: 1.45", "font-weight: 700"]);
  });

  it("defines the scale in rem on the 4px line grid", () => {
    const want: [string, string, string][] = [
      ["caption", "0.75rem", "1rem"],
      ["body", "0.875rem", "1.25rem"],
      ["prose", "1rem", "1.5rem"],
      ["section", "1.25rem", "1.75rem"],
      ["headline", "1.5rem", "2rem"],
      ["display", "2rem", "2.5rem"],
    ];
    for (const [step, size, lh] of want) {
      expect(CSS).toContain(`--font-${step}: ${size};`);
      expect(CSS).toContain(`--lh-${step}: ${lh};`);
    }
    expect(CSS).toContain("--font-badge: 0.625rem;");
  });

  it("reads body at 14/20 and chat at 16/24", () => {
    expect(CSS).toMatch(/body \{[^}]*font-size: var\(--font-body\);[^}]*line-height: var\(--lh-body\);/);
    expect(CSS).toMatch(/\.bubble \{[^}]*font-size: var\(--font-prose\);[^}]*line-height: var\(--lh-prose\);/);
    expect(CSS).toMatch(/\.composer textarea \{[^}]*font-size: var\(--font-prose\);/);
  });

  it("steps titles down on narrow windows, never body text", () => {
    const narrow = /@media \(max-width: 899px\) \{\s*:root \{([^}]*)\}/.exec(CSS)?.[1] ?? "";
    expect(narrow).toContain("--font-headline: 1.25rem;");
    expect(narrow).toContain("--font-display: 1.5rem;");
    expect(narrow).not.toContain("--font-body");
    expect(narrow).not.toContain("--font-prose");
  });
});
