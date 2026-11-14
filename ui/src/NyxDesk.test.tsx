import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopNyxState } from "./desktopNyxApi";

const api = vi.hoisted(() => ({
  desktopNyxGet: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxDock: vi.fn<() => Promise<DesktopNyxState>>(),
  desktopNyxSet: vi.fn<(patch: object) => Promise<DesktopNyxState>>(),
  desktopNyxSetOutfit: vi.fn<(id: string | null) => Promise<DesktopNyxState>>(),
  desktopNyxSetHit: vi.fn<(boxes: object[]) => Promise<void>>(),
  desktopNyxDrag: vi.fn<() => Promise<void>>(),
  onDesktopNyx: vi.fn<(fn: (s: DesktopNyxState) => void) => Promise<() => void>>(),
}));

let heard: ((s: DesktopNyxState) => void) | null = null;

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
    api.desktopNyxSetHit.mockResolvedValue();
    api.desktopNyxDrag.mockResolvedValue();
    heard = null;
    api.onDesktopNyx.mockImplementation((fn) => {
      heard = fn;
      return Promise.resolve(() => {});
    });
  });

  it("changes her outfit when the dock's wardrobe does", async () => {
    const { container } = render(<NyxDesk />);
    const figure = () => container.querySelector(".nyx-desk-figure")?.outerHTML ?? "";
    await waitFor(() => expect(figure()).toContain("nyx-hanging-out"));
    act(() => heard?.(state({ outfit: "working", outfitFile: "brand/nyx-working.jpg" })));
    expect(figure()).toContain("nyx-working");
    expect(figure()).not.toContain("nyx-hanging-out");
  });

  it("tells the app which parts take the mouse: her stage, and the menu while it is open", async () => {
    const box = vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const menu = this.classList.contains("nyx-desk-menu");
      return { left: menu ? 10 : 40, top: menu ? 20 : 100, width: menu ? 120 : 240, height: menu ? 80 : 360 } as DOMRect;
    });
    try {
      render(<NyxDesk />);
      const root = await screen.findByLabelText("Nyx");
      await waitFor(() =>
        expect(api.desktopNyxSetHit).toHaveBeenLastCalledWith([{ x: 40, y: 100, w: 240, h: 360 }]),
      );
      await userEvent.pointer({ keys: "[MouseRight]", target: root });
      await screen.findByRole("menu");
      await waitFor(() =>
        expect(api.desktopNyxSetHit).toHaveBeenLastCalledWith([
          { x: 40, y: 100, w: 240, h: 360 },
          { x: 10, y: 20, w: 120, h: 80 },
        ]),
      );
    } finally {
      box.mockRestore();
    }
  });

  it("drags her with a left press on her, not on the menu or with the right button", async () => {
    const { container } = render(<NyxDesk />);
    const root = await screen.findByLabelText("Nyx");
    const stage = container.querySelector(".nyx-desk-stage");
    if (!stage) throw new Error("no stage");
    await userEvent.pointer({ keys: "[MouseRight]", target: stage });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(0);
    const menu = await screen.findByRole("menu");
    await userEvent.pointer({ keys: "[MouseLeft]", target: within(menu).getByRole("menuitem", { name: "Gaming" }) });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(0);
    await userEvent.pointer({ keys: "[MouseLeft]", target: stage });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(1);
    expect(root).toBeInTheDocument();
  });

  it("closes her menu when the click lands on the desktop beside her", async () => {
    render(<NyxDesk />);
    const root = await screen.findByLabelText("Nyx");
    await userEvent.pointer({ keys: "[MouseRight]", target: root });
    await screen.findByRole("menu");
    act(() => {
      window.dispatchEvent(new Event("blur"));
    });
    expect(screen.queryByRole("menu")).toBeNull();
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
