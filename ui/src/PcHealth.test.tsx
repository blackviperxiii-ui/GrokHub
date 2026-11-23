import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PcHealth as Shape } from "./api";

const api = vi.hoisted(() => ({
  getPcHealth: vi.fn<() => Promise<Shape>>(),
  setPcHealth: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { PC_HEALTH_HELP, PC_HEALTH_TITLE, PcHealth } from "./PcHealth";

describe("PcHealth", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("starts off and says it only reads", async () => {
    api.getPcHealth.mockResolvedValue({ on: false });
    render(<PcHealth />);
    const toggle = await screen.findByRole("switch", { name: PC_HEALTH_TITLE });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(PC_HEALTH_HELP)).toBeVisible();
    expect(PC_HEALTH_HELP).toContain("Never changes fans, clocks, or drivers.");
  });

  it("turns on and shows what was saved", async () => {
    api.getPcHealth.mockResolvedValue({ on: false });
    api.setPcHealth.mockResolvedValue({ on: true });
    render(<PcHealth />);
    await userEvent.click(
      await screen.findByRole("switch", { name: PC_HEALTH_TITLE }),
    );
    expect(api.setPcHealth).toHaveBeenCalledWith(true);
    expect(
      await screen.findByRole("switch", { name: PC_HEALTH_TITLE }),
    ).toHaveAttribute("aria-checked", "true");
  });

  it("shows why a save failed and keeps the switch as it was", async () => {
    api.getPcHealth.mockResolvedValue({ on: false });
    api.setPcHealth.mockRejectedValue("app.json is read-only");
    render(<PcHealth />);
    await userEvent.click(
      await screen.findByRole("switch", { name: PC_HEALTH_TITLE }),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "app.json is read-only",
    );
    expect(
      screen.getByRole("switch", { name: PC_HEALTH_TITLE }),
    ).toHaveAttribute("aria-checked", "false");
  });
});
