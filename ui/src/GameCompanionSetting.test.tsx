import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GameCompanion } from "./api";

const api = vi.hoisted(() => ({
  getGameCompanion: vi.fn<() => Promise<GameCompanion>>(),
  setGameCompanion: vi.fn<(on: boolean) => Promise<GameCompanion>>(),
  setGameCompanionPing: vi.fn<(on: boolean) => Promise<GameCompanion>>(),
}));
vi.mock("./api", () => api);

import { GameCompanionSetting, PING_TITLE } from "./GameCompanionSetting";

describe("GameCompanionSetting", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("shows the ping only while the companion is on, and turns it on", async () => {
    api.getGameCompanion.mockResolvedValue({ on: false, ping: false, note: "" });
    api.setGameCompanion.mockResolvedValue({
      on: true,
      ping: false,
      note: "On. Start a game and what changed since you last played shows here.",
    });
    api.setGameCompanionPing.mockResolvedValue({
      on: true,
      ping: true,
      note: "On. A quiet notification comes with the card.",
    });
    render(<GameCompanionSetting notifications />);
    const main = await screen.findByRole("switch", { name: "Game launch companion" });
    expect(main).not.toBeChecked();
    expect(screen.queryByText(PING_TITLE)).toBeNull();

    await userEvent.click(main);
    expect(api.setGameCompanion).toHaveBeenCalledWith(true);
    const ping = await screen.findByRole("switch", { name: PING_TITLE });
    expect(ping).not.toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(
      "On. Start a game and what changed since you last played shows here.",
    );

    await userEvent.click(ping);
    expect(api.setGameCompanionPing).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("switch", { name: PING_TITLE })).toBeChecked();
    expect(screen.getByRole("status")).toHaveTextContent(
      "On. A quiet notification comes with the card.",
    );
    expect(
      screen.queryByText("Notifications are off in Settings → General, so this stays quiet."),
    ).toBeNull();
  });

  it("says the ping stays quiet while notifications are off", async () => {
    api.getGameCompanion.mockResolvedValue({ on: true, ping: true, note: "" });
    render(<GameCompanionSetting notifications={false} />);
    expect(
      await screen.findByText("Notifications are off in Settings → General, so this stays quiet."),
    ).toBeVisible();
  });
});
