import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({
  getDiagnostics: vi.fn(() => new Promise(() => {})),
  listChats: vi.fn(() => Promise.resolve([])),
}));

const desk = vi.hoisted(() => ({
  desktopNyxGet: vi.fn(() =>
    Promise.resolve({
      enabled: false,
      undocked: false,
      behind: false,
      adult: false,
      adultsOk: false,
      outfit: null,
      outfitFile: "brand/nyx-hanging-out.jpg",
      outfits: [],
      pos: null,
      effects: { transparent: true, onTop: true },
      companion: "Nyx",
      hiddenForGame: false,
      still: false,
      look: "idle",
    }),
  ),
  desktopNyxSet: vi.fn(),
  onDesktopNyx: vi.fn((fn: (s: unknown) => void) => {
    deskHeard.fn = fn;
    return Promise.resolve(() => {});
  }),
}));
const deskHeard = vi.hoisted(() => ({ fn: null as ((s: unknown) => void) | null }));

vi.mock("./api", () => api);
vi.mock("./desktopNyxApi", () => desk);
const transport = vi.hoisted(() => ({ shell: false }));
vi.mock("./transport", () => ({ inShell: () => transport.shell }));

import { CHANNEL_NOTE, Labs, PLUGINS_MOVED } from "./Labs";

describe("Labs", () => {
  beforeEach(() => {
    api.getDiagnostics.mockClear();
    api.listChats.mockClear();
  });

  it("points to About for the channel and to the Marketplace for plugins", () => {
    render(<Labs />);
    expect(
      screen.getByText("Switch between beta and stable on your computer, in About, under Updates."),
    ).toBeVisible();
    expect(screen.queryByText(/cabin/)).toBeNull();
    expect(screen.getByText(PLUGINS_MOVED)).toBeVisible();
    expect(screen.queryByRole("heading", { name: /Plugins/ })).toBeNull();
  });

  it("follows a desktop companion change made elsewhere, such as the tray", async () => {
    transport.shell = true;
    try {
      const on = { ...(await desk.desktopNyxGet()), enabled: true };
      desk.desktopNyxGet.mockResolvedValueOnce(on);
      render(<Labs />);
      expect(await screen.findByText("Stay behind windows")).toBeVisible();
      act(() => deskHeard.fn?.({ ...on, enabled: false }));
      expect(screen.queryByText("Stay behind windows")).toBeNull();
      expect(desk.desktopNyxSet).not.toHaveBeenCalled();
    } finally {
      transport.shell = false;
    }
  });

  it("says she is hidden while you game, and shows the usual note again after", async () => {
    transport.shell = true;
    try {
      const out = { ...(await desk.desktopNyxGet()), enabled: true, undocked: true };
      desk.desktopNyxGet.mockResolvedValueOnce({ ...out, hiddenForGame: true });
      render(<Labs />);
      expect(await screen.findByText("On the desktop, hidden while you game.")).toBeVisible();
      act(() => deskHeard.fn?.(out));
      expect(screen.queryByText("On the desktop, hidden while you game.")).toBeNull();
      expect(
        screen.getByText("On the desktop. Use Bring her back on her dock or right-click menu."),
      ).toBeVisible();
    } finally {
      transport.shell = false;
    }
  });

  it("on this computer, points to About and Updates for the channel, not the cabin", async () => {
    transport.shell = true;
    try {
      render(<Labs />);
      expect(
        await screen.findByText("Switch between beta and stable in About, under Updates."),
      ).toBeVisible();
      expect(CHANNEL_NOTE).not.toMatch(/cabin/i);
    } finally {
      transport.shell = false;
    }
  });
});
