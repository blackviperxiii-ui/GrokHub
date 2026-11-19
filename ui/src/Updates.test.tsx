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
  rolledBack: null,
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

  it("says what each channel gets, without the old app's name", async () => {
    api.getUpdateStatus.mockResolvedValue(STABLE);
    render(<Updates />);
    expect(
      await screen.findByText("Beta gets every new build first. Stable gets only full releases."),
    ).toBeVisible();
    expect(screen.queryByText(/cabin/i)).toBeNull();
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

  it("says when the last update was rolled back", async () => {
    api.getUpdateStatus.mockResolvedValue({
      ...BETA,
      rolledBack: "3.5.0-beta.48 didn't stay running after the update, so 3.5.0-beta.47 was put back.",
    });
    render(<Updates />);
    expect(
      await screen.findByText(
        "3.5.0-beta.48 didn't stay running after the update, so 3.5.0-beta.47 was put back.",
      ),
    ).toBeTruthy();
  });

  it("a copy that updates outside the app says how and offers no button", async () => {
    api.getUpdateStatus.mockResolvedValue({
      ...STABLE,
      updatable: false,
      note: "This copy updates outside the app: use your package manager for a .deb or AUR install, or run scripts/install.sh --from-source --channel stable again in the clone you built it from.",
    });
    render(<Updates />);
    expect(
      await screen.findByText("This copy updates outside the app: use your package manager for a .deb or AUR install, or run scripts/install.sh --from-source --channel stable again in the clone you built it from."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
