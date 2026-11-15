import { describe, expect, it } from "vitest";
import css from "./styles.css?raw";

/**
 * The glam motion (pulsing dot, tab underline, card entrance, XP and streak
 * fill, composer glow) runs only when nobody asked for less motion: not under
 * the OS setting, and not under Settings → Appearance → Reduce motion.
 */

const GLAM = ["dot-pulse", "tab-open", "card-enter", "xp-grow", "streak-fill", "composer-glow"];
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
    expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\) \{\s*:root:root \{\s*--backdrop-grid: none;\s*\}\s*\}/);
  });

  it("keeps the display size and the display family in separate tokens", () => {
    const sizes = [...css.matchAll(/--font-display:\s*([^;]+);/g)].map((m) => m[1]);
    expect(sizes).toEqual(["2rem", "1.5rem"]);
    const families = [...css.matchAll(/--font-family-display:\s*([^;]+);/g)].map((m) => m[1]);
    expect(families).toEqual(['"Chakra Petch", "Inter", system-ui, sans-serif']);
  });

  it("loads the display face without blocking first paint", () => {
    const faces = [...css.matchAll(/@font-face \{([^}]*)\}/g)].map((m) => m[1]).filter((f) => f.includes("Chakra Petch"));
    expect(faces.map((f) => /font-weight: (\d+)/.exec(f)?.[1])).toEqual(["500", "600"]);
    for (const face of faces) expect(face).toContain("font-display: swap;");
  });
});

/** Every plain rule with its @media and body, comments dropped. */
function rules(source: string): { media: string | null; selector: string; body: string }[] {
  const text = source.replace(/\/\*[\s\S]*?\*\//g, "");
  const stack: string[] = [];
  const out: { media: string | null; selector: string; body: string }[] = [];
  let start = 0;
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "{") {
      stack.push(text.slice(start, i).split(/[;}]/).pop()!.trim());
      start = i + 1;
    } else if (text[i] === "}") {
      const head = stack[stack.length - 1] ?? "";
      if (head && !head.startsWith("@") && !/^(from|to|\d)/.test(head)) {
        const media = stack.slice(0, -1).find((h) => h.startsWith("@media")) ?? null;
        for (const selector of splitSelectors(head)) out.push({ media, selector, body: text.slice(start, i) });
      }
      stack.pop();
      start = i + 1;
    }
  }
  return out;
}

describe("the desktop companion's motion and reduce motion", () => {
  const all = rules(css);
  const DESK = ["nyx-desk-bob", "nyx-desk-breath", "nyx-desk-think", "nyx-desk-spin"];
  const stopped = (selector: string, media: string | null, root: boolean) =>
    all.some(
      (r) =>
        r.media === media &&
        r.selector === (root ? `:root[data-reduce-motion="true"] ${selector}` : selector) &&
        /animation:\s*none/.test(r.body),
    );

  it("breathes from her feet every 4 s and floats 3 px either way every 5 s", () => {
    const figure = all.find((r) => r.selector === ".nyx-desk-figure" && r.media === null && /animation:/.test(r.body));
    expect(figure?.body).toMatch(/transform-origin:\s*50% 100%/);
    expect(figure?.body).toMatch(/nyx-desk-bob 5s ease-in-out infinite,\s*nyx-desk-breath 4s ease-in-out infinite/);
    expect(css).toMatch(/@keyframes nyx-desk-breath \{\s*0%,\s*100% \{\s*scale: 1;\s*\}\s*50% \{\s*scale: 1\.006 1\.012;\s*\}\s*\}/);
    expect(css).toMatch(/@keyframes nyx-desk-bob \{\s*0%,\s*100% \{\s*translate: 0 3px;\s*\}\s*50% \{\s*translate: 0 -3px;\s*\}\s*\}/);
  });

  it("glows her pedestal with the voice", () => {
    const pedestal = (look: string) =>
      all.find((r) => r.media === null && r.selector === `.nyx-desk[data-look="${look}"] .nyx-desk-pedestal`)?.body ?? "";
    expect(pedestal("listening")).toMatch(/filter:\s*brightness\(1\.35\)/);
    expect(pedestal("speaking")).toMatch(/filter:\s*brightness\(1\.2\)/);
    expect(pedestal("thinking")).toMatch(/animation:\s*nyx-desk-think 0\.286s ease-in-out infinite alternate/);
    expect(css).toMatch(/@keyframes nyx-desk-think \{\s*from \{\s*filter: brightness\(1\);\s*\}\s*to \{\s*filter: brightness\(1\.18\);\s*\}\s*\}/);
  });

  it("stops every desk animation under both reduce-motion settings", () => {
    // One name at a time: a rule can run two of them.
    const uses = DESK.flatMap((name) => animationUses(css, [name]));
    expect([...new Set(uses.map((u) => u.name))].sort()).toEqual([...DESK].sort());
    for (const use of uses) {
      for (const sel of use.selectors) {
        expect(stopped(sel, null, true), `${sel} under data-reduce-motion`).toBe(true);
        expect(stopped(sel, "@media (prefers-reduced-motion: reduce)", false), `${sel} under the OS setting`).toBe(true);
      }
    }
  });

  it("keeps a steady glow while thinking under reduce motion", () => {
    const steady = all.filter(
      (r) =>
        r.selector.endsWith('.nyx-desk[data-look="thinking"] .nyx-desk-pedestal') &&
        /animation:\s*none/.test(r.body) &&
        /filter:\s*brightness\(1\.1\)/.test(r.body),
    );
    expect(steady.map((r) => r.media)).toEqual([null, "@media (prefers-reduced-motion: reduce)"]);
  });
});
