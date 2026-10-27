import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WebAccess as Shape } from "./api";

const api = vi.hoisted(() => ({
  getWebAccess: vi.fn<() => Promise<Shape>>(),
  setWebAccess: vi.fn<(on: boolean) => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { REFRESH_MS, WEB_NOTE, WebAccess } from "./WebAccess";

const OFF: Shape = { on: false, code: "", links: [] };
const ON: Shape = {
  on: true,
  code: "0123456789abcdef0123456789abcdef0123456789abcdef",
  links: [
    "http://192.168.1.20:4620/#code=0123456789abcdef0123456789abcdef0123456789abcdef",
    "http://127.0.0.1:4620/#code=0123456789abcdef0123456789abcdef0123456789abcdef",
  ],
};

describe("WebAccess", () => {
  beforeEach(() => {
    api.getWebAccess.mockReset();
    api.setWebAccess.mockReset();
  });

  it("turns web access on, shows the pairing links and code, and turns it off", async () => {
    api.getWebAccess.mockResolvedValue(OFF);
    api.setWebAccess.mockImplementation(async (on) => (on ? ON : OFF));
    render(<WebAccess />);
    expect(screen.getByText(WEB_NOTE)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Web access Preview" })).toBeInTheDocument();
    expect(screen.getByText("Preview")).toHaveAttribute(
      "title",
      "A preview: it works, but it runs over plain HTTP and has no sign-in beyond the pairing code.",
    );

    await userEvent.click(await screen.findByRole("button", { name: "Turn on web access" }));
    expect(api.setWebAccess).toHaveBeenLastCalledWith(true);
    expect(await screen.findByText(ON.links[0])).toBeInTheDocument();
    expect(screen.getByText(ON.links[1])).toBeInTheDocument();
    expect(screen.getByLabelText("Pairing code")).toHaveTextContent(ON.code);
    expect(
      screen.getByText(
        "Web access ends when you turn it off or quit Loadout. After 30 minutes with no browser using it, the code changes and browsers pair again.",
      ),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Turn off web access" }));
    expect(api.setWebAccess).toHaveBeenLastCalledWith(false);
    expect(await screen.findByRole("button", { name: "Turn on web access" })).toBeInTheDocument();
    expect(screen.queryByText(ON.links[0])).toBeNull();
  });

  it("warns that a paired browser can approve changes here", () => {
    api.getWebAccess.mockResolvedValue(OFF);
    render(<WebAccess />);
    expect(WEB_NOTE).toContain(
      "A paired browser can approve file changes and commands on this computer",
    );
    expect(screen.getByText(WEB_NOTE)).toBeInTheDocument();
  });

  it("shows a renewed code while web access is on", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const renewed: Shape = {
        on: true,
        code: "fedcba9876543210fedcba9876543210fedcba9876543210",
        links: ["http://127.0.0.1:4620/#code=fedcba9876543210fedcba9876543210fedcba9876543210"],
      };
      api.getWebAccess.mockResolvedValueOnce(ON).mockResolvedValue(renewed);
      render(<WebAccess />);
      expect(await screen.findByLabelText("Pairing code")).toHaveTextContent(ON.code);
      expect(api.getWebAccess).toHaveBeenCalledTimes(1);

      await act(() => vi.advanceTimersByTimeAsync(REFRESH_MS));
      expect(api.getWebAccess).toHaveBeenCalledTimes(2);
      expect(screen.getByLabelText("Pairing code")).toHaveTextContent(renewed.code);
      expect(screen.getByText(renewed.links[0])).toBeInTheDocument();
      expect(screen.queryByText(ON.links[0])).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops re-reading the code once web access is off", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      api.getWebAccess.mockResolvedValue(OFF);
      render(<WebAccess />);
      await screen.findByRole("button", { name: "Turn on web access" });
      await act(() => vi.advanceTimersByTimeAsync(REFRESH_MS * 3));
      expect(api.getWebAccess).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("says why web access didn't start", async () => {
    api.getWebAccess.mockResolvedValue(OFF);
    api.setWebAccess.mockRejectedValue("Web access couldn't start on port 4620: address in use");
    render(<WebAccess />);
    await userEvent.click(await screen.findByRole("button", { name: "Turn on web access" }));
    expect(
      await screen.findByText("Web access couldn't start on port 4620: address in use"),
    ).toBeInTheDocument();
  });
});
