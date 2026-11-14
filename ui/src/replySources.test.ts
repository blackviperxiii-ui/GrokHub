import { describe, expect, it } from "vitest";
import fixtures from "./replySources.fixtures.json";
import { sourceHost, splitSources } from "./replySources";

// replySources.fixtures.json is the Rust split's output
// (src-tauri/src/reply_sources_fixtures.rs), so these fail when the port
// drifts from grokhub_core::reply_sources.
describe("the TypeScript Sources split matches the core split", () => {
  it("has the fixtures, not an empty table", () => {
    expect(fixtures.cases.length).toBeGreaterThanOrEqual(30);
    expect(fixtures.cases.filter((c) => c.split !== null).length).toBeGreaterThanOrEqual(15);
  });

  for (const [n, c] of fixtures.cases.entries()) {
    it(`case ${n}: ${JSON.stringify(c.input).slice(0, 60)}`, () => {
      expect(splitSources(c.input)).toEqual(c.split);
    });
  }
});

describe("splitSources", () => {
  it("cuts a numbered list off the answer and names each site", () => {
    expect(
      splitSources(
        "Rust 1.92 is out.\n\nSources:\n1. https://blog.rust-lang.org/2026/10/30/Rust-1.92.html\n2. https://www.phoronix.com/news/Rust-1.92\n",
      ),
    ).toEqual({
      answer: "Rust 1.92 is out.",
      sources: [
        { url: "https://blog.rust-lang.org/2026/10/30/Rust-1.92.html", site: "blog.rust-lang.org" },
        { url: "https://www.phoronix.com/news/Rust-1.92", site: "phoronix.com" },
      ],
    });
  });

  it("leaves a reply with no trailing list whole", () => {
    expect(splitSources("Answer.\n- https://a.com")).toBeNull();
    expect(splitSources("Sources:\n1. https://a.com\n\nThen more text after.")).toBeNull();
  });

  it("names a site by its lowercase host without www", () => {
    expect(sourceHost("https://WWW.Example.COM:443/a")).toBe("example.com");
    expect(sourceHost("ftp://example.com")).toBeNull();
  });
});
