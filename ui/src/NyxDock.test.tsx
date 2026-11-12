import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopNyxState } from "./desktopNyxApi";

const api = vi.hoisted(() => ({
  desktopNyxGet: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxUndock: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxDock: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxSetOutfit: vi.fn<(id: string | null) => Promise<DesktopNyxState>>(),
}));

vi.mock("./desktopNyxApi", () => api);
vi.mock("./transport", () => ({ inShell: () => true }));

import { NyxDock } from "./NyxDock";

function state(over: Partial<DesktopNyxState> = {}): DesktopNyxState {
  return {
    enabled: true,
    undocked: false,
    behind: false,
    adult: false,
    adultsOk: false,
    outfit: null,
    outfitFile: "brand/nyx-hanging-out.jpg",
    outfits: [
      { id: "hanging-out", label: "Hanging out", file: "brand/nyx-hanging-out.jpg", adult: false },
      { id: "working", label: "Working", file: "brand/nyx-working.jpg", adult: false },
    ],
    pos: null,
    effects: { transparent: true, onTop: true },
    companion: "Nyx",
    ...over,
  };
}

describe("NyxDock desktop controls", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    Object.defineProperty(window, "matchMedia", {
      writable: true,
      value: (query: string) => ({
        matches: query.includes("900"),
        media: query,
        addEventListener: () => {},
        removeEventListener: () => {},
        addListener: () => {},
        removeListener: () => {},
        dispatchEvent: () => false,
        onchange: null,
      }),
    });
  });

  it("puts her on the desktop and brings her back", async () => {
    api.desktopNyxGet.mockResolvedValue(state());
    api.desktopNyxUndock.mockResolvedValue(state({ undocked: true }));
    api.desktopNyxDock.mockResolvedValue(state({ undocked: false }));
    render(<NyxDock url="/nyx.jpg" mood="hanging-out" />);
    const put = await screen.findByRole("button", { name: "Put her on the desktop" });
    await userEvent.click(put);
    await waitFor(() => expect(api.desktopNyxUndock).toHaveBeenCalled());
    expect(await screen.findByRole("button", { name: "Bring her back" })).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Bring her back" }));
    await waitFor(() => expect(api.desktopNyxDock).toHaveBeenCalled());
  });

  it("saves a wardrobe pick from the dock", async () => {
    api.desktopNyxGet.mockResolvedValue(state());
    api.desktopNyxSetOutfit.mockResolvedValue(state({ outfit: "working" }));
    render(<NyxDock url="/nyx.jpg" mood="hanging-out" />);
    const select = await screen.findByLabelText("Outfit");
    await userEvent.selectOptions(select, "working");
    await waitFor(() => expect(api.desktopNyxSetOutfit).toHaveBeenCalledWith("working"));
  });

  it("hides desktop controls while the Labs switch is off", async () => {
    api.desktopNyxGet.mockResolvedValue(state({ enabled: false }));
    render(<NyxDock url="/nyx.jpg" mood="hanging-out" />);
    expect(await screen.findByText("Hanging out")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Put her on the desktop" })).toBeNull();
  });
});
