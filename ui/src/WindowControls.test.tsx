import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const win = vi.hoisted(() => ({
  isMaximized: vi.fn(),
  onResized: vi.fn(),
  minimize: vi.fn(),
  toggleMaximize: vi.fn(),
  close: vi.fn(),
}));

vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => win }));

import { WindowControls } from "./WindowControls";

describe("WindowControls", () => {
  let resized: (() => void) | null;
  const unlisten = vi.fn();

  beforeEach(() => {
    resized = null;
    unlisten.mockReset();
    win.isMaximized.mockReset().mockResolvedValue(false);
    win.onResized.mockReset().mockImplementation(async (handler: () => void) => {
      resized = handler;
      return unlisten;
    });
    win.minimize.mockReset().mockResolvedValue(undefined);
    win.toggleMaximize.mockReset().mockResolvedValue(undefined);
    win.close.mockReset().mockResolvedValue(undefined);
  });

  it("minimizes, maximizes and closes the window", async () => {
    render(<WindowControls />);
    await userEvent.click(screen.getByRole("button", { name: "Minimize" }));
    await userEvent.click(screen.getByRole("button", { name: "Maximize" }));
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(win.minimize).toHaveBeenCalledTimes(1);
    expect(win.toggleMaximize).toHaveBeenCalledTimes(1);
    expect(win.close).toHaveBeenCalledTimes(1);
  });

  it("offers Restore while the window is maximized, read again on each resize", async () => {
    win.isMaximized.mockResolvedValue(true);
    const { unmount } = render(<WindowControls />);
    expect(await screen.findByRole("button", { name: "Restore" })).toBeInTheDocument();
    win.isMaximized.mockResolvedValue(false);
    await act(async () => {
      resized?.();
    });
    expect(await screen.findByRole("button", { name: "Maximize" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).toBeNull();
    unmount();
    await act(async () => {});
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("keeps working when the window refuses a call", async () => {
    win.isMaximized.mockRejectedValue(new Error("not allowed"));
    win.minimize.mockRejectedValue(new Error("not allowed"));
    render(<WindowControls />);
    await userEvent.click(screen.getByRole("button", { name: "Minimize" }));
    expect(win.minimize).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Maximize" })).toBeInTheDocument();
  });
});
