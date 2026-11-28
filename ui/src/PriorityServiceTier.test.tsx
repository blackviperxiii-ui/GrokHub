import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PriorityServiceTier as Shape } from "./api";

const api = vi.hoisted(() => ({
  getPriorityServiceTier: vi.fn<() => Promise<Shape>>(),
  setPriorityServiceTier: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import {
  PRIORITY_TIER_HELP,
  PRIORITY_TIER_TITLE,
  PriorityServiceTier,
} from "./PriorityServiceTier";

describe("PriorityServiceTier", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("loads off and saves the switch", async () => {
    api.getPriorityServiceTier.mockResolvedValue({ on: false });
    api.setPriorityServiceTier.mockResolvedValue({ on: true });
    render(<PriorityServiceTier />);
    const sw = await screen.findByRole("switch", { name: PRIORITY_TIER_TITLE });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(PRIORITY_TIER_HELP)).toBeVisible();
    expect(PRIORITY_TIER_HELP).toMatch(/higher token cost/);
    await userEvent.click(sw);
    expect(api.setPriorityServiceTier).toHaveBeenCalledWith(true);
    expect(await screen.findByRole("switch", { name: PRIORITY_TIER_TITLE })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("says why a save failed and keeps the old state", async () => {
    api.getPriorityServiceTier.mockResolvedValue({ on: true });
    api.setPriorityServiceTier.mockRejectedValue("app.json: read-only");
    render(<PriorityServiceTier />);
    await userEvent.click(await screen.findByRole("switch", { name: PRIORITY_TIER_TITLE }));
    expect(await screen.findByRole("status")).toHaveTextContent("app.json: read-only");
    expect(screen.getByRole("switch", { name: PRIORITY_TIER_TITLE })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("stays out where the command is missing", async () => {
    api.getPriorityServiceTier.mockRejectedValue("unknown command get_priority_service_tier");
    render(<PriorityServiceTier />);
    await act(async () => {});
    expect(screen.queryByRole("group", { name: PRIORITY_TIER_TITLE })).toBeNull();
  });
});
