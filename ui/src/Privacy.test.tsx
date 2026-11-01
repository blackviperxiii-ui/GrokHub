import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PrivacyPage } from "./api";

const api = vi.hoisted(() => ({
  getPrivacy: vi.fn<() => Promise<PrivacyPage>>(),
  setOfflineOnly: vi.fn<(on: boolean) => Promise<PrivacyPage>>(),
  openDataFolder: vi.fn<() => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { OFFLINE_NOT_READY, Privacy } from "./Privacy";

function page(over: Partial<PrivacyPage> = {}): PrivacyPage {
  return {
    offlineOnly: false,
    localReady: true,
    sent: "3 calls sent off-device this week",
    hosts: [
      { host: "api.x.ai", calls: 2 },
      { host: "hn.algolia.com", calls: 1 },
    ],
    encrypted: true,
    encryptedNote: "Personal memory and the logs are sealed with your keyring.",
    dataFolder: "/home/user/.config/GrokHub",
    canOpenFolder: true,
    ...over,
  };
}

describe("Privacy", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.getPrivacy.mockResolvedValue(page());
  });

  it("shows the week, the data folder and sealed memory, with Offline only off", async () => {
    render(<Privacy />);
    expect(await screen.findByLabelText("Offline only")).not.toBeChecked();
    expect(screen.getByText("3 calls sent off-device this week")).toBeInTheDocument();
    expect(screen.getByText("api.x.ai")).toBeInTheDocument();
    expect(screen.getByText("/home/user/.config/GrokHub")).toBeInTheDocument();
    const sealed = screen.getByLabelText("Encrypted memory");
    expect(sealed).toBeChecked();
    expect(sealed).toBeDisabled();
  });

  it("Offline only and Open data folder call their commands", async () => {
    api.setOfflineOnly.mockResolvedValue(page({ offlineOnly: true }));
    api.openDataFolder.mockResolvedValue();
    render(<Privacy />);
    await userEvent.click(await screen.findByLabelText("Offline only"));
    expect(api.setOfflineOnly).toHaveBeenCalledWith(true);
    expect(await screen.findByText("Offline only is on.")).toBeInTheDocument();
    expect(screen.getByLabelText("Offline only")).toBeChecked();
    await userEvent.click(screen.getByRole("button", { name: "Open data folder" }));
    expect(api.openDataFolder).toHaveBeenCalledTimes(1);
  });

  it("asks first when no local model is ready, and Cancel leaves it off", async () => {
    api.getPrivacy.mockResolvedValue(page({ localReady: false }));
    api.setOfflineOnly.mockResolvedValue(page({ offlineOnly: true, localReady: false }));
    render(<Privacy />);
    await userEvent.click(await screen.findByLabelText("Offline only"));
    expect(screen.getByText(OFFLINE_NOT_READY)).toBeInTheDocument();
    expect(api.setOfflineOnly).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByLabelText("Offline only")).not.toBeChecked();

    await userEvent.click(screen.getByLabelText("Offline only"));
    await userEvent.click(screen.getByRole("button", { name: "Turn on anyway" }));
    expect(api.setOfflineOnly).toHaveBeenCalledWith(true);
    expect(screen.queryByText(OFFLINE_NOT_READY)).toBeNull();
  });

  it("reads offline.toml again when the window gets focus", async () => {
    render(<Privacy />);
    expect(await screen.findByLabelText("Offline only")).not.toBeChecked();
    api.getPrivacy.mockResolvedValue(page({ offlineOnly: true }));
    fireEvent.focus(window);
    expect(await screen.findByRole("switch", { checked: true, name: "Offline only" })).toBeInTheDocument();
    expect(api.getPrivacy).toHaveBeenCalledTimes(2);
  });

  it("the web app sees it with nothing to change", async () => {
    render(<Privacy readOnly />);
    expect(await screen.findByText("3 calls sent off-device this week")).toBeInTheDocument();
    expect(screen.getByText("Off")).toBeInTheDocument();
    expect(screen.queryByRole("button")).toBeNull();
    expect(screen.queryByRole("switch")).toBeNull();
  });
});
