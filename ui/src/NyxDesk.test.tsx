import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopNyxState } from "./desktopNyxApi";

const api = vi.hoisted(() => ({
  desktopNyxGet: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxDock: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxSet: vi.fn<(patch: object) => Promise<DesktopNyxState>>(),
  desktopNyxSetOutfit: vi.fn<(id: string | null) => Promise<DesktopNyxState>>(),
  desktopNyxSavePos: vi.fn(() => Promise.resolve(state())),
  desktopNyxSetPassthrough: vi.fn(() => Promise.resolve()),
}));

vi.mock("./desktopNyxApi", () => api);

import { NyxDesk } from "./NyxDesk";

function state(over: Partial<DesktopNyxState> = {}): DesktopNyxState {
  return {
    enabled: true,
    undocked: true,
    behind: false,
    adult: false,
    adultsOk: false,
    outfit: null,
    outfitFile: "brand/nyx-hanging-out.jpg",
    outfits: [
      { id: "hanging-out", label: "Hanging out", file: "brand/nyx-hanging-out.jpg", adult: false },
      { id: "gaming", label: "Gaming", file: "brand/nyx-gaming.jpg", adult: false },
    ],
    pos: null,
    effects: { transparent: true, onTop: true },
    companion: "Nyx",
    ...over,
  };
}

describe("NyxDesk", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.desktopNyxGet.mockResolvedValue(state());
    api.desktopNyxDock.mockResolvedValue(state({ undocked: false }));
    api.desktopNyxSetOutfit.mockImplementation(async (id) => state({ outfit: id }));
    api.desktopNyxSet.mockImplementation(async (patch) => state(patch as Partial<DesktopNyxState>));
    api.desktopNyxSavePos.mockResolvedValue(state());
    api.desktopNyxSetPassthrough.mockResolvedValue();
  });

  it("docks her back from the context menu", async () => {
    render(<NyxDesk />);
    const root = await screen.findByLabelText("Nyx");
    await userEvent.pointer({ keys: "[MouseRight]", target: root });
    const menu = await screen.findByRole("menu");
    await userEvent.click(within(menu).getByRole("menuitem", { name: "Bring her back" }));
    await waitFor(() => expect(api.desktopNyxDock).toHaveBeenCalled());
  });

  it("saves a wardrobe pick", async () => {
    render(<NyxDesk />);
    const root = await screen.findByLabelText("Nyx");
    await userEvent.pointer({ keys: "[MouseRight]", target: root });
    const menu = await screen.findByRole("menu");
    await userEvent.click(within(menu).getByRole("menuitem", { name: "Gaming" }));
    await waitFor(() => expect(api.desktopNyxSetOutfit).toHaveBeenCalledWith("gaming"));
  });
});
