import { afterEach, describe, expect, it } from "vitest";
import { applyCachedTheme, applyTheme } from "./theme";
import indexHtml from "../index.html?raw";

afterEach(() => {
  localStorage.clear();
  delete document.documentElement.dataset.theme;
});

describe("theme", () => {
  it("remembers a pinned theme and paints it on the next start", () => {
    applyTheme("light");
    delete document.documentElement.dataset.theme;
    applyCachedTheme();
    expect(document.documentElement.dataset.theme).toBe("light");
  });

  it("forgets the theme when Settings goes back to System", () => {
    applyTheme("oled-black");
    applyTheme("system");
    delete document.documentElement.dataset.theme;
    applyCachedTheme();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("ignores a cached value that isn't a theme", () => {
    localStorage.setItem("shell.theme", "neon");
    applyCachedTheme();
    expect(document.documentElement.dataset.theme).toBeUndefined();
  });

  it("gives the page the app icon as its favicon", () => {
    expect(indexHtml).toContain('<link rel="icon" type="image/svg+xml" href="../brand/app-icon.svg" />');
  });
});
