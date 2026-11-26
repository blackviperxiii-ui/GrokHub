import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_RAIL_PREFS, loadRailPrefs, RAIL_PREFS_EVENT, saveRailPrefs } from "./railPrefs";

afterEach(() => {
  localStorage.clear();
});

describe("railPrefs", () => {
  it("loads the Simple default when nothing is stored", () => {
    expect(loadRailPrefs()).toEqual({ mode: "simple", pinned: [], collapsed: [] });
    expect(DEFAULT_RAIL_PREFS.mode).toBe("simple");
  });

  it("round-trips Advanced prefs and drops a settings pin", () => {
    saveRailPrefs({
      mode: "advanced",
      pinned: ["automations", "settings" as never, "skills"],
      collapsed: ["make", "nope" as never],
    });
    expect(loadRailPrefs()).toEqual({
      mode: "advanced",
      pinned: ["automations", "skills"],
      collapsed: ["make"],
    });
  });

  it("fires shell-rail-prefs after a save", () => {
    const seen: unknown[] = [];
    const onPrefs = (event: Event) => seen.push((event as CustomEvent).detail);
    window.addEventListener(RAIL_PREFS_EVENT, onPrefs);
    saveRailPrefs({ mode: "advanced", pinned: ["mods"], collapsed: [] });
    window.removeEventListener(RAIL_PREFS_EVENT, onPrefs);
    expect(seen).toEqual([{ mode: "advanced", pinned: ["mods"], collapsed: [] }]);
  });
});
