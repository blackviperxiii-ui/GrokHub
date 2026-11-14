import { describe, expect, it } from "vitest";
import RAW_CSS from "./styles.css?raw";

// One spacing scale for both apps (docs/design/spacing.md). 0 and 1px hairlines
// are allowed. Fix a failure with `scripts/spacing_snap.py css ui/src/styles.css`.
const SCALE = [0, 1, 2, 4, 8, 12, 16, 24, 32, 40, 48, 64, 80, 96];
const CSS = RAW_CSS.replace(/\r\n/g, "\n").replace(/\/\*[\s\S]*?\*\//g, "");

/** Every padding / margin / gap px value that is not on the scale. */
function offScale(sheet: string): string[] {
  const bad: string[] = [];
  const decl = /\b((?:padding|margin|gap|row-gap|column-gap)(?:-[a-z]+)*)\s*:\s*([^;{}]+)/g;
  for (const m of sheet.matchAll(decl)) {
    for (const px of m[2].matchAll(/(-?\d+(?:\.\d+)?)px/g)) {
      const v = Math.abs(Number(px[1]));
      if (v <= 96 && !SCALE.includes(v)) bad.push(`${m[1]}: ${m[2].trim()}`);
    }
  }
  return bad;
}

describe("styles.css spacing", () => {
  it("keeps every gap on the spacing scale", () => {
    expect(offScale(CSS)).toEqual([]);
  });

  it("catches an off-scale gap", () => {
    expect(offScale(".a { padding: 6px 12px; gap: 8px; margin-top: 10px }")).toEqual([
      "padding: 6px 12px",
      "margin-top: 10px",
    ]);
  });

  it("defines the scale as tokens", () => {
    for (const step of [2, 4, 8, 12, 16, 24, 32, 40, 48, 64, 80, 96]) {
      expect(CSS).toContain(`--space-${step}: ${step}px;`);
    }
  });

  it("tightens the page margin on narrow windows", () => {
    expect(CSS).toContain("--page-pad: 24px;");
    expect(CSS).toMatch(/@media \(max-width: 899px\) \{\s*:root \{\s*--page-pad: 16px;/);
    // The sides use the page margin; the top 12px belongs to the top bar so
    // the window's top edge drags it.
    expect(CSS).toMatch(/\.app \{[^}]*padding: 0 var\(--page-pad\) 12px;/);
    expect(CSS).toMatch(/\.topbar \{[^}]*padding-top: 12px;[^}]*padding-bottom: 8px;/);
  });
});
