import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BotDesktop as Shape } from "./botDesktopApi";

const api = vi.hoisted(() => ({
  getBotDesktop: vi.fn<() => Promise<Shape | null>>(),
  setBotDesktop: vi.fn<(on: boolean) => Promise<Shape | null>>(),
}));
vi.mock("./botDesktopApi", () => api);

import { BotDesktop } from "./BotDesktop";

const off: Shape = { supported: true, on: false, display: null, problem: null };

describe("Bot Desktop setting", () => {
  beforeEach(() => {
    api.getBotDesktop.mockReset();
    api.setBotDesktop.mockReset();
  });

  it("stays hidden where it can't run", async () => {
    api.getBotDesktop.mockResolvedValue({ ...off, supported: false });
    const { container } = render(<BotDesktop />);
    await waitFor(() => expect(api.getBotDesktop).toHaveBeenCalledTimes(1));
    expect(container.textContent).toBe("");
  });

  it("turns on and shows the screen it runs on", async () => {
    api.getBotDesktop.mockResolvedValue(off);
    api.setBotDesktop.mockResolvedValue({ ...off, on: true, display: ":90" });
    render(<BotDesktop />);
    const toggle = await screen.findByRole("switch", { name: /work on its own screen/i });
    expect(toggle.getAttribute("aria-checked")).toBe("false");
    await userEvent.click(toggle);
    expect(api.setBotDesktop).toHaveBeenCalledWith(true);
    expect(await screen.findByText("Running on display :90.")).toBeTruthy();
    expect(toggle.getAttribute("aria-checked")).toBe("true");
  });

  it("says why a screen couldn't start and leaves the switch off", async () => {
    api.getBotDesktop.mockResolvedValue(off);
    api.setBotDesktop.mockResolvedValue({ ...off, problem: "Xvfb stopped at start (exit status: 1)" });
    render(<BotDesktop />);
    await userEvent.click(await screen.findByRole("switch", { name: /work on its own screen/i }));
    expect(await screen.findByText("Xvfb stopped at start (exit status: 1)")).toBeTruthy();
    expect(screen.getByRole("switch").getAttribute("aria-checked")).toBe("false");
  });
});
