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

describe("styles.css chat cards", () => {
  it("a new message or card rises in by its own animation, with no view transition", () => {
    expect(rule(CSS, ".log > .bubble,\n.log > .card {")).toBe(
      ".log > .bubble, .log > .card { animation: rise-in var(--t-base) var(--ease-out) backwards; }",
    );
    // A view transition holds the page's updates while it runs, and the
    // message box then drops what is typed.
    expect(CSS).not.toContain("::view-transition");
  });

  it("turns motion off for reduced motion", () => {
    const reduced = mediaBlocks(CSS, "(prefers-reduced-motion: reduce)").find((b) =>
      b.includes("animation-duration: 0.01ms !important"),
    );
    expect(reduced).toContain("transition-duration: 0.01ms !important");
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

// The window opens 900px wide (src-tauri/tauri.conf.json), where the tabs fold
// to icons. Fifteen of them, the brand, the Activity dock and Command are wider
// than that, so the icons wrap to a second row: a page that scrolls sideways
// puts a scrollbar along the window's bottom edge, over the composer's Send,
// and a views bar that scrolls in place takes no clicks in WebKitGTK. The shell
// smoke checks the real width and clicks the tabs.
describe("styles.css views bar in a narrow window", () => {
  const narrow = mediaBlocks(CSS, "(max-width: 900px)").find((b) => b.includes(".tab-label")) ?? "";

  it("wraps the icons to another row instead of widening the page", () => {
    const tabs = rule(narrow, ".tabs {");
    expect(tabs).toContain("flex-wrap: wrap;");
    expect(tabs).toContain("min-width: 0;");
  });

  it("keeps the tab row out of scroll boxes, where WebKitGTK cannot click its tabs", () => {
    expect(rule(narrow, ".tabs {")).not.toMatch(/overflow/);
    const base = rule(CSS, ".tabs {");
    expect(base).toContain("display: flex;");
    expect(base).not.toMatch(/overflow/);
  });

  it("keeps each tab at its own width so a row wraps rather than squashing icons", () => {
    expect(rule(narrow, ".tabs button {")).toContain("flex: none;");
  });

  // A phone also matches the 900px query. The phone bar is one fixed 52px row,
  // so the last phone rule for .tabs must come after the wrap and undo it, or
  // the icons stack into rows over the composer.
  it("keeps the phone bar on one row after the narrow-window wrap", () => {
    const bare = CSS.replace(/\/\*[\s\S]*?\*\//g, "");
    const phones = mediaBlocks(CSS, "(max-width: 600px)").filter((b) => rule(b, ".tabs {") !== "");
    const last = phones[phones.length - 1] ?? "";
    expect(bare.indexOf(last)).toBeGreaterThan(bare.indexOf(narrow));
    expect(rule(last, ".tabs {")).toContain("flex-wrap: nowrap;");
  });

  it("gives the phone composer textarea a full row above its buttons", () => {
    const bare = CSS.replace(/\/\*[\s\S]*?\*\//g, "");
    const phone = mediaBlocks(CSS, "(max-width: 600px)").find((b) => b.includes(".composer {")) ?? "";
    expect(bare.indexOf(phone)).toBeGreaterThan(bare.indexOf(".composer-ghost-wrap {"));
    expect(rule(phone, ".composer {")).toContain("flex-wrap: wrap;");
    // Textarea lives inside .composer-ghost-wrap; the wrap takes the full row.
    expect(rule(phone, ".composer-ghost-wrap {")).toContain("order: -1;");
    expect(rule(phone, ".composer-ghost-wrap {")).toContain("flex: 1 0 100%;");
  });
});

// The Code tab is a column flex box that scrolls. A section allowed to shrink
// is squeezed to nothing as soon as the tab is taller than the window, and its
// content then paints over the section below it.
describe("styles.css Code tab layout", () => {
  it("lets no section of the Code tab shrink, so the changed-files list keeps its height", () => {
    expect(rule(CSS, ".automations,\n.memory,\n.connectors,\n.journal,\n.code,")).toContain("overflow-y: auto;");
    expect(rule(CSS, ".code > * {")).toBe(".code > * { flex-shrink: 0; }");
  });

  it("gives the Activity list its own scroll", () => {
    expect(rule(CSS, ".code-activity {")).toContain("max-height: 40vh; overflow-y: auto;");
  });

  it("draws the changed-files list without bullets, and marks the open file", () => {
    expect(rule(CSS, ".code > .code-split > .rows {")).toBe(
      ".code > .code-split > .rows { list-style: none; margin: 0; padding: 0; display: flex; flex-direction: column; gap: 4px; }",
    );
    expect(rule(CSS, '.code-file[aria-pressed="true"] {')).toBe(
      '.code-file[aria-pressed="true"] { border-color: var(--red); background: var(--red-wash); box-shadow: inset 3px 0 0 var(--red); }',
    );
    expect(CSS).not.toContain('.code-tree .code-file[aria-pressed="true"]');
  });

  it("wraps a long changed-file path inside its row instead of drawing it over the diff", () => {
    expect(rule(CSS, ".code-file {")).toBe(
      ".code-file { width: 100%; text-align: start; font-family: ui-monospace, monospace; overflow-wrap: anywhere; }",
    );
  });

  it("scrolls a long changed-files list beside the diff, so the diff a click opens is on screen", () => {
    // The padding (taken back by the margin) leaves room for a row's focus ring, which a scroll box cuts off.
    expect(rule(CSS, ".code > .code-split > .code-changed {")).toBe(
      ".code > .code-split > .code-changed { max-height: 70vh; overflow-y: auto; padding: 4px; margin: -4px; }",
    );
    // It comes after the list's own rule, whose `padding: 0; margin: 0;` it replaces.
    expect(CSS.indexOf(".code > .code-split > .code-changed {")).toBeGreaterThan(CSS.indexOf(".code > .code-split > .rows {"));
    // One column in a narrow window: the diff is under the list, which is shorter to leave it room.
    const narrow = mediaBlocks(CSS, "(max-width: 720px)").find((b) => b.includes(".code-split {")) ?? "";
    expect(rule(narrow, ".code-split {")).toBe(".code-split { grid-template-columns: 1fr; }");
    expect(rule(narrow, ".code > .code-split > .code-changed {")).toBe(".code > .code-split > .code-changed { max-height: 35vh; }");
  });

  it("styles the Deploy heading like the other sections, and the Mode and Channel boxes", () => {
    expect(rule(CSS, ".code-tests h3,\n.code-deploy h3 {")).toBe(
      ".code-tests h3, .code-deploy h3 { margin: 0 0 8px; font-size: var(--font-body); font-weight: 600; }",
    );
    expect(rule(CSS, ".code-deploy {")).toBe(".code-deploy { margin-top: 8px; }");
    expect(rule(CSS, "fieldset.channel {")).toBe(
      "fieldset.channel { border: 1px solid var(--border); border-radius: var(--radius-sm); padding: 8px 12px; margin: 0 0 8px; display: flex; flex-direction: column; gap: 4px; }",
    );
    // Settings stacks a label over its field; a radio sits beside its text.
    expect(rule(CSS, ".settings label {")).toContain("flex-direction: column;");
    expect(rule(CSS, "fieldset.channel label {")).toBe(
      "fieldset.channel label { display: flex; flex-direction: row; align-items: center; gap: 8px; }",
    );
  });

  it("wraps a long path in a helper work row instead of widening the Tasks panel", () => {
    expect(rule(CSS, ".card.agent .card-status.task-work {")).toBe(
      ".card.agent .card-status.task-work { color: var(--text); overflow-wrap: anywhere; }",
    );
    expect(rule(CSS, ".card.agent .card-status.work-detail {")).toContain("overflow-wrap: anywhere;");
    expect(rule(CSS, ".card.agent.work .error {")).toContain("overflow-wrap: anywhere;");
  });

  it("shows a focus ring on the panes that scroll, and on the editor's frame rather than inside it", () => {
    expect(rule(CSS, ".code-diff:focus-visible,\n.code-log:focus-visible {")).toBe(
      ".code-diff:focus-visible, .code-log:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }",
    );
    expect(rule(CSS, ".code-activity:focus-visible,\n.code-diff:focus-visible,\n.code-log:focus-visible {")).toBe(
      ".code-activity:focus-visible, .code-diff:focus-visible, .code-log:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }",
    );
    expect(rule(CSS, ".code-editor-host:focus-within {")).toBe(
      ".code-editor-host:focus-within { box-shadow: var(--ring); }",
    );
    expect(rule(CSS, ".code-editor-host .cm-editor.cm-focused {")).toBe(
      ".code-editor-host .cm-editor.cm-focused { outline: none; }",
    );
  });
});
