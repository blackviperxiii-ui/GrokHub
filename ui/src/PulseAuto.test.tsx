import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PulseAuto as Shape } from "./api";

const api = vi.hoisted(() => ({
  getPulseAuto: vi.fn<() => Promise<Shape>>(),
  setPulseAutoFix: vi.fn<(id: string, on: boolean) => Promise<Shape>>(),
  setProactiveReminders: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import {
  AUTO_FIXES_HELP,
  PulseAuto,
  REMINDERS_HELP,
  REMINDERS_TITLE,
} from "./PulseAuto";

const CRASH = "Check a crashing program's crash history";
const CRASH_NOTE =
  "Read only. Runs when the card shows up, and what it finds goes on the card.";

function shape(crash: boolean, reminders: boolean): Shape {
  return {
    fixes: [{ id: "crashCheck", label: CRASH, note: CRASH_NOTE, on: crash }],
    reminders,
  };
}

describe("PulseAuto", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("shows every switch off by default with its note", async () => {
    api.getPulseAuto.mockResolvedValue(shape(false, false));
    render(<PulseAuto />);
    const crash = await screen.findByRole("switch", { name: CRASH });
    expect(crash).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByRole("switch", { name: REMINDERS_TITLE }),
    ).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(CRASH_NOTE)).toBeVisible();
    expect(screen.getByText(AUTO_FIXES_HELP)).toBeVisible();
    expect(screen.getByText(REMINDERS_HELP)).toBeVisible();
  });

  it("turns on a fix by its id and the reminders on their own", async () => {
    api.getPulseAuto.mockResolvedValue(shape(false, false));
    api.setPulseAutoFix.mockResolvedValue(shape(true, false));
    api.setProactiveReminders.mockResolvedValue(shape(true, true));
    render(<PulseAuto />);
    await userEvent.click(await screen.findByRole("switch", { name: CRASH }));
    expect(api.setPulseAutoFix).toHaveBeenCalledWith("crashCheck", true);
    expect(await screen.findByRole("switch", { name: CRASH })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await userEvent.click(
      screen.getByRole("switch", { name: REMINDERS_TITLE }),
    );
    expect(api.setProactiveReminders).toHaveBeenCalledWith(true);
    expect(
      await screen.findByRole("switch", { name: REMINDERS_TITLE }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("says why a save failed and keeps the switch as it was", async () => {
    api.getPulseAuto.mockResolvedValue(shape(false, false));
    api.setPulseAutoFix.mockRejectedValue("app.json is busy. Try again.");
    render(<PulseAuto />);
    await userEvent.click(await screen.findByRole("switch", { name: CRASH }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "app.json is busy. Try again.",
    );
    expect(screen.getByRole("switch", { name: CRASH })).toHaveAttribute(
      "aria-checked",
      "false",
    );
  });
});
