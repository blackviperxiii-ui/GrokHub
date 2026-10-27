import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("./transport", () => ({
  pair: vi.fn(),
  inShell: vi.fn(() => false),
  paired: vi.fn(() => false),
  onPairChange: vi.fn(() => () => {}),
}));

import { APP_ICON_URL, COMPANION_NAME, LOGO_MARK_URL, PRODUCT_NAME, SUPPORT_LABEL, TAGLINE } from "./brand";
import { Pair } from "./Pair";
import indexHtml from "../index.html?raw";

// Every UI source except tests and brand.ts itself.
const sources = import.meta.glob<string>(["./*.ts", "./*.tsx", "!./*.test.*", "!./brand.ts"], {
  query: "?raw",
  import: "default",
  eager: true,
});

/** Lines outside comments that spell `name` out. */
function hardcoded(src: string, name: string): string[] {
  return src
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => !line.startsWith("//") && !line.startsWith("/*") && !line.startsWith("*"))
    .filter((line) => line.includes(name));
}

describe("brand", () => {
  it("reads the name, tagline and companion from brand/brand.json", () => {
    expect(PRODUCT_NAME).toBe("Loadout");
    expect(TAGLINE).toBe("Gear up. Level up.");
    expect(COMPANION_NAME).toBe("Nyx");
    expect(SUPPORT_LABEL).toBe("Support the developer");
    expect(LOGO_MARK_URL).toMatch(/logo-mark\.svg|^data:image\/svg\+xml/);
    expect(APP_ICON_URL).toMatch(/app-icon\.svg|^data:image\/svg\+xml/);
  });

  it("leaves no product name hardcoded in the UI", () => {
    expect(Object.keys(sources).length).toBeGreaterThan(20);
    const hits = Object.entries(sources).flatMap(([file, src]) =>
      ["GrokHub", PRODUCT_NAME].flatMap((name) => hardcoded(src, name).map((l) => `${file}: ${l}`)),
    );
    expect(hits).toEqual([]);
  });

  it("titles the page from brand.json", () => {
    expect(indexHtml).toContain("<title>%PRODUCT_NAME%</title>");
  });

  it("the guard sees a hardcoded name and skips comments", () => {
    expect(hardcoded('// GrokHub\nconst a = "Use GrokHub";', "GrokHub")).toEqual(['const a = "Use GrokHub";']);
  });

  it("shows the logo mark and the name on the pairing screen", () => {
    render(<Pair onPaired={() => {}} />);
    const heading = screen.getByRole("heading", { level: 1 });
    expect(heading).toHaveTextContent("Loadout");
    expect(heading.querySelector("img")?.getAttribute("src")).toBe(LOGO_MARK_URL);
  });
});
