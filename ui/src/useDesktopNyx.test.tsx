import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DesktopNyxState } from "./desktopNyxApi";

const api = vi.hoisted(() => ({
  desktopNyxGet: vi.fn<() => Promise<DesktopNyxState>>(),
  onDesktopNyx: vi.fn<(fn: (s: DesktopNyxState) => void) => Promise<() => void>>(),
}));
vi.mock("./desktopNyxApi", () => api);

import { useDesktopNyx } from "./useDesktopNyx";

function state(over: Partial<DesktopNyxState> = {}): DesktopNyxState {
  return {
    enabled: true,
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
    ...over,
  };
}

function Probe({ active = true, onError }: { active?: boolean; onError?: (e: unknown) => void }) {
  const [desk] = useDesktopNyx(active, onError);
  return <p>{desk === null ? "none" : `undocked=${String(desk.undocked)}`}</p>;
}

let heard: ((s: DesktopNyxState) => void) | null = null;
const unlisten = vi.fn();

describe("useDesktopNyx", () => {
  beforeEach(() => {
    heard = null;
    unlisten.mockReset();
    api.desktopNyxGet.mockReset();
    api.onDesktopNyx.mockReset();
    api.onDesktopNyx.mockImplementation((fn) => {
      heard = fn;
      return Promise.resolve(unlisten);
    });
  });

  it("reads once, then follows every change, and stops listening on unmount", async () => {
    api.desktopNyxGet.mockResolvedValue(state({ undocked: false }));
    const { unmount } = render(<Probe />);
    expect(await screen.findByText("undocked=false")).toBeInTheDocument();
    act(() => heard?.(state({ undocked: true })));
    expect(screen.getByText("undocked=true")).toBeInTheDocument();
    expect(api.desktopNyxGet).toHaveBeenCalledTimes(1);
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("keeps a change heard while the read is out over the read's older answer", async () => {
    let answer: (s: DesktopNyxState) => void = () => {};
    api.desktopNyxGet.mockReturnValue(new Promise((done) => (answer = done)));
    render(<Probe />);
    await waitFor(() => expect(heard).not.toBeNull());
    act(() => heard?.(state({ undocked: true })));
    await act(async () => answer(state({ undocked: false })));
    expect(screen.getByText("undocked=true")).toBeInTheDocument();
  });

  it("neither reads nor listens while inactive", () => {
    render(<Probe active={false} />);
    expect(screen.getByText("none")).toBeInTheDocument();
    expect(api.desktopNyxGet).not.toHaveBeenCalled();
    expect(api.onDesktopNyx).not.toHaveBeenCalled();
  });

  it("hands a failed read to onError", async () => {
    api.desktopNyxGet.mockRejectedValue(new Error("no shell"));
    const onError = vi.fn();
    render(<Probe onError={onError} />);
    await waitFor(() => expect(onError).toHaveBeenCalledTimes(1));
    expect(String(onError.mock.calls[0][0])).toBe("Error: no shell");
    expect(screen.getByText("none")).toBeInTheDocument();
  });
});
