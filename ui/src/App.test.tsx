import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";

// The App's own chat listener (the Activity Dock); tests push events through it.
const chatHandlers = vi.hoisted(() => [] as ((envelope: unknown) => void)[]);
const api = vi.hoisted(() => ({
  CORE_ONLY: { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false },
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
  getSettings: vi.fn(() => Promise.resolve({ theme: "light" })),
  onChatEvent: vi.fn((handler: (envelope: unknown) => void) => {
    chatHandlers.push(handler);
    return Promise.resolve(() => {});
  }),
}));

vi.mock("./api", () => api);
// Each page has its own tests; here only the tabs matter.
vi.mock("./Chat", () => ({
  Chat: ({
    prefill,
    onSignIn,
  }: {
    prefill?: { text: string; n: number; conversationId?: string };
    onSignIn: () => void;
  }) => (
    <div>
      <p>Chat page{prefill && `: ${prefill.text} in ${prefill.conversationId ?? "this chat"}`}</p>
      <button type="button" onClick={onSignIn}>
        Sign in
      </button>
    </div>
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
vi.mock("./Imagine", () => ({ Imagine: () => <p>Imagine page</p> }));
vi.mock("./Settings", () => ({
  Settings: ({ caps, goTo }: { caps: Capabilities; goTo?: { section: string } }) => (
    <p>
      Settings for {caps.desktopControl ? "a desktop" : "a phone"}
      {goTo && ` at ${goTo.section}`}
    </p>
  ),
}));

import { App } from "./App";

const DESKTOP: Capabilities = { desktopControl: true, pulse: true, localAi: true, tray: true, webAccess: true, plugins: true };

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
    expect(await screen.findByText("Pulse page")).toBeInTheDocument();
    expect(tabs()).toEqual([
      "Chat",
      "Pulse",
      "Imagine",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Settings",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a desktop")).toBeInTheDocument();
  });

  it("puts the saved theme on the page at startup", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    delete document.documentElement.dataset.theme;
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    delete document.documentElement.dataset.theme;
  });

  it("shows the brand mark and name in the top bar, and an icon on every tab", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    const { container } = render(<App />);
    const bar = container.querySelector(".topbar");
    expect(bar?.querySelector(".brand-mark")).toHaveTextContent("Loadout");
    expect(bar?.querySelector(".brand-mark img")).toHaveAttribute("alt", "");
    await screen.findByRole("button", { name: "Pulse" });
    const nav = screen.getByRole("navigation", { name: "Views" });
    for (const tab of within(nav).getAllByRole("button")) {
      expect(tab.querySelector("svg.tab-icon")).not.toBeNull();
      expect(tab).toHaveAttribute("title", tab.textContent);
    }
  });

  it("hides Pulse on a phone", async () => {
    api.getCapabilities.mockResolvedValue(api.CORE_ONLY);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(tabs()).toEqual(["Chat", "Imagine", "Journal", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("leaves the Journal off the web app", async () => {
    api.getCapabilities.mockResolvedValue({ ...api.CORE_ONLY, remote: true });
    render(<App />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Journal" })).toBeNull());
    expect(tabs()).toEqual(["Chat", "Imagine", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("To mission puts the prompt in the mission's chat and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Journal" }));
    await userEvent.click(await screen.findByRole("button", { name: "To mission" }));
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
    expect(await screen.findByText("Settings for a desktop at localAi")).toBeVisible();
  });

  it("the Command button opens it too, and Ask Loadout fills the chat box", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    await userEvent.keyboard("tidy the board");
    await userEvent.click(screen.getByRole("option", { name: "Ask Loadout: tidy the board" }));
    expect(screen.getByText("Chat page: tidy the board in this chat")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  it("the chat's sign-in opens Settings at Account", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText("Settings for a desktop at account")).toBeVisible();
  });

  it("the Activity Dock shows an approval waiting from any page and opens the chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    chatHandlers.length = 0;
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(screen.queryByRole("complementary", { name: "Activity" })).toBeNull();
    await waitFor(() => expect(chatHandlers.length).toBe(1));
    act(() => {
      chatHandlers[0]({
        conversationId: "shell-1",
        event: { kind: "permission", id: "p1", title: "Run npm test", action: "run", reason: "" },
      });
    });
    const dock = screen.getByRole("complementary", { name: "Activity" });
    await userEvent.click(within(dock).getByRole("button", { name: /Activity/ }));
    expect(within(dock).getByRole("listitem", { name: "Chat reply" })).toHaveTextContent(
      "Needs your OK: Run npm test",
    );
    await userEvent.click(within(dock).getByRole("button", { name: "Review" }));
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });
});
