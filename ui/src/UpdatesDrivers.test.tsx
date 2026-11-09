import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { UpdateOffer, UpdatesPage } from "./api";

const api = vi.hoisted(() => ({
  getUpdatesPage: vi.fn<() => Promise<UpdatesPage>>(),
  runUpdateOffer: vi.fn<(offer: UpdateOffer) => Promise<string>>(),
  installShellUpdate: vi.fn<() => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { itemLine, UpdatesDrivers } from "./UpdatesDrivers";

const PAGE: UpdatesPage = {
  headline: "3 waiting",
  driver: "GPU driver 560.35",
  groups: [
    { kind: "loadout", title: "Loadout", line: "3.6.0 is ready (you have 3.5.0)", items: [], offer: { offer: "updateLoadout", label: "Update", held: null } },
    {
      kind: "steam",
      title: "Steam games",
      line: "1 game waiting",
      items: [{ name: "Hades II", from: null, to: null }],
      offer: { offer: "openSteamDownloads", label: "Open Steam downloads", held: null },
    },
    {
      kind: "system",
      title: "System",
      line: "1 package waiting",
      items: [{ name: "linux", from: "6.10.1", to: "6.10.2" }],
      offer: { offer: "updateSystem", label: "Update system", held: null },
    },
  ],
};

beforeEach(() => {
  vi.clearAllMocks();
  api.getUpdatesPage.mockResolvedValue(PAGE);
  api.runUpdateOffer.mockResolvedValue("System updated. A restart may be needed for some of it.");
  api.installShellUpdate.mockResolvedValue();
});

describe("Updates & Drivers", () => {
  it("writes an item with its versions when the source gives them", () => {
    expect(itemLine({ name: "linux", from: "6.10.1", to: "6.10.2" })).toBe("linux 6.10.1 → 6.10.2");
    expect(itemLine({ name: "kernel", from: null, to: "6.10.2" })).toBe("kernel 6.10.2");
    expect(itemLine({ name: "Hades II", from: null, to: null })).toBe("Hades II");
  });

  it("lists each group with its one offer, and the system update says how it went", async () => {
    render(<UpdatesDrivers />);
    expect(await screen.findByText("3 waiting")).toBeInTheDocument();
    expect(screen.getByText("GPU driver 560.35")).toBeInTheDocument();
    const system = screen.getByRole("generic", { name: "System" });
    expect(within(system).getByText("linux 6.10.1 → 6.10.2")).toBeInTheDocument();
    await userEvent.click(within(system).getByRole("button", { name: "Update system" }));
    expect(api.runUpdateOffer).toHaveBeenCalledWith("updateSystem");
    expect(await screen.findByText("System updated. A restart may be needed for some of it.")).toBeInTheDocument();
    // It reads the page again so the list shows what's left.
    expect(api.getUpdatesPage).toHaveBeenCalledTimes(2);
  });

  it("updates the app itself through its own signed updater", async () => {
    render(<UpdatesDrivers />);
    const own = await screen.findByRole("generic", { name: "Loadout" });
    await userEvent.click(within(own).getByRole("button", { name: "Update" }));
    expect(api.installShellUpdate).toHaveBeenCalledTimes(1);
    expect(api.runUpdateOffer).not.toHaveBeenCalled();
  });

  it("holds a heavy update while a game runs and says why", async () => {
    api.getUpdatesPage.mockResolvedValue({
      ...PAGE,
      groups: [{ ...PAGE.groups[2], offer: { offer: "updateSystem", label: "Update system", held: "Waits until your game closes" } }],
    });
    render(<UpdatesDrivers />);
    const button = await screen.findByRole("button", { name: "Update system" });
    expect(button).toBeDisabled();
    expect(screen.getByText("Waits until your game closes")).toBeInTheDocument();
  });

  it("puts the old driver back after a driver update, then reads the page again", async () => {
    api.getUpdatesPage.mockResolvedValue({
      ...PAGE,
      groups: [
        {
          kind: "drivers",
          title: "Drivers",
          line: "Updated. Restart to load it. Put old driver back if a game misbehaves.",
          items: [{ name: "nvidia-utils", from: "560.35.03-1", to: "565.57.01-1" }],
          offer: { offer: "rollBackDrivers", label: "Put old driver back", held: null },
        },
      ],
    });
    api.runUpdateOffer.mockResolvedValue("The old driver is back. Restart to load it.");
    render(<UpdatesDrivers />);
    const drivers = await screen.findByRole("generic", { name: "Drivers" });
    expect(within(drivers).getByText("nvidia-utils 560.35.03-1 → 565.57.01-1")).toBeInTheDocument();
    await userEvent.click(within(drivers).getByRole("button", { name: "Put old driver back" }));
    expect(api.runUpdateOffer).toHaveBeenCalledWith("rollBackDrivers");
    expect(await screen.findByText("The old driver is back. Restart to load it.")).toBeInTheDocument();
    expect(api.getUpdatesPage).toHaveBeenCalledTimes(2);
  });
});
