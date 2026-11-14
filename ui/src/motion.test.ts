import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

/**
 * The glam motion (pulsing dot, tab underline, card entrance, XP and streak
 * fill) runs only when nobody asked for less motion: not under the OS
 * setting, and not under Settings → Appearance → Reduce motion.
 */

const GLAM = ["dot-pulse", "tab-open", "card-enter", "xp-grow", "streak-fill"];
const GATE_MEDIA = "@media (prefers-reduced-motion: no-preference)";
const GATE_ROOT = ':root:not([data-reduce-motion="true"])';

type Use = { name: string; media: string | null; selectors: string[] };

/** A selector list split at its top-level commas, not the ones inside :is() or :not(). */
function splitSelectors(list: string): string[] {
  const out: string[] = [];
  let depth = 0;
  let from = 0;
  for (let i = 0; i < list.length; i++) {
    if (list[i] === "(") depth++;
    else if (list[i] === ")") depth--;
    else if (list[i] === "," && depth === 0) {
      out.push(list.slice(from, i));
      from = i + 1;
    }
  }
  out.push(list.slice(from));
  return out.map((s) => s.replace(/\s+/g, " ").trim());
}

/** Every `animation:` that names one of `names`, with its @media and selectors. */
export function animationUses(source: string, names: string[]): Use[] {
  const text = source.replace(/\/\*[\s\S]*?\*\//g, "");
  const stack: string[] = [];
  const uses: Use[] = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    const ch = text[i];
    if (ch === "{") {
      stack.push(text.slice(start, i).split(/[;}]/).pop()!.trim());
      start = i + 1;
    } else if (ch === "}") {
      const head = stack[stack.length - 1] ?? "";
      if (!head.startsWith("@")) {
        for (const m of text.slice(start, i).matchAll(/(?:^|[\s;])animation(?:-name)?\s*:\s*([^;]+)/g)) {
          const name = names.find((n) => new RegExp(`(^|[\\s,])${n}([\\s,]|$)`).test(m[1]));
          if (!name) continue;
          const media = stack.slice(0, -1).find((h) => h.startsWith("@media")) ?? null;
          uses.push({ name, media, selectors: splitSelectors(head) });
        }
      }
      stack.pop();
      start = i + 1;
    }
  }
  return uses;
}

describe("glam motion and reduce motion", () => {
  const uses = animationUses(css, GLAM);

  it("uses every glam animation somewhere", () => {
    expect([...new Set(uses.map((u) => u.name))].sort()).toEqual([...GLAM].sort());
  });

  it("runs a glam animation only without either reduce-motion setting", () => {
    for (const use of uses) {
      expect(use.media, use.name).toBe(GATE_MEDIA);
      for (const sel of use.selectors) expect(sel.startsWith(GATE_ROOT), sel).toBe(true);
    }
  });

  it("catches an ungated animation", () => {
    const sample = [
      "@media (prefers-reduced-motion: no-preference) {",
      `  ${GATE_ROOT} :is(.a, .c) { animation: tab-open 1s; }`,
      "}",
      ".b { animation: card-enter 1s backwards; }",
    ].join("\n");
    expect(animationUses(sample, GLAM)).toEqual([
      { name: "tab-open", media: GATE_MEDIA, selectors: [`${GATE_ROOT} :is(.a, .c)`] },
      { name: "card-enter", media: null, selectors: [".b"] },
    ]);
  });

  it("drops the backdrop grid under either reduce-motion setting", () => {
    expect(css).toMatch(/:root\[data-reduce-motion="true"\] \{\s*--backdrop-grid: none;\s*\}/);
    expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\) \{\s*:root \{\s*--backdrop-grid: none;\s*\}\s*\}/);
  });

  it("loads the display face without blocking first paint", () => {
    const faces = [...css.matchAll(/@font-face \{([^}]*)\}/g)].map((m) => m[1]).filter((f) => f.includes("Chakra Petch"));
    expect(faces.map((f) => /font-weight: (\d+)/.exec(f)?.[1])).toEqual(["500", "600"]);
    for (const face of faces) expect(face).toContain("font-display: swap;");
  });
});
