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
    hiddenForGame: false,
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

  it("tells the app which parts take the mouse: her pedestal and figure, and the menu while it is open", async () => {
    // The real layout: the root and the stage fill the 320x480 window.
    const rects: Record<string, [number, number, number, number]> = {
      "nyx-desk": [0, 0, 320, 480],
      "nyx-desk-stage": [0, 0, 320, 480],
      "nyx-desk-pedestal": [26, 259, 268, 202],
      "nyx-desk-figure": [60, 40, 200, 300],
      "nyx-desk-menu": [10, 20, 120, 80],
    };
    const box = vi.spyOn(Element.prototype, "getBoundingClientRect").mockImplementation(function (this: Element) {
      const name = Object.keys(rects).find((c) => this.classList.contains(c));
      const [left, top, width, height] = name ? rects[name] : [0, 0, 0, 0];
      return { left, top, width, height } as DOMRect;
    });
    const pedestal = { x: 26, y: 259, w: 268, h: 202 };
    const figure = { x: 60, y: 40, w: 200, h: 300 };
    try {
      render(<NyxDesk />);
      const root = await screen.findByLabelText("Nyx");
      await waitFor(() => expect(api.desktopNyxSetHit).toHaveBeenLastCalledWith([pedestal, figure]));
      await userEvent.pointer({ keys: "[MouseRight]", target: root });
      await screen.findByRole("menu");
      await waitFor(() =>
        expect(api.desktopNyxSetHit).toHaveBeenLastCalledWith([pedestal, figure, { x: 10, y: 20, w: 120, h: 80 }]),
      );
      act(() => {
        window.dispatchEvent(new Event("blur"));
      });
      await waitFor(() => expect(api.desktopNyxSetHit).toHaveBeenLastCalledWith([pedestal, figure]));
    } finally {
      box.mockRestore();
    }
  });

  it("drags her once a left press on her moves, and a press alone closes her menu", async () => {
    const { container } = render(<NyxDesk />);
    await screen.findByLabelText("Nyx");
    const figure = container.querySelector(".nyx-desk-figure");
    if (!figure) throw new Error("no figure");
    await userEvent.pointer({ keys: "[MouseRight]", target: figure });
    const menu = await screen.findByRole("menu");
    // A left press on the menu neither drags nor closes it.
    await userEvent.pointer({ keys: "[MouseLeft>]", target: within(menu).getByRole("menuitem", { name: "Gaming" }) });
    await userEvent.pointer({ coords: { clientX: 50, clientY: 50 } });
    await userEvent.pointer({ keys: "[/MouseLeft]" });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(0);
    // A press on her closes the menu without dragging.
    await userEvent.pointer({ keys: "[MouseLeft>]", target: figure, coords: { clientX: 100, clientY: 100 } });
    expect(screen.queryByRole("menu")).toBeNull();
    await userEvent.pointer({ target: figure, coords: { clientX: 102, clientY: 101 } });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(0);
    // Past 4 px, the window drags, once.
    await userEvent.pointer({ target: figure, coords: { clientX: 110, clientY: 100 } });
    await userEvent.pointer({ target: figure, coords: { clientX: 130, clientY: 100 } });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(1);
    await userEvent.pointer({ keys: "[/MouseLeft]", target: figure });
    // The right button never drags.
    await userEvent.pointer({ keys: "[MouseRight>]", target: figure, coords: { clientX: 100, clientY: 100 } });
    await userEvent.pointer({ target: figure, coords: { clientX: 140, clientY: 100 } });
    await userEvent.pointer({ keys: "[/MouseRight]", target: figure });
    expect(api.desktopNyxDrag).toHaveBeenCalledTimes(1);
  });

  it("closes her menu on Escape", async () => {
    render(<NyxDesk />);
    const root = await screen.findByLabelText("Nyx");
    await userEvent.pointer({ keys: "[MouseRight]", target: root });
    await screen.findByRole("menu");
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("reports again when her figure swaps its element on its own", async () => {
    const { container } = render(<NyxDesk />);
    await screen.findByLabelText("Nyx");
    const stage = container.querySelector(".nyx-desk-stage");
    if (!stage) throw new Error("no stage");
    await waitFor(() => expect(api.desktopNyxSetHit).toHaveBeenCalled());
    const before = api.desktopNyxSetHit.mock.calls.length;
    const swapped = document.createElement("img");
    swapped.className = "nyx-desk-figure";
    await act(async () => {
      stage.replaceChildren(stage.children[0], swapped);
    });
    await waitFor(() => expect(api.desktopNyxSetHit.mock.calls.length).toBeGreaterThan(before));
    expect(api.desktopNyxSetHit.mock.lastCall?.[0]).toHaveLength(2);
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
