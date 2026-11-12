import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ScreenshotOfferView } from "./screenshotOfferApi";

const api = vi.hoisted(() => ({
  getScreenshotOffer: vi.fn<() => Promise<ScreenshotOfferView>>(),
  setScreenshotExplain: vi.fn<(on: boolean) => Promise<ScreenshotOfferView>>(),
}));

vi.mock("./screenshotOfferApi", () => api);

import { ScreenshotExplain } from "./ScreenshotExplain";

describe("ScreenshotExplain", () => {
  beforeEach(() => {
    api.getScreenshotOffer.mockReset().mockResolvedValue({ enabled: false, offer: null });
    api.setScreenshotExplain.mockReset().mockResolvedValue({ enabled: true, offer: null });
  });

  it("is off by default and says the picture isn't read until a tap", async () => {
    render(<ScreenshotExplain />);
    expect(await screen.findByRole("switch", { name: "Screenshot explain" })).toHaveAttribute("aria-checked", "false");
    expect(
      screen.getByText("When you save a new screenshot, offer a small Explain chip. The image is not read until you tap."),
    ).toBeVisible();
  });

  it("turns on and saves", async () => {
    render(<ScreenshotExplain />);
    await userEvent.click(await screen.findByRole("switch", { name: "Screenshot explain" }));
    expect(api.setScreenshotExplain).toHaveBeenCalledWith(true);
    expect(screen.getByRole("switch", { name: "Screenshot explain" })).toHaveAttribute("aria-checked", "true");
  });

  it("a failed save keeps the switch where it was and says why", async () => {
    api.setScreenshotExplain.mockRejectedValue("app.json is busy. Try again.");
    render(<ScreenshotExplain />);
    await userEvent.click(await screen.findByRole("switch", { name: "Screenshot explain" }));
    expect(screen.getByRole("switch", { name: "Screenshot explain" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByRole("status")).toHaveTextContent("app.json is busy. Try again.");
  });
});
