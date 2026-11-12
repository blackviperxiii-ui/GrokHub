import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SteamPulse } from "./api";

const api = vi.hoisted(() => ({
  getSteamPulse: vi.fn<() => Promise<SteamPulse>>(),
  setSteamAchievements: vi.fn<(on: boolean) => Promise<SteamPulse>>(),
  setSteamWishlist: vi.fn<(on: boolean) => Promise<SteamPulse>>(),
  setSteamWishlistThreshold: vi.fn<(threshold: string) => Promise<SteamPulse>>(),
  saveSteamId: vi.fn<(draft: string, useLocal: boolean) => Promise<SteamPulse>>(),
  forgetSteamId: vi.fn<() => Promise<SteamPulse>>(),
  steamCheckNow: vi.fn<(module: "achievements" | "deals") => Promise<SteamPulse>>(),
}));

vi.mock("./api", () => api);

import {
  ACHIEVEMENTS_LABEL,
  STEAM_ID_SAVED,
  SteamPulseSources,
  WISHLIST_LABEL,
} from "./SteamPulseSources";

const THRESHOLDS = [
  { key: "anySale", label: "Any sale" },
  { key: "off25", label: "25% off or more" },
  { key: "off50", label: "50% off or more" },
  { key: "lowest", label: "Lowest price seen" },
];

function base(over: Partial<SteamPulse> = {}): SteamPulse {
  return {
    achievementsOn: false,
    wishlistOn: false,
    threshold: "anySale",
    thresholds: THRESHOLDS,
    hasId: false,
    canUseLocal: false,
    achNote: "",
    wishNote: "",
    achPrivate: false,
    wishPrivate: false,
    ...over,
  };
}

describe("SteamPulseSources", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("renders rows off by default and toggles save", async () => {
    api.getSteamPulse.mockResolvedValue(base());
    api.setSteamWishlist.mockResolvedValue(base({ wishlistOn: true }));
    api.setSteamAchievements.mockResolvedValue(
      base({ wishlistOn: true, achievementsOn: true }),
    );
    render(<SteamPulseSources />);
    const wish = await screen.findByRole("switch", { name: WISHLIST_LABEL });
    expect(wish).toHaveAttribute("aria-checked", "false");
    await userEvent.click(wish);
    expect(api.setSteamWishlist).toHaveBeenCalledWith(true);
    expect(await screen.findByLabelText("SteamID64 or profiles link")).toBeVisible();

    await userEvent.click(screen.getByRole("switch", { name: ACHIEVEMENTS_LABEL }));
    expect(api.setSteamAchievements).toHaveBeenCalledWith(true);
  });

  it("shows SteamID saved without digits and Check now", async () => {
    api.getSteamPulse.mockResolvedValue(base({ wishlistOn: true, hasId: true }));
    api.steamCheckNow.mockResolvedValue(
      base({ wishlistOn: true, hasId: true, wishNote: "Checking your wishlist…" }),
    );
    render(<SteamPulseSources />);
    const row = await screen.findByRole("group", { name: "Wishlist alerts" });
    expect(within(row).getByText(STEAM_ID_SAVED)).toBeVisible();
    expect(screen.queryByDisplayValue(/7656/)).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Check now" }));
    expect(api.steamCheckNow).toHaveBeenCalledWith("deals");
    expect(await screen.findByText("Checking your wishlist…")).toBeVisible();
  });

  it("saves a password-style SteamID draft", async () => {
    api.getSteamPulse.mockResolvedValue(base({ wishlistOn: true }));
    api.saveSteamId.mockResolvedValue(base({ wishlistOn: true, hasId: true }));
    render(<SteamPulseSources />);
    const box = await screen.findByLabelText("SteamID64 or profiles link");
    expect(box).toHaveAttribute("type", "password");
    await userEvent.type(box, "76561197960278073");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveSteamId).toHaveBeenCalledWith("76561197960278073", false);
    expect(await screen.findByText(STEAM_ID_SAVED)).toBeVisible();
  });
});
