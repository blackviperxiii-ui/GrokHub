import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HomeCatalog, HomePick } from "./api";

const api = vi.hoisted(() => ({
  homeCatalog: vi.fn<() => Promise<HomeCatalog>>(),
  setHomeLayout: vi.fn<(pick: HomePick) => Promise<void>>(),
}));
vi.mock("./api", () => api);

import { HomeEdit, place } from "./HomeEdit";

const MISSION: HomePick = { preset: "mission_control", large: "objectives", medium: ["pc_loadout", ""], small: ["up_next", null, null, null] };
const STREAMER: HomePick = {
  preset: "streamer",
  large: "clips",
  medium: ["squad", "pulse"],
  small: ["stream_check", "speeds", "connection", "daily_drop"],
};
const mod = (id: string, name: string, sizes: ("large" | "medium" | "small")[]) => ({ id, name, category: "Play", sizes, purpose: `${name} purpose` });
const CATALOG: HomeCatalog = {
  current: MISSION,
  modules: [
    mod("objectives", "Objectives", ["large", "medium"]),
    mod("clips", "Clips", ["large"]),
    mod("pc_loadout", "PC Loadout", ["large", "medium"]),
    mod("squad", "Squad", ["medium"]),
    mod("pulse", "Pulse", ["medium"]),
    mod("up_next", "Up Next", ["small"]),
    mod("speeds", "Speeds", ["medium", "small"]),
    mod("stream_check", "Stream Check", ["small"]),
    mod("connection", "Connection", ["small"]),
    mod("daily_drop", "Daily Drop", ["small"]),
  ],
  presets: [
    { id: "mission_control", label: "Mission Control", pick: MISSION },
    { id: "streamer", label: "Streamer/Social", pick: STREAMER },
  ],
};

describe("Edit Home", () => {
  beforeEach(() => {
    api.homeCatalog.mockReset().mockResolvedValue(CATALOG);
    api.setHomeLayout.mockReset().mockResolvedValue();
  });

  it("moves a module to its new slot instead of showing it twice", () => {
    const moved = place(MISSION, "small-1", "up_next");
    expect(moved.small).toEqual([null, "up_next", null, null]);
    expect(moved.preset).toBe("custom");
    expect(place(MISSION, "medium-0", "").medium).toEqual(["", ""]);
    expect(place(MISSION, "medium-1", "objectives").large).toBe("");
  });

  it("starts from a preset, changes one slot and saves", async () => {
    const onDone = vi.fn();
    render(<HomeEdit onDone={onDone} />);
    await userEvent.click(await screen.findByRole("button", { name: "Streamer/Social" }));
    expect(screen.getByRole("combobox", { name: "Big tile" })).toHaveValue("clips");
    await userEvent.selectOptions(screen.getByRole("combobox", { name: "Small tile 4" }), "");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.setHomeLayout).toHaveBeenCalledWith({ ...STREAMER, preset: "custom", small: ["stream_check", "speeds", "connection", null] });
    expect(onDone).toHaveBeenCalledWith(true);
  });

  it("offers only modules that fit each slot, and Big tile can't be empty", async () => {
    render(<HomeEdit onDone={vi.fn()} />);
    const big = await screen.findByRole("combobox", { name: "Big tile" });
    const names = [...big.querySelectorAll("option")].map((o) => o.textContent);
    expect(names).toEqual(["Objectives", "Clips", "PC Loadout"]);
    const small = screen.getByRole("combobox", { name: "Small tile 2" });
    expect(small.querySelector("option")?.textContent).toBe("Nothing here");
  });

  it("keeps the editor open and says so when saving fails", async () => {
    api.setHomeLayout.mockRejectedValue("Couldn't save Home. Try again.");
    const onDone = vi.fn();
    render(<HomeEdit onDone={onDone} />);
    await userEvent.click(await screen.findByRole("button", { name: "Save" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Couldn't save Home");
    expect(onDone).not.toHaveBeenCalled();
  });
});
