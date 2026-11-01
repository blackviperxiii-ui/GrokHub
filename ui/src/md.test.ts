import { describe, expect, it } from "vitest";
import fixtures from "./md.fixtures.json";
import { codeTokens, displayText, mdBlocks, mdLines, mdLinkOk, mdSpans, TEXT_FILE_CAP } from "./md";

// md.fixtures.json is the Rust parser's output (src-tauri/src/md_fixtures.rs),
// so these fail when the port drifts from grokhub_core::md.
describe("the TypeScript parser matches the core parser", () => {
  it("has the fixtures, not an empty table", () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(30);
    expect(fixtures.code.length).toBeGreaterThanOrEqual(10);
  });

  for (const [n, c] of fixtures.cases.entries()) {
    it(`blocks and spans, case ${n}: ${JSON.stringify(c.input).slice(0, 48)}`, () => {
      expect(mdBlocks(c.input)).toEqual(c.blocks);
      expect(mdLines(c.input).map(mdSpans)).toEqual(c.spans);
    });
  }

  for (const [n, c] of fixtures.code.entries()) {
    it(`code tokens, case ${n}: ${c.lang || "(no lang)"} ${c.line}`, () => {
      expect(codeTokens(c.lang, c.line)).toEqual(c.tokens);
    });
  }
});

describe("md", () => {
  it("reads a heading, a list, a table and a fence", () => {
    expect(mdBlocks("# Title\n- a\n1. b\n| x | y |\n|---|--:|\n| 1 | 2 |\n```ts\nlet a\n```")).toEqual([
      { kind: "heading", level: 1, text: "Title" },
      { kind: "bullet", depth: 0, text: "a" },
      { kind: "numbered", depth: 0, num: "1", text: "b" },
      { kind: "table", header: ["x", "y"], align: ["left", "right"], rows: [["1", "2"]] },
      { kind: "code", lang: "ts", body: "let a" },
    ]);
  });

  it("runs a half-streamed fence to the end", () => {
    expect(mdBlocks("Here:\n```py\nprint(1)\nprint(")).toEqual([
      { kind: "para", text: "Here:" },
      { kind: "code", lang: "py", body: "print(1)\nprint(" },
    ]);
  });

  it("links only http(s)", () => {
    expect(mdLinkOk("https://x.ai")).toBe(true);
    expect(mdLinkOk("http://example.com/a?b=c")).toBe(true);
    for (const bad of ["javascript:alert(1)", "data:text/html,x", "file:///etc/passwd", "https://", "https://a b"]) {
      expect(mdLinkOk(bad)).toBe(false);
    }
    expect(mdSpans("[x](javascript:alert(1))")).toEqual([{ kind: "text", text: "[x](javascript:alert(1))" }]);
  });

  it("caps a reply at TEXT_FILE_CAP bytes without splitting a character", () => {
    expect(displayText("short")).toBe("short");
    const shown = displayText("é".repeat(TEXT_FILE_CAP));
    expect(new TextEncoder().encode(shown).length).toBe(TEXT_FILE_CAP);
    expect(shown).toBe("é".repeat(TEXT_FILE_CAP / 2));
    const emoji = displayText("😀".repeat(TEXT_FILE_CAP));
    expect(emoji.length).toBe(TEXT_FILE_CAP / 2);
    expect(emoji.endsWith("😀")).toBe(true);
  });
});
