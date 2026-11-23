import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PulseAuto as Shape } from "./api";

const api = vi.hoisted(() => ({
  getPulseAuto: vi.fn<() => Promise<Shape>>(),
  setPulseAutoFix: vi.fn<(id: string, on: boolean) => Promise<Shape>>(),
  setProactiveReminders: vi.fn<(on: boolean) => Promise<Shape>>(),
  setPulseAutoPause: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import {
  AUTO_FIXES_HELP,
  DONE_FOR_YOU_TITLE,
  PulseAuto,
  REMINDERS_HELP,
  REMINDERS_TITLE,
} from "./PulseAuto";

const CRASH = "Check a crashing program's crash history";
const CRASH_NOTE =
  "Read only. Runs when the card shows up, and what it finds goes on the card.";

const PAUSE = "Pause an automation that keeps failing";
const PAUSE_NOTE =
  "Changes a GrokHub setting, not your computer. Only after 5 failed runs in a row.";
const HELD =
  'Waiting: "When the AI wants to change things" is on "Ask me first".';

function shape(crash: boolean, reminders: boolean, pause = false): Shape {
  return {
    fixes: [{ id: "crashCheck", label: CRASH, note: CRASH_NOTE, on: crash }],
    reminders,
    pause: { on: pause, label: PAUSE, note: PAUSE_NOTE, held: HELD },
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
    expect(
      screen.getByRole("heading", { name: DONE_FOR_YOU_TITLE }),
    ).toBeVisible();
    expect(screen.getByRole("switch", { name: PAUSE })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.getByText(PAUSE_NOTE)).toBeVisible();
    // What holds it back shows only while it is on.
    expect(screen.queryByText(HELD)).toBeNull();
  });

  it("turns on Done for you and says what still holds it back", async () => {
    api.getPulseAuto.mockResolvedValue(shape(false, false));
    api.setPulseAutoPause.mockResolvedValue(shape(false, false, true));
    render(<PulseAuto />);
    await userEvent.click(await screen.findByRole("switch", { name: PAUSE }));
    expect(api.setPulseAutoPause).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("switch", { name: PAUSE })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("status")).toHaveTextContent(HELD);
    expect(api.setPulseAutoFix).not.toHaveBeenCalled();
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
