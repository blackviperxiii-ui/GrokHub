import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Devices as Shape } from "./api";

const api = vi.hoisted(() => ({
  getDevices: vi.fn<() => Promise<Shape>>(),
  forgetAllDevices: vi.fn<() => Promise<Shape>>(),
}));

vi.mock("./api", () => api);

import { ago, browserLine, Devices, NONE_PAIRED, OFF_NOTE, SIGNED_OUT } from "./Devices";

const MINUTE = 60_000;
const NOW = Date.now();
const ON: Shape = {
  computer: "desk-1",
  on: true,
  lan: "https://192.168.1.20:4620",
  browsers: [
    { name: "Firefox on Android", firstSeen: NOW - 30 * MINUTE, lastCall: NOW, chats: 2 },
    { name: "Safari on iPhone", firstSeen: NOW - 3 * 60 * MINUTE, lastCall: NOW - 5 * MINUTE, chats: 1 },
  ],
};
const SIGNED_OUT_VIEW: Shape = { ...ON, browsers: [] };
const OFF: Shape = { computer: "desk-1", on: false, lan: "", browsers: [] };

describe("ago", () => {
  it("says how long ago in minutes, hours or days", () => {
    expect(ago(NOW, NOW)).toBe("just now");
    expect(ago(NOW + 5_000, NOW)).toBe("just now");
    expect(ago(NOW - 59_999, NOW)).toBe("just now");
    expect(ago(NOW - 5 * MINUTE, NOW)).toBe("5 min ago");
    expect(ago(NOW - 125 * MINUTE, NOW)).toBe("2 h ago");
    expect(ago(NOW - 24 * 60 * MINUTE, NOW)).toBe("1 day ago");
    expect(ago(NOW - 3 * 24 * 60 * MINUTE, NOW)).toBe("3 days ago");
  });

  it("puts a browser's pairing, last use and chats on one line", () => {
    expect(browserLine(ON.browsers[1], NOW)).toBe("Paired 3 h ago · Last used 5 min ago · 1 chat");
  });
});

describe("Devices", () => {
  beforeEach(() => {
    api.getDevices.mockReset();
    api.forgetAllDevices.mockReset();
  });

  it("shows this computer, web access and the paired browsers", async () => {
    api.getDevices.mockResolvedValue(ON);
    const open = vi.fn();
    render(<Devices onOpenWebAccess={open} />);
    const list = await screen.findByRole("list", { name: "Paired browsers" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((row) => row.querySelector(".card-title")?.textContent)).toEqual([
      "Firefox on Android",
      "Safari on iPhone",
    ]);
    expect(rows[0]).toHaveTextContent("Last used just now · 2 chats");
    expect(screen.getByLabelText("This computer")).toHaveTextContent("desk-1");
    expect(screen.getByText("https://192.168.1.20:4620")).toBeInTheDocument();
    expect(screen.getByLabelText("Web access")).toHaveTextContent("On");

    await userEvent.click(screen.getByRole("button", { name: "Open Web access settings" }));
    expect(open).toHaveBeenCalledTimes(1);
  });

  it("signs out every browser with one click", async () => {
    api.getDevices.mockResolvedValue(ON);
    api.forgetAllDevices.mockResolvedValue(SIGNED_OUT_VIEW);
    render(<Devices onOpenWebAccess={() => {}} />);
    await screen.findByRole("list", { name: "Paired browsers" });

    await userEvent.click(screen.getByRole("button", { name: "Sign out all browsers" }));
    expect(api.forgetAllDevices).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent(SIGNED_OUT);
    expect(screen.queryByRole("list", { name: "Paired browsers" })).toBeNull();
    expect(screen.getByText(NONE_PAIRED)).toBeInTheDocument();
  });

  it("says web access is off and keeps the button off", async () => {
    api.getDevices.mockResolvedValue(OFF);
    render(<Devices onOpenWebAccess={() => {}} />);
    expect(await screen.findByText(OFF_NOTE)).toBeInTheDocument();
    expect(screen.getByLabelText("Web access")).toHaveTextContent("Off");
    expect(screen.getByRole("button", { name: "Sign out all browsers" })).toBeDisabled();
    expect(screen.queryByText("https://192.168.1.20:4620")).toBeNull();
  });

  it("shows why the list couldn't be read", async () => {
    api.getDevices.mockRejectedValue("Not on this platform.");
    render(<Devices onOpenWebAccess={() => {}} />);
    expect(await screen.findByRole("status")).toHaveTextContent("Not on this platform.");
  });
});
