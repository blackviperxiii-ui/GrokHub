import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";

const api = vi.hoisted(() => ({
  CORE_ONLY: { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false },
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
}));

vi.mock("./api", () => api);
// Each page has its own tests; here only the tabs matter.
vi.mock("./Chat", () => ({
  Chat: ({ prefill }: { prefill?: { text: string; n: number; conversationId?: string } }) => (
    <p>Chat page{prefill && `: ${prefill.text} in ${prefill.conversationId ?? "this chat"}`}</p>
  ),
}));
vi.mock("./Journal", () => ({
  Journal: ({ onMission }: { onMission: (m: { conversationId: string; title: string; prompt: string }) => void }) => (
    <button type="button" onClick={() => onMission({ conversationId: "shell-m1", title: "Q", prompt: "Mission: Q" })}>
      To mission
    </button>
  ),
}));
vi.mock("./Automations", () => ({ Automations: () => null }));
vi.mock("./Memory", () => ({ Memory: () => null }));
vi.mock("./Connectors", () => ({ Connectors: () => null }));
vi.mock("./Pulse", () => ({ Pulse: () => <p>Pulse page</p> }));
vi.mock("./Settings", () => ({
  Settings: ({ caps, goTo }: { caps: Capabilities; goTo?: { section: string } }) => (
    <p>
      Settings for {caps.desktopControl ? "a desktop" : "a phone"}
      {goTo && ` at ${goTo.section}`}
    </p>
  ),
}));

import { App } from "./App";

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true, webAccess: true };

function tabs() {
  const nav = screen.getByRole("navigation", { name: "Views" });
  return within(nav).getAllByRole("button").map((tab) => tab.textContent);
}

describe("App", () => {
  beforeEach(() => { api.getCapabilities.mockReset(); });

  it("shows every tab on a desktop and hands Settings the report", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    expect(screen.getByText("Pulse page")).toBeInTheDocument();
    expect(tabs()).toEqual([
      "Chat",
      "Pulse",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Settings",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(screen.getByText("Settings for a desktop")).toBeInTheDocument();
  });

  it("hides Pulse on a phone", async () => {
    api.getCapabilities.mockResolvedValue(api.CORE_ONLY);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(tabs()).toEqual(["Chat", "Journal", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("leaves the Journal off the web app", async () => {
    api.getCapabilities.mockResolvedValue({ ...api.CORE_ONLY, remote: true });
    render(<App />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Journal" })).toBeNull());
    expect(tabs()).toEqual(["Chat", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("To mission puts the prompt in the mission's chat and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Journal" }));
    await userEvent.click(screen.getByRole("button", { name: "To mission" }));
    expect(screen.getByText("Chat page: Mission: Q in shell-m1")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
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

  it("Ctrl+K opens the palette, and a pick opens its page", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.keyboard("{Control>}k{/Control}");
    const dialog = screen.getByRole("dialog", { name: "Command palette" });
    expect(within(dialog).getByLabelText("Search pages and commands")).toHaveFocus();
    await userEvent.keyboard("local{Enter}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(screen.getByText("Settings for a desktop at localAi")).toBeVisible();
  });

  it("the Command button opens it too, and Ask Grok fills the chat box", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    await userEvent.keyboard("tidy the board");
    await userEvent.click(screen.getByRole("option", { name: "Ask Grok: tidy the board" }));
    expect(screen.getByText("Chat page: tidy the board in this chat")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });
});
