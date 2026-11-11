import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  BattlenetGame,
  BattlenetPage,
  CompanionFrame,
  CoverShape,
  LibraryGame,
  LibraryPage,
  MissionPage,
} from "./api";

const api = vi.hoisted(() => ({
  getLibrary: vi.fn<() => Promise<LibraryPage>>(),
  getBattlenet: vi.fn<() => Promise<BattlenetPage>>(),
  getMission: vi.fn<() => Promise<MissionPage>>(),
  libraryCover: vi.fn<(appId: number, shape: CoverShape) => Promise<string | null>>(),
  libraryPlay: vi.fn<(appId: number) => Promise<string>>(),
  battlenetPlay: vi.fn<(product: string) => Promise<string>>(),
  battlenetOpenFolder: vi.fn<(product: string) => Promise<string>>(),
  companionFrame: vi.fn<
    (noteId: number, lastPlayed: number, steamAppId?: number | null) => Promise<CompanionFrame>
  >(),
  companionSetNote: vi.fn<(appId: number, text: string) => Promise<string>>(),
}));

vi.mock("./api", () => api);

import {
  filterGames,
  filterTiles,
  Library,
  NO_MATCH,
  NO_OBJECTIVE,
  NO_STEAM,
  namesGame,
  objectiveText,
  playtimeLabel,
} from "./Library";

function game(over: Partial<LibraryGame> = {}): LibraryGame {
  return {
    appId: 620,
    name: "Portal 2",
    installed: true,
    lastPlayed: 1_700_000_000,
    playtimeMin: 754,
    ...over,
  };
}

function bnet(over: Partial<BattlenetGame> = {}): BattlenetGame {
  return {
    product: "wow",
    name: "World of Warcraft",
    install: "/home/user/Games/wow",
    seed: 42,
    ...over,
  };
}

const GAMES: LibraryGame[] = [
  game(),
  game({ appId: 1145360, name: "Hades", installed: false, lastPlayed: 1_600_000_000, playtimeMin: 90 }),
  game({ appId: 413150, name: "Stardew Valley", lastPlayed: 0, playtimeMin: 0 }),
];

const BNET: BattlenetGame[] = [bnet(), bnet({ product: "fenris", name: "Diablo IV", seed: 7 })];

const EMPTY_XP = {
  level: 1,
  title: "Rookie",
  xp: 0,
  nextLevelXp: 100,
  frac: 0,
  today: 0,
  week: { days: [false, false, false, false, false, false, false], today: 0, active: 0, target: 3, inARow: 0, played: 0, freezes: 0 },
};

function gridNames() {
  const grid = within(screen.getByRole("list", { name: "Games" }));
  return grid.getAllByRole("button").map((b) => b.getAttribute("aria-label"));
}

describe("filterGames / filterTiles", () => {
  it("keeps installed games, never-played games or name matches", () => {
    const names = (gs: LibraryGame[]) => gs.map((g) => g.name);
    expect(names(filterGames(GAMES, "all", ""))).toEqual(["Portal 2", "Hades", "Stardew Valley"]);
    expect(names(filterGames(GAMES, "installed", ""))).toEqual(["Portal 2", "Stardew Valley"]);
    expect(names(filterGames(GAMES, "backlog", ""))).toEqual(["Stardew Valley"]);
    expect(names(filterGames(GAMES, "all", "  hAD "))).toEqual(["Hades"]);
    expect(names(filterGames(GAMES, "backlog", "portal"))).toEqual([]);
  });

  it("On objective keeps Steam and Battle.net games named in the mission", () => {
    const objective = "Finish Portal 2\nLevel a World of Warcraft alt";
    const tiles = filterTiles(GAMES, BNET, "onObjective", "", objective);
    expect(tiles.map((t) => t.game.name)).toEqual(["Portal 2", "World of Warcraft"]);
    expect(filterTiles(GAMES, BNET, "backlog", "", "").map((t) => t.game.name)).toEqual([
      "Stardew Valley",
    ]);
    expect(namesGame(objective, "Portal 2")).toBe(true);
    expect(namesGame(objective, "Diablo IV")).toBe(false);
    expect(objectiveText(null)).toBe("");
  });

  it("says how long a game was played", () => {
    expect(playtimeLabel(0)).toBe("Never played");
    expect(playtimeLabel(45)).toBe("45 min played");
    expect(playtimeLabel(754)).toBe("12.6 h played");
  });
});

describe("Library", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.libraryCover.mockResolvedValue(null);
    api.getBattlenet.mockResolvedValue({ found: false, games: [] });
    api.getMission.mockResolvedValue({ mission: null, xp: EMPTY_XP });
    api.companionFrame.mockResolvedValue({ note: "", news: [], guidesUrl: null });
  });

  it("filters the grid by tab and search", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES });
    render(<Library />);
    await screen.findByRole("list", { name: "Games" });
    expect(gridNames()).toEqual(["Portal 2", "Hades", "Stardew Valley"]);

    await userEvent.click(screen.getByRole("button", { name: "Installed" }));
    expect(gridNames()).toEqual(["Portal 2", "Stardew Valley"]);

    await userEvent.click(screen.getByRole("button", { name: "Backlog" }));
    expect(gridNames()).toEqual(["Stardew Valley"]);
    expect(within(screen.getByRole("complementary", { name: "Game details" })).getByText("Never played")).toBeVisible();

    await userEvent.click(screen.getByRole("button", { name: "All" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Search games" }), "hades");
    expect(gridNames()).toEqual(["Hades"]);
    expect(
      within(screen.getByRole("complementary", { name: "Game details" })).getByText(
        "Steam is still downloading or updating it.",
      ),
    ).toBeVisible();

    await userEvent.clear(screen.getByRole("searchbox", { name: "Search games" }));
    await userEvent.type(screen.getByRole("searchbox", { name: "Search games" }), "zelda");
    expect(screen.getByText(NO_MATCH)).toBeVisible();
    expect(screen.queryByRole("list", { name: "Games" })).toBeNull();
  });

  it("shows Battle.net tiles beside Steam and opens the companion frame", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES.slice(0, 1) });
    api.getBattlenet.mockResolvedValue({ found: true, games: BNET });
    render(<Library />);
    await screen.findByRole("button", { name: "World of Warcraft" });
    expect(gridNames()).toEqual(["Portal 2", "World of Warcraft", "Diablo IV"]);
    await userEvent.click(screen.getByRole("button", { name: "World of Warcraft" }));
    const detail = within(screen.getByRole("complementary", { name: "Game details" }));
    expect(detail.getByRole("heading", { name: "World of Warcraft" })).toBeVisible();
    expect(detail.getByText("Battle.net")).toBeVisible();
    expect(detail.getByRole("region", { name: "Game companion" })).toBeVisible();
    expect(api.companionFrame).toHaveBeenCalledWith(42, 0, null);
  });

  it("On objective filters by the open mission", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES });
    api.getBattlenet.mockResolvedValue({ found: true, games: BNET });
    api.getMission.mockResolvedValue({
      mission: {
        conversationId: "c1",
        title: "Finish Portal 2",
        status: "active",
        steps: [{ id: "1", text: "beat the last chamber", status: "planned" }],
        done: 0,
        total: 1,
        canAccept: false,
        accepted: false,
      },
      xp: EMPTY_XP,
    });
    render(<Library />);
    await screen.findByRole("list", { name: "Games" });
    await userEvent.click(screen.getByRole("button", { name: "On objective" }));
    expect(gridNames()).toEqual(["Portal 2"]);
  });

  it("On objective empty state when no mission names a game", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES });
    render(<Library />);
    await screen.findByRole("list", { name: "Games" });
    await userEvent.click(screen.getByRole("button", { name: "On objective" }));
    expect(screen.getByText(NO_OBJECTIVE)).toBeVisible();
  });

  it("Play asks Steam to start the picked game", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES });
    api.libraryPlay.mockResolvedValue("Asked Steam to start Hades.");
    render(<Library />);
    await userEvent.click(await screen.findByRole("button", { name: "Hades" }));
    const detail = within(screen.getByRole("complementary", { name: "Game details" }));
    expect(detail.getByRole("heading", { name: "Hades" })).toBeVisible();
    expect(detail.getByText("1.5 h played")).toBeVisible();
    await userEvent.click(detail.getByRole("button", { name: "Play" }));
    expect(api.libraryPlay).toHaveBeenCalledWith(1145360);
    expect(api.libraryPlay).toHaveBeenCalledTimes(1);
    expect(await screen.findByRole("status")).toHaveTextContent("Asked Steam to start Hades.");
  });

  it("Play asks Battle.net to start the picked game", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: [] });
    api.getBattlenet.mockResolvedValue({ found: true, games: BNET });
    api.battlenetPlay.mockResolvedValue("Asked Battle.net to start World of Warcraft.");
    render(<Library />);
    await userEvent.click(await screen.findByRole("button", { name: "World of Warcraft" }));
    const detail = within(screen.getByRole("complementary", { name: "Game details" }));
    await userEvent.click(detail.getByRole("button", { name: "Play" }));
    expect(api.battlenetPlay).toHaveBeenCalledWith("wow");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Asked Battle.net to start World of Warcraft.",
    );
  });

  it("shows covers it gets and paints the rest", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: true, games: GAMES.slice(0, 2) });
    api.libraryCover.mockImplementation((appId, shape) =>
      Promise.resolve(appId === 620 && shape === "tall" ? "data:image/jpeg;base64,/9j/" : null),
    );
    const { container } = render(<Library />);
    await waitFor(() => expect(container.querySelector("img.library-art.tall")).not.toBeNull());
    expect(container.querySelector("img.library-art.tall")).toHaveAttribute("src", "data:image/jpeg;base64,/9j/");
    const hades = screen.getByRole("button", { name: "Hades" });
    expect(hades.querySelector(".placeholder")).toHaveTextContent("H");
    expect(api.libraryCover).toHaveBeenCalledWith(620, "tall");
    expect(api.libraryCover).toHaveBeenCalledWith(1145360, "tall");
    expect(api.libraryCover).toHaveBeenCalledWith(620, "wide");
  });

  it("says when Steam isn't on this computer", async () => {
    api.getLibrary.mockResolvedValue({ steamFound: false, games: [] });
    render(<Library />);
    expect(await screen.findByText(NO_STEAM)).toBeVisible();
    expect(api.libraryCover).not.toHaveBeenCalled();
  });
});
