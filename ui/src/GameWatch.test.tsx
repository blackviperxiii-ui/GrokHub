import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GameWatch as Shape } from "./api";

const api = vi.hoisted(() => ({
  getGameWatch: vi.fn<() => Promise<Shape>>(),
  setGameWatch: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { GAME_WATCH_HELP, GAME_WATCH_TITLE, GameWatch } from "./GameWatch";

describe("GameWatch", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("loads off by default and saves the toggle", async () => {
    api.getGameWatch.mockResolvedValue({ on: false, note: "" });
    api.setGameWatch.mockResolvedValue({
      on: true,
      note: "Set up for Counter-Strike 2. Start the game, or restart it if it's open, so it reads the file.",
    });
    render(<GameWatch />);
    expect(await screen.findByText(GAME_WATCH_HELP)).toBeVisible();
    const sw = await screen.findByRole("switch", { name: GAME_WATCH_TITLE });
    expect(sw).toHaveAttribute("aria-checked", "false");
    await userEvent.click(sw);
    expect(api.setGameWatch).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("status")).toHaveTextContent("Set up for Counter-Strike 2");
    expect(screen.getByRole("switch", { name: GAME_WATCH_TITLE })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("renders a compact row for the Library companion", async () => {
    api.getGameWatch.mockResolvedValue({ on: false, note: "" });
    render(<GameWatch compact />);
    expect(await screen.findByRole("switch", { name: GAME_WATCH_TITLE })).toBeVisible();
    expect(screen.queryByRole("heading", { name: "Game watcher" })).toBeNull();
  });
});
