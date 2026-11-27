import { afterEach, describe, expect, it } from "vitest";
import { DEFAULT_RAIL_PREFS, loadRailPrefs, RAIL_PREFS_EVENT, saveRailPrefs } from "./railPrefs";

afterEach(() => {
  localStorage.clear();
});

describe("railPrefs", () => {
  it("loads empty pins and folds when nothing is stored", () => {
    expect(loadRailPrefs()).toEqual({ pinned: [], collapsed: [] });
    expect(DEFAULT_RAIL_PREFS).toEqual({ pinned: [], collapsed: [] });
  });

  it("round-trips pins and folds and drops a settings pin", () => {
    saveRailPrefs({
      pinned: ["automations", "settings" as never, "skills"],
      collapsed: ["make", "nope" as never],
    });
    expect(loadRailPrefs()).toEqual({
      pinned: ["automations", "skills"],
      collapsed: ["make"],
    });
  });

  it("keeps pins and folds from a legacy simple-mode save and drops mode", () => {
    localStorage.setItem(
      "shell.railPrefs",
      JSON.stringify({ mode: "simple", pinned: ["mods"], collapsed: ["pc"] }),
    );
    expect(loadRailPrefs()).toEqual({ pinned: ["mods"], collapsed: ["pc"] });
    expect(loadRailPrefs()).not.toHaveProperty("mode");
  });

  it("fires shell-rail-prefs after a save", () => {
    const seen: unknown[] = [];
    const onPrefs = (event: Event) => seen.push((event as CustomEvent).detail);
    window.addEventListener(RAIL_PREFS_EVENT, onPrefs);
    saveRailPrefs({ pinned: ["mods"], collapsed: [] });
    window.removeEventListener(RAIL_PREFS_EVENT, onPrefs);
    expect(seen).toEqual([{ pinned: ["mods"], collapsed: [] }]);
  });
});
