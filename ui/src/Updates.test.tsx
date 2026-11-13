import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateChannel, UpdateView } from "./api";

const api = vi.hoisted(() => ({
  getUpdateStatus: vi.fn<() => Promise<UpdateView>>(),
  setUpdateChannel: vi.fn<(channel: UpdateChannel) => Promise<UpdateView>>(),
  checkShellUpdate: vi.fn<() => Promise<UpdateView>>(),
  installShellUpdate: vi.fn<() => Promise<void>>(),
  onUpdateAvailable: vi.fn<(handler: (view: UpdateView) => void) => Promise<() => void>>(),
  handler: null as ((view: UpdateView) => void) | null,
}));

vi.mock("./api", () => api);

import { UP_TO_DATE, Updates } from "./Updates";

const STABLE: UpdateView = {
  version: "3.5.0",
  channel: "stable",
  updatable: true,
  note: null,
  available: null,
};
const BETA: UpdateView = { ...STABLE, version: "3.5.0-beta.47", channel: "beta" };

describe("Updates", () => {
  beforeEach(() => {
    for (const fn of [
      api.getUpdateStatus,
      api.setUpdateChannel,
      api.checkShellUpdate,
      api.installShellUpdate,
      api.onUpdateAvailable,
    ]) {
      fn.mockReset();
    }
    api.onUpdateAvailable.mockImplementation(async (handler) => {
      api.handler = handler;
      return () => {};
    });
  });

  it("switches to Beta through the shared channel", async () => {
    api.getUpdateStatus.mockResolvedValue(STABLE);
    api.setUpdateChannel.mockResolvedValue(BETA);
    render(<Updates />);
    const beta = await screen.findByRole("radio", { name: "Beta" });
    expect(screen.getByRole("radio", { name: "Stable" })).toBeChecked();
    await userEvent.click(beta);
    expect(api.setUpdateChannel).toHaveBeenCalledWith("beta");
    expect(await screen.findByRole("radio", { name: "Beta" })).toBeChecked();
  });

  it("checks, offers the found version and installs it on a click", async () => {
    api.getUpdateStatus.mockResolvedValue(BETA);
    api.checkShellUpdate.mockResolvedValue({ ...BETA, available: "3.5.0-beta.48" });
    api.installShellUpdate.mockResolvedValue();
    render(<Updates />);
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    const install = await screen.findByRole("button", { name: "Install 3.5.0-beta.48 and restart" });
    expect(api.installShellUpdate).not.toHaveBeenCalled();
    await userEvent.click(install);
    expect(api.installShellUpdate).toHaveBeenCalledTimes(1);
  });

  it("says so when there's nothing newer", async () => {
    api.getUpdateStatus.mockResolvedValue(BETA);
    api.checkShellUpdate.mockResolvedValue(BETA);
    render(<Updates />);
    await userEvent.click(await screen.findByRole("button", { name: "Check now" }));
    expect(await screen.findByRole("status")).toHaveTextContent(UP_TO_DATE);
  });

  it("shows a background find without installing it", async () => {
    api.getUpdateStatus.mockResolvedValue(BETA);
    render(<Updates />);
    await screen.findByRole("button", { name: "Check now" });
    act(() => api.handler?.({ ...BETA, available: "3.5.0-beta.49" }));
    expect(screen.getByRole("button", { name: "Install 3.5.0-beta.49 and restart" })).toBeInTheDocument();
    expect(api.installShellUpdate).not.toHaveBeenCalled();
  });

  it("a .deb install points at the package manager and offers no button", async () => {
    api.getUpdateStatus.mockResolvedValue({
      ...STABLE,
      updatable: false,
      note: "Installed from the .deb: update it with your package manager.",
    });
    render(<Updates />);
    expect(
      await screen.findByText("Installed from the .deb: update it with your package manager."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
