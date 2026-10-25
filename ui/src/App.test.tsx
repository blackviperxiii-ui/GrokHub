import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";

const api = vi.hoisted(() => ({
  CORE_ONLY: { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false },
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
}));

vi.mock("./api", () => api);
// Each page has its own tests; here only the tabs matter.
vi.mock("./Chat", () => ({ Chat: () => <p>Chat page</p> }));
vi.mock("./Automations", () => ({ Automations: () => null }));
vi.mock("./Memory", () => ({ Memory: () => null }));
vi.mock("./Connectors", () => ({ Connectors: () => null }));
vi.mock("./Pulse", () => ({ Pulse: () => <p>Pulse page</p> }));
vi.mock("./Settings", () => ({
  Settings: ({ caps }: { caps: Capabilities }) => (
    <p>Settings for {caps.desktopControl ? "a desktop" : "a phone"}</p>
  ),
}));

import { App } from "./App";

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true, webAccess: true };

function tabs() {
  return screen.getAllByRole("button").map((tab) => tab.textContent);
}

describe("App", () => {
  beforeEach(() => { api.getCapabilities.mockReset(); });

  it("shows every tab on a desktop and hands Settings the report", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    expect(screen.getByText("Pulse page")).toBeInTheDocument();
    expect(tabs()).toEqual(["Chat", "Pulse", "Automations", "Memory", "Connectors", "Settings"]);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByText("Settings for a desktop")).toBeInTheDocument();
  });

  it("hides Pulse on a phone", async () => {
    api.getCapabilities.mockResolvedValue(api.CORE_ONLY);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(tabs()).toEqual(["Chat", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("falls back to the core features when the report fails", async () => {
    api.getCapabilities.mockRejectedValue("no report");
    render(<App />);
    await waitFor(() =>
      expect(api.getCapabilities.mock.settledResults[0]?.type).toBe("rejected"),
    );
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pulse" })).toBeNull();
  });
});
