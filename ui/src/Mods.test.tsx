import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ModGame, ModGamesView, ModList, ModPage, ModRow, ModSuggestion } from "./api";

const api = vi.hoisted(() => ({
  modsGames: vi.fn<() => Promise<ModGamesView>>(),
  modsBrowse: vi.fn<(game: string, source: string, list: ModList, page: number) => Promise<ModPage>>(),
  modsSearch: vi.fn<(game: string, source: string, text: string, page: number) => Promise<ModPage>>(),
  modsNewFinds: vi.fn<(game: string, source: string, page: number) => Promise<ModPage>>(),
  modsForYou: vi.fn<() => Promise<ModSuggestion[]>>(),
  modsNotForMe: vi.fn<(source: string, id: string) => Promise<void>>(),
  modsHideGame: vi.fn<(game: string, hidden: boolean) => Promise<void>>(),
  openUrl: vi.fn<(url: string) => Promise<void>>(),
}));

vi.mock("./api", () => api);

const install = vi.hoisted(() => ({
  modsFiles: vi.fn(),
  modsInstalled: vi.fn(),
  modsPlanInstall: vi.fn(),
  modsPlanUninstall: vi.fn(),
  modsPickupDownload: vi.fn(),
  modsWaitForBrowser: vi.fn(),
  modsConfirm: vi.fn(),
  modsCancel: vi.fn(),
  onModsJob: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("./modsInstallApi", () => install);

import { Mods, NO_GAMES, NOTHING, shortCount, SUBTITLE } from "./Mods";

function game(over: Partial<ModGame> = {}): ModGame {
  return {
    id: "stardew",
    name: "Stardew Valley",
    modeNote: "Singleplayer and your own co-op",
    sources: [{ id: "nexus", label: "Nexus Mods" }],
    lastPlayed: 1_800_000_000,
    playtimeMin: 2400,
    newCount: 0,
    suggestionsHidden: false,
    ...over,
  };
}

function mod(over: Partial<ModRow> = {}): ModRow {
  return {
    id: "1",
    name: "Sodium",
    author: "jellysquid3",
    summary: "The fastest rendering optimization mod.",
    downloads: 90_000_000,
    likes: 30_000,
    updated: 1_790_000_000,
    pageUrl: "https://modrinth.com/mod/sodium",
    iconUrl: null,
    categories: ["optimization"],
    ...over,
  };
}

const page = (mods: ModRow[], over: Partial<ModPage> = {}): ModPage => ({ mods, page: 0, more: false, note: null, cached: false, ...over });

const RAIL: ModGamesView = {
  games: [
    game({
      id: "minecraft",
      name: "Minecraft: Java Edition",
      sources: [
        { id: "modrinth", label: "Modrinth" },
        { id: "curseforge", label: "CurseForge" },
      ],
      newCount: 3,
    }),
    game(),
  ],
  cant: [{ key: "apex", name: "Apex Legends", reason: "Easy Anti-Cheat. Mods there can get accounts banned." }],
  status: "Watching 2 games for new mods.",
  offline: false,
};

beforeEach(() => {
  vi.clearAllMocks();
  api.modsGames.mockResolvedValue(RAIL);
  api.modsBrowse.mockResolvedValue(page([mod(), mod({ id: "2", name: "Fabric API" })]));
  api.modsSearch.mockResolvedValue(page([mod({ id: "9", name: "Iris Shaders" })]));
  api.modsNewFinds.mockResolvedValue(page([]));
  api.modsForYou.mockResolvedValue([]);
  api.modsNotForMe.mockResolvedValue();
  api.modsHideGame.mockResolvedValue();
  api.openUrl.mockResolvedValue();
});

describe("shortCount", () => {
  it("shortens big counts", () => {
    expect(shortCount(512)).toBe("512");
    expect(shortCount(34_200)).toBe("34.2K");
    expect(shortCount(90_000_000)).toBe("90M");
  });
});

describe("Mods", () => {
  it("shows the rail most recently played first, with Can't mod collapsed", async () => {
    const user = userEvent.setup();
    render(<Mods />);
    const rail = within(await screen.findByRole("navigation", { name: "Moddable games" }));
    const games = rail.getAllByRole("listitem").map((li) => li.textContent);
    expect(games.slice(0, 2)).toEqual(["Minecraft: Java Edition3 new", "Stardew Valley"]);
    expect(screen.getByText(`${SUBTITLE} Watching 2 games for new mods.`)).toBeInTheDocument();
    expect(screen.queryByText("Apex Legends")).not.toBeInTheDocument();
    await user.click(rail.getByRole("button", { name: "Can't mod (1)" }));
    expect(screen.getByText("Apex Legends")).toBeInTheDocument();
    expect(screen.getByText(/Easy Anti-Cheat/)).toBeInTheDocument();
  });

  it("browses the first game's preferred source and opens a mod's files from View", async () => {
    const user = userEvent.setup();
    install.modsFiles.mockResolvedValue({
      files: [{ id: "f1", name: "Sodium 0.6.0", line: "0.6.0 · 1.2 MB", offer: { kind: "install" }, waiting: false }],
      blocked: null,
      needs: [],
      about: "Renders faster.",
      note: null,
    });
    render(<Mods />);
    const list = within(await screen.findByRole("list", { name: "Mods" }));
    expect(list.getByText("Sodium")).toBeInTheDocument();
    expect(api.modsBrowse).toHaveBeenCalledWith("minecraft", "modrinth", "popular", 0);
    expect(list.queryByRole("button", { name: "Install" })).not.toBeInTheDocument();
    expect(screen.getByRole("group", { name: "Source" })).toBeInTheDocument();
    await user.click(list.getAllByRole("button", { name: "View" })[0]);
    const drawer = within(await screen.findByRole("complementary", { name: "Sodium details" }));
    expect(await drawer.findByText("0.6.0 · 1.2 MB")).toBeInTheDocument();
    expect(install.modsFiles).toHaveBeenCalledWith("minecraft", "modrinth", "1", "The fastest rendering optimization mod.");
    expect(drawer.getByRole("button", { name: "Install" })).toBeEnabled();
  });

  it("lists what is installed on the Installed tab without asking a source", async () => {
    const user = userEvent.setup();
    install.modsInstalled.mockResolvedValue({
      ours: [{ key: "modrinth:AANobbMI", name: "Sodium", version: "0.6.0", offlineOnly: null }],
      others: [],
      recovered: null,
    });
    render(<Mods />);
    await screen.findByRole("list", { name: "Mods" });
    api.modsBrowse.mockClear();
    await user.click(screen.getByRole("button", { name: "Installed" }));
    const rows = within(await screen.findByRole("list", { name: "Installed mods" }));
    expect(rows.getByText("Sodium")).toBeInTheDocument();
    expect(install.modsInstalled).toHaveBeenCalledWith("minecraft");
    expect(api.modsBrowse).not.toHaveBeenCalled();
  });

  it("searches after typing pauses", async () => {
    const user = userEvent.setup();
    render(<Mods />);
    await screen.findByRole("list", { name: "Mods" });
    await user.type(screen.getByRole("searchbox", { name: "Search Minecraft: Java Edition mods" }), "iris");
    await waitFor(() => expect(api.modsSearch).toHaveBeenCalledWith("minecraft", "modrinth", "iris", 0));
    expect(api.modsSearch).toHaveBeenCalledTimes(1);
    expect(await screen.findByText("Iris Shaders")).toBeInTheDocument();
  });

  it("says when a list is empty and when New finds has nothing yet", async () => {
    const user = userEvent.setup();
    api.modsNewFinds.mockResolvedValue(page([], { note: "No new finds yet. Loadout checks for new mods every day and lists them here." }));
    render(<Mods />);
    await screen.findByRole("list", { name: "Mods" });
    await user.click(screen.getByRole("button", { name: "New finds" }));
    expect(await screen.findByText(/No new finds yet/)).toBeInTheDocument();
    expect(api.modsNewFinds).toHaveBeenCalledWith("minecraft", "modrinth", 0);
    api.modsBrowse.mockResolvedValue(page([]));
    await user.click(screen.getByRole("button", { name: "Updated" }));
    expect(await screen.findByText(NOTHING)).toBeInTheDocument();
  });

  it("shows For you, opens a card's game and drops one marked Not for me", async () => {
    const user = userEvent.setup();
    const s: ModSuggestion = { game: "stardew", gameName: "Stardew Valley", source: "nexus", tab: "trending", why: "Trending now", mod: mod({ id: "77", name: "SpaceCore" }) };
    api.modsForYou.mockResolvedValueOnce([s]).mockResolvedValue([]);
    render(<Mods />);
    const row = within(await screen.findByRole("region", { name: "For you" }));
    expect(row.getByText("SpaceCore")).toBeInTheDocument();
    expect(row.getByText("Trending now")).toBeInTheDocument();
    await user.click(row.getByRole("button", { name: "View" }));
    await waitFor(() => expect(api.modsBrowse).toHaveBeenCalledWith("stardew", "nexus", "trending", 0));
    await user.click(row.getByRole("button", { name: "Not for me" }));
    expect(api.modsNotForMe).toHaveBeenCalledWith("nexus", "77");
    await waitFor(() => expect(screen.queryByRole("region", { name: "For you" })).not.toBeInTheDocument());
  });

  it("says when no game here can be modded", async () => {
    api.modsGames.mockResolvedValue({ games: [], cant: [], status: "", offline: false });
    render(<Mods />);
    expect(await screen.findByText(NO_GAMES)).toBeInTheDocument();
    expect(api.modsBrowse).not.toHaveBeenCalled();
  });
});
