import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PcHelperKey, PcHelpers as Shape } from "./api";

const api = vi.hoisted(() => ({
  getPcHelpers: vi.fn<() => Promise<Shape>>(),
  setPcHelper: vi.fn<(key: PcHelperKey, on: boolean) => Promise<Shape>>(),
  setPcAutoSleep: vi.fn<(minutes: number) => Promise<Shape>>(),
  forgetPcRhythm: vi.fn<() => Promise<Shape>>(),
}));

vi.mock("./api", () => api);
vi.mock("./PcFiles", () => ({ PcFiles: () => <p>Files panel</p> }));

import { PcHelpers, WIND_BLIND_NOTE } from "./PcHelpers";

const FRESH: Shape = {
  focusHelp: false,
  windDown: false,
  autoSleepMin: null,
  sleepChoices: [
    { minutes: 15, label: "15 min" },
    { minutes: 30, label: "30 min" },
    { minutes: 60, label: "1 hour" },
  ],
  sleepBlind: false,
  rhythm: true,
  rhythmGames: false,
  rhythmMinutes: 0,
  rhythmSummary: "Still learning. A few days of use fills this in.",
};

describe("PcHelpers", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getPcHelpers.mockResolvedValue(FRESH);
  });

  it("shows the cabin's defaults: focus help and sleep off, the rhythm on", async () => {
    render(<PcHelpers />);
    expect(await screen.findByRole("switch", { name: "Help with the app I was just in" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "Sleep when idle" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("switch", { name: "Learn my daily rhythm" })).toHaveAttribute("aria-checked", "true");
    expect(screen.queryByRole("group", { name: "Sleep after" })).toBeNull();
    expect(screen.getByText("Still learning. A few days of use fills this in.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Forget my rhythm" })).toBeNull();
  });

  it("saves a switch and shows what the shell kept", async () => {
    api.setPcHelper.mockResolvedValue({ ...FRESH, focusHelp: true });
    render(<PcHelpers />);
    await userEvent.click(await screen.findByRole("switch", { name: "Help with the app I was just in" }));
    expect(api.setPcHelper).toHaveBeenCalledWith("focusHelp", true);
    expect(await screen.findByText("Saved.")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Help with the app I was just in" })).toHaveAttribute("aria-checked", "true");
  });

  it("offers the idle times once sleep is on and saves the pick", async () => {
    api.setPcHelper.mockResolvedValue({ ...FRESH, windDown: true, sleepBlind: true });
    api.setPcAutoSleep.mockResolvedValue({ ...FRESH, windDown: true, sleepBlind: true, autoSleepMin: 30 });
    render(<PcHelpers />);
    await userEvent.click(await screen.findByRole("switch", { name: "Sleep when idle" }));
    expect(api.setPcHelper).toHaveBeenCalledWith("windDown", true);
    const group = await screen.findByRole("group", { name: "Sleep after" });
    expect(group.querySelectorAll("button")).toHaveLength(3);
    expect(screen.getByText(WIND_BLIND_NOTE)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "30 min" }));
    expect(api.setPcAutoSleep).toHaveBeenCalledWith(30);
    expect(await screen.findByRole("button", { name: "30 min" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "15 min" })).toHaveAttribute("aria-pressed", "false");
  });

  it("counts games, then forgets the rhythm", async () => {
    api.getPcHelpers.mockResolvedValue({ ...FRESH, rhythmMinutes: 120, rhythmSummary: "Usually on: 6 PM–10 PM." });
    api.setPcHelper.mockResolvedValue({ ...FRESH, rhythmGames: true, rhythmMinutes: 120, rhythmSummary: "Usually on: 6 PM–10 PM." });
    api.forgetPcRhythm.mockResolvedValue(FRESH);
    render(<PcHelpers />);
    expect(await screen.findByText("Usually on: 6 PM–10 PM.")).toBeVisible();
    await userEvent.click(screen.getByRole("switch", { name: "Count when I last played" }));
    expect(api.setPcHelper).toHaveBeenCalledWith("rhythmGames", true);
    await userEvent.click(await screen.findByRole("button", { name: "Forget my rhythm" }));
    expect(api.forgetPcRhythm).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Still learning. A few days of use fills this in.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Forget my rhythm" })).toBeNull();
  });

  it("hides the games switch while the rhythm is off", async () => {
    api.getPcHelpers.mockResolvedValue({ ...FRESH, rhythm: false });
    render(<PcHelpers />);
    expect(await screen.findByRole("switch", { name: "Learn my daily rhythm" })).toHaveAttribute("aria-checked", "false");
    expect(screen.queryByRole("switch", { name: "Count when I last played" })).toBeNull();
  });

  it("shows the refusal and keeps the switch as it was", async () => {
    api.setPcHelper.mockRejectedValue("app.json isn't readable, so nothing was changed.");
    render(<PcHelpers />);
    await userEvent.click(await screen.findByRole("switch", { name: "Help with the app I was just in" }));
    expect(await screen.findByText("app.json isn't readable, so nothing was changed.")).toBeVisible();
    expect(screen.getByRole("switch", { name: "Help with the app I was just in" })).toHaveAttribute("aria-checked", "false");
  });

  it("says why it could not load", async () => {
    api.getPcHelpers.mockRejectedValue("locked");
    render(<PcHelpers />);
    expect(await screen.findByText("locked")).toBeVisible();
  });
});
