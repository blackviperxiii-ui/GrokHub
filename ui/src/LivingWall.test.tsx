import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LivingWall as Shape } from "./api";

const api = vi.hoisted(() => ({
  getLivingWall: vi.fn<() => Promise<Shape>>(),
  setLivingWall: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { LIVING_WALL_HELP, LIVING_WALL_TITLE, LivingWall } from "./LivingWall";

describe("LivingWall", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("loads off and saves the switch", async () => {
    api.getLivingWall.mockResolvedValue({ on: false });
    api.setLivingWall.mockResolvedValue({ on: true });
    render(<LivingWall />);
    const sw = await screen.findByRole("switch", { name: LIVING_WALL_TITLE });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(LIVING_WALL_HELP)).toBeVisible();
    await userEvent.click(sw);
    expect(api.setLivingWall).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("switch", { name: LIVING_WALL_TITLE })).toHaveAttribute("aria-checked", "true");
  });

  it("says why a save failed and keeps the old state", async () => {
    api.getLivingWall.mockResolvedValue({ on: true });
    api.setLivingWall.mockRejectedValue("app.json: read-only");
    render(<LivingWall />);
    await userEvent.click(await screen.findByRole("switch", { name: LIVING_WALL_TITLE }));
    expect(await screen.findByRole("status")).toHaveTextContent("app.json: read-only");
    expect(screen.getByRole("switch", { name: LIVING_WALL_TITLE })).toHaveAttribute("aria-checked", "true");
  });

  it("stays out where the app has no wall", async () => {
    api.getLivingWall.mockRejectedValue("unknown command get_living_wall");
    render(<LivingWall />);
    await act(async () => {});
    expect(screen.queryByRole("group", { name: LIVING_WALL_TITLE })).toBeNull();
  });
});
