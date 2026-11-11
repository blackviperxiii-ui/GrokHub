import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, HomePage, HomeSlot, QuickChip, TileAction, TileView } from "./api";

// The real Home in the real App: where a tile, a chip or the chat may take you.
const api = vi.hoisted(() => {
  const CORE_ONLY = {
    desktopControl: false,
    pulse: false,
    localAi: false,
    tray: false,
    webAccess: false,
    plugins: false,
    library: false,
  };
  return {
    CORE_ONLY,
    getCapabilities: vi.fn<() => Promise<Capabilities>>(),
    getSettings: vi.fn(() => Promise.resolve({ theme: "light", desktopControl: false })),
    haltDesktop: vi.fn(() => Promise.resolve([])),
    slashHits: vi.fn(() => Promise.resolve([])),
    getWorkboard: vi.fn(() => Promise.resolve({ columns: [], archived: [], stamp: 1 })),
    onChatEvent: vi.fn(() => Promise.resolve(() => {})),
    onHalt: vi.fn(() => Promise.resolve(() => {})),
    onAutomationRan: vi.fn(() => Promise.resolve(() => {})),
    onUpdateAvailable: vi.fn(() => Promise.resolve(() => {})),
    onLocalEngineEvent: vi.fn(() => Promise.resolve(() => {})),
    workboardSweepUndo: vi.fn(() => Promise.resolve({})),
    listChats: vi.fn(() => Promise.resolve([])),
    shareSchedulerClock: vi.fn(() => Promise.resolve()),
    quickChips: vi.fn<(surface: string) => Promise<QuickChip[]>>(),
    chipOutcome: vi.fn(() => Promise.resolve(true)),
    getHome: vi.fn<() => Promise<HomePage>>(),
    pulseAct: vi.fn(() => Promise.resolve({})),
    homeCatalog: vi.fn(),
    setHomeLayout: vi.fn(),
    homeOpenClip: vi.fn(),
    homeTurnOn: vi.fn(),
    homeSetup: vi.fn(),
    homeSetupSteps: vi.fn(),
    libraryFriends: vi.fn(),
    librarySteamSettings: vi.fn(),
    homeRunPlaybook: vi.fn(),
    libraryPlay: vi.fn(),
    openUrl: vi.fn(),
  };
});

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({
  newConversationId: () => "shell-new",
  Chat: ({ onOpenView }: { onOpenView?: (view: string) => void }) => (
    <div>
      <p>Chat page</p>
      <button type="button" onClick={() => onOpenView?.("history")}>
        Recent chats from chat
      </button>
    </div>
  ),
}));
vi.mock("./Automations", () => ({ Automations: () => null, resultLine: () => "" }));
vi.mock("./Journal", () => ({ Journal: () => <p>Journal page</p> }));
vi.mock("./History", () => ({ History: () => <p>History page</p> }));
vi.mock("./Library", () => ({ Library: () => <p>Library page</p> }));
vi.mock("./UpdatesDrivers", () => ({ UpdatesDrivers: () => <p>Updates page</p> }));
vi.mock("./Workboard", () => ({ Workboard: () => null }));
vi.mock("./Memory", () => ({ Memory: () => null }));
vi.mock("./Agents", () => ({ Agents: () => null }));
vi.mock("./Connectors", () => ({ Connectors: () => null }));
vi.mock("./Skills", () => ({ Skills: () => null }));
vi.mock("./Objectives", () => ({ Objectives: () => null }));
vi.mock("./Pulse", () => ({ Pulse: () => null }));
vi.mock("./Imagine", () => ({ Imagine: () => null }));
vi.mock("./Settings", () => ({ Settings: () => null }));

import { App } from "./App";

const WEB: Capabilities = { ...api.CORE_ONLY, remote: true };
const DESKTOP: Capabilities = { ...api.CORE_ONLY, desktopControl: true };

/** A tile whose one button opens `page`. */
function navTile(module: string, name: string, label: string, page: string): HomeSlot {
  const action: TileAction = { kind: "nav", page };
  const view: TileView = {
    state: "ready",
    tag: null,
    text: { big: "", caption: "", detail: null, tone: "normal" },
    rows: [],
    progress: null,
    buttons: [{ label, action, primary: false }],
    tap: null,
    updatedMs: 1,
  };
  return { slot: module, size: "medium", module, name, purpose: "", refreshSecs: null, tile: { kind: "view", view } };
}

const PAGE: HomePage = {
  preset: "performance",
  xpBar: false,
  xp: {
    level: 1,
    title: "Rookie",
    xp: 0,
    nextLevelXp: 100,
    frac: 0,
    today: 0,
    week: { days: [false, false, false, false, false, false, false], today: 0, active: 0, target: 3, inARow: 0, played: 0, freezes: 0 },
  },
  glance: { memories: 0, proposed: 0, connectors: 0, connected: 0, failing: 0 },
  slots: [
    navTile("game_mode", "Game Mode", "Library", "library"),
    navTile("journal", "Journal", "Open Journal", "journal"),
    navTile("updates", "Updates", "Open Updates", "updates"),
  ],
};

const tab = (name: string) => within(screen.getByRole("navigation", { name: "Views" })).getByRole("button", { name });

describe("where Home and the chat can take you", () => {
  beforeEach(() => {
    api.getCapabilities.mockReset();
    api.getHome.mockReset().mockResolvedValue(PAGE);
    api.quickChips.mockReset().mockImplementation((surface) =>
      Promise.resolve(
        surface === "home"
          ? [{ id: "empty-history", label: "Recent chats", value: "__nav:history", action: "go", hint: "History", primary: false, view: "history" }]
          : [],
      ),
    );
  });

  it("on the web app a tile or chip for a page it doesn't have leaves Home showing", async () => {
    api.getCapabilities.mockResolvedValue(WEB);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Home" }));
    const stillHome = () => {
      expect(tab("Home")).toHaveAttribute("aria-pressed", "true");
      expect(screen.getByRole("listitem", { name: "Game Mode" })).toBeVisible();
    };

    await userEvent.click(within(await screen.findByRole("listitem", { name: "Game Mode" })).getByRole("button", { name: "Library" }));
    stillHome();
    await userEvent.click(within(screen.getByRole("listitem", { name: "Journal" })).getByRole("button", { name: "Open Journal" }));
    stillHome();
    await userEvent.click(within(screen.getByRole("listitem", { name: "Updates" })).getByRole("button", { name: "Open Updates" }));
    stillHome();
    await userEvent.click(await screen.findByRole("button", { name: "Recent chats" }));
    stillHome();
    expect(screen.queryByText("Library page")).toBeNull();
    expect(screen.queryByText("Journal page")).toBeNull();
    expect(screen.queryByText("History page")).toBeNull();
    expect(screen.queryByText("Updates page")).toBeNull();
  });

  it("on the web app the chat can't open History either", async () => {
    api.getCapabilities.mockResolvedValue(WEB);
    render(<App />);
    await screen.findByRole("button", { name: "Home" });
    await userEvent.click(screen.getByRole("button", { name: "Recent chats from chat" }));
    expect(tab("Chat")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText("Chat page")).toBeVisible();
    expect(screen.queryByText("History page")).toBeNull();
  });

  it("on the desktop the same taps open their pages, Updates & Drivers included", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Recent chats from chat" }));
    expect(await screen.findByText("History page")).toBeVisible();
    expect(tab("History")).toHaveAttribute("aria-pressed", "true");

    await userEvent.click(tab("Home"));
    await userEvent.click(within(await screen.findByRole("listitem", { name: "Journal" })).getByRole("button", { name: "Open Journal" }));
    expect(await screen.findByText("Journal page")).toBeVisible();

    await userEvent.click(tab("Home"));
    await userEvent.click(within(screen.getByRole("listitem", { name: "Updates" })).getByRole("button", { name: "Open Updates" }));
    expect(await screen.findByText("Updates page")).toBeVisible();
    expect(screen.queryByRole("listitem", { name: "Updates" })).toBeNull();

    // This desktop has no Library page, so its tile stays on Home.
    await userEvent.click(tab("Home"));
    await userEvent.click(within(screen.getByRole("listitem", { name: "Game Mode" })).getByRole("button", { name: "Library" }));
    expect(tab("Home")).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("listitem", { name: "Game Mode" })).toBeVisible();
    expect(screen.queryByText("Library page")).toBeNull();
  });
});
