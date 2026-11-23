import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const api = vi.hoisted(() => ({ getDeviceGlance: vi.fn() }));
vi.mock("./api", () => api);

import { DeviceGlanceRow, GLANCE_MS, glanceLine, glanceThumb } from "./DeviceGlance";

const JPG = "data:image/jpeg;base64,/9j/4AA=";

describe("device glance", () => {
  beforeEach(() => api.getDeviceGlance.mockReset());
  afterEach(() => vi.useRealTimers());

  it("says Sharing while the hub is on, else Last frame", () => {
    expect(glanceLine({ sharing: true, frame: JPG })).toBe("Sharing");
    expect(glanceLine({ sharing: false, frame: JPG })).toBe("Last frame");
  });

  it("draws only an inline image as the thumbnail", () => {
    expect(glanceThumb({ sharing: false, frame: JPG })).toBe(JPG);
    expect(glanceThumb({ sharing: true, frame: null })).toBeNull();
    expect(glanceThumb({ sharing: false, frame: "https://example.com/a.png" })).toBeNull();
    expect(glanceThumb({ sharing: false, frame: "data:text/html;base64,PGI+" })).toBeNull();
    expect(glanceThumb({ sharing: false, frame: "javascript:alert(1)" })).toBeNull();
  });

  it("shows the newest frame and opens Devices on a tap", async () => {
    api.getDeviceGlance.mockResolvedValue({ sharing: false, frame: JPG });
    const onOpen = vi.fn();
    const { container } = render(<DeviceGlanceRow onOpen={onOpen} />);
    const row = await screen.findByRole("button", { name: "Last frame. Open Devices" });
    expect(row).toHaveTextContent("Last frame");
    expect(container.querySelector("img")).toHaveAttribute("src", JPG);
    await userEvent.click(row);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("stays hidden with nothing to show or no hub, and shows once sharing starts", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.getDeviceGlance.mockResolvedValueOnce(null).mockRejectedValueOnce(new Error("unknown command"));
    render(<DeviceGlanceRow onOpen={() => {}} />);
    await act(() => Promise.resolve());
    expect(screen.queryByRole("button")).toBeNull();
    await act(() => vi.advanceTimersByTimeAsync(GLANCE_MS));
    expect(screen.queryByRole("button")).toBeNull();
    api.getDeviceGlance.mockResolvedValue({ sharing: true, frame: null });
    await act(() => vi.advanceTimersByTimeAsync(GLANCE_MS));
    const row = screen.getByRole("button", { name: "Sharing. Open Devices" });
    expect(row.querySelector("img")).toBeNull();
    expect(api.getDeviceGlance).toHaveBeenCalledTimes(3);
  });

  it("reads only while the chat is on screen", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.getDeviceGlance.mockResolvedValue({ sharing: true, frame: null });
    const { rerender } = render(<DeviceGlanceRow active={false} onOpen={() => {}} />);
    await act(() => vi.advanceTimersByTimeAsync(GLANCE_MS * 4));
    expect(api.getDeviceGlance).not.toHaveBeenCalled();
    rerender(<DeviceGlanceRow active onOpen={() => {}} />);
    expect(await screen.findByRole("button", { name: "Sharing. Open Devices" })).toBeInTheDocument();
    expect(api.getDeviceGlance).toHaveBeenCalledTimes(1);
    rerender(<DeviceGlanceRow active={false} onOpen={() => {}} />);
    await act(() => vi.advanceTimersByTimeAsync(GLANCE_MS * 4));
    expect(api.getDeviceGlance).toHaveBeenCalledTimes(1);
  });
});
