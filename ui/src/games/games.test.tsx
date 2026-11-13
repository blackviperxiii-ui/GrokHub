import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  companionScreen,
  DESTINY_APP_ID,
  ELITE_APP_ID,
  ESO_APP_ID,
  FACTORIO_APP_ID,
  GW2_APP_ID,
  OSRS_APP_ID,
  POE_APP_ID,
  ROCKET_LEAGUE_APP_ID,
  STARDEW_APP_ID,
} from "./index";
import { Destiny } from "./Destiny";
import { Elite } from "./Elite";
import { Eso } from "./Eso";
import { Factorio } from "./Factorio";
import { Gw2 } from "./Gw2";
import { Minecraft } from "./Minecraft";
import { Osrs } from "./Osrs";
import { Poe } from "./Poe";
import { RocketLeague } from "./RocketLeague";
import { Stardew } from "./Stardew";
import { Wow } from "./Wow";

const api = vi.hoisted(() => ({
  gameWow: vi.fn(),
  gameWowAddon: vi.fn(),
  gameGw2: vi.fn(),
  gameGw2Refresh: vi.fn(),
  gameGw2StartKey: vi.fn(),
  gameGw2AnswerKey: vi.fn(),
  gameGw2RemoveKey: vi.fn(),
  gameEso: vi.fn(),
  gameEsoAddon: vi.fn(),
  gameOsrs: vi.fn(),
  gameOsrsSave: vi.fn(),
  gameOsrsForget: vi.fn(),
  gameOsrsRefresh: vi.fn(),
  gameDestiny: vi.fn(),
  gameDestinyRefresh: vi.fn(),
  gameDestinyStartKey: vi.fn(),
  gameDestinyAnswerKey: vi.fn(),
  gameDestinySaveName: vi.fn(),
  gameDestinyRemove: vi.fn(),
  gamePoe: vi.fn(),
  gameElite: vi.fn(),
  gameFactorio: vi.fn(),
  gameFactorioMod: vi.fn(),
  gameStardew: vi.fn(),
  gameMinecraft: vi.fn(),
  gameRl: vi.fn(),
}));

vi.mock("../api", () => api);

describe("companionScreen", () => {
  it("picks screens by Battle.net product and Steam app id", () => {
    expect(
      companionScreen({
        kind: "battlenet",
        game: {
          product: "wow",
          name: "World of Warcraft",
          install: "/home/user/Games/wow",
          seed: 1,
        },
      }),
    ).not.toBeNull();
    expect(
      companionScreen({
        kind: "steam",
        game: {
          appId: GW2_APP_ID,
          name: "Guild Wars 2",
          installed: true,
          lastPlayed: 0,
          playtimeMin: 0,
        },
      }),
    ).not.toBeNull();
    expect(
      companionScreen({
        kind: "steam",
        game: {
          appId: ESO_APP_ID,
          name: "ESO",
          installed: true,
          lastPlayed: 0,
          playtimeMin: 0,
        },
      }),
    ).not.toBeNull();
    expect(
      companionScreen({
        kind: "steam",
        game: {
          appId: OSRS_APP_ID,
          name: "OSRS",
          installed: true,
          lastPlayed: 0,
          playtimeMin: 0,
        },
      }),
    ).not.toBeNull();
    expect(
      companionScreen({
        kind: "battlenet",
        game: { product: "fenris", name: "Diablo IV", install: "/d4", seed: 2 },
      }),
    ).toBeNull();
    expect(
      companionScreen({
        kind: "steam",
        game: {
          appId: 620,
          name: "Portal 2",
          installed: true,
          lastPlayed: 0,
          playtimeMin: 0,
        },
      }),
    ).toBeNull();
    for (const appId of [
      DESTINY_APP_ID,
      POE_APP_ID,
      ELITE_APP_ID,
      FACTORIO_APP_ID,
      STARDEW_APP_ID,
      ROCKET_LEAGUE_APP_ID,
    ]) {
      expect(
        companionScreen({
          kind: "steam",
          game: {
            appId,
            name: "Game",
            installed: true,
            lastPlayed: 0,
            playtimeMin: 0,
          },
        }),
      ).not.toBeNull();
    }
    expect(
      companionScreen({
        kind: "steam",
        game: {
          appId: 1,
          name: "Minecraft: Java Edition",
          installed: true,
          lastPlayed: 0,
          playtimeMin: 0,
        },
      }),
    ).not.toBeNull();
  });
});

describe("Wow", () => {
  beforeEach(() => {
    api.gameWow.mockReset();
    api.gameWowAddon.mockReset();
  });

  it("renders fixture flavors and empty saved", async () => {
    api.gameWow.mockResolvedValue({
      title: "WORLD OF WARCRAFT",
      running: false,
      caption: "Retail · Battle.net",
      logging: null,
      watcherNote: null,
      flavors: [
        {
          install: "/home/user/Games/wow",
          flavor: "_retail_",
          label: "Retail",
          addonLine: "Loadout addon not installed",
          action: "Install addon",
          characters: [],
          events: [],
          emptySaved:
            "Log in, then /reload or log out: WoW saves addon data then, and it shows here.",
        },
      ],
      installAll: null,
      note: "",
    });
    render(<Wow />);
    expect(await screen.findByText("WORLD OF WARCRAFT")).toBeVisible();
    expect(screen.getByText(/Retail · Loadout addon not installed/)).toBeVisible();
    expect(
      screen.getByText(/Log in, then \/reload or log out/),
    ).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Install addon" }));
    expect(api.gameWowAddon).toHaveBeenCalledWith(
      "/home/user/Games/wow",
      "_retail_",
      "install",
    );
  });
});

describe("Gw2", () => {
  beforeEach(() => {
    for (const k of [
      "gameGw2",
      "gameGw2Refresh",
      "gameGw2StartKey",
      "gameGw2AnswerKey",
      "gameGw2RemoveKey",
    ] as const) {
      api[k].mockReset();
    }
  });

  it("empty state asks for a key; Save posts the credentials card", async () => {
    api.gameGw2.mockResolvedValue({
      status:
        "Guild Wars 2: add a read-only API key to read your account through ArenaNet's official API.",
      hasKey: false,
      accountLine: null,
      note: "",
      keyHint: "Make a key at account.arena.net under Applications.",
    });
    api.gameGw2StartKey.mockResolvedValue({
      id: "gw2-key-1",
      action: "Save the Guild Wars 2 API key",
    });
    api.gameGw2AnswerKey.mockResolvedValue({
      status: "Guild Wars 2: key saved, nothing read yet.",
      hasKey: true,
      accountLine: null,
      note: "Key saved. Reading your account...",
      keyHint: "Make a key at account.arena.net under Applications.",
    });
    render(<Gw2 />);
    expect(
      await screen.findByText(/add a read-only API key/),
    ).toBeVisible();
    const key =
      "ABCDEF01-2345-6789-ABCD-EF0123456789ABCDEF01-2345-6789-ABCD-EF0123456789";
    await userEvent.type(screen.getByLabelText("Guild Wars 2 API key"), key);
    await userEvent.click(screen.getByRole("button", { name: "Save key" }));
    expect(api.gameGw2StartKey).toHaveBeenCalledWith(key);
    expect(await screen.findByText(/Save the Guild Wars 2 API key\?/)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.gameGw2AnswerKey).toHaveBeenCalledWith("gw2-key-1", true);
  });
});

describe("Eso", () => {
  beforeEach(() => {
    api.gameEso.mockReset();
    api.gameEsoAddon.mockReset();
  });

  it("empty and installed states", async () => {
    api.gameEso.mockResolvedValue({
      servers: [],
      character: null,
      note: "",
      empty: "No Elder Scrolls Online add-on folders found on this computer.",
    });
    const { rerender } = render(<Eso />);
    expect(await screen.findByText(/No Elder Scrolls Online/)).toBeVisible();

    api.gameEso.mockResolvedValue({
      servers: [
        {
          dir: "/home/user/Documents/Elder Scrolls Online/live",
          label: "",
          status: "ESO addon: not installed.",
          action: "Install ESO addon",
        },
      ],
      character: null,
      note: "",
      empty: null,
    });
    api.gameEsoAddon.mockResolvedValue({
      servers: [
        {
          dir: "/home/user/Documents/Elder Scrolls Online/live",
          label: "",
          status: "ESO addon: installed (1.0.0).",
          action: "Remove ESO addon",
        },
      ],
      character: null,
      note: "Installed. Turn on Loadout in ESO's Add-Ons menu, then it saves when you log out or reload the UI.",
      empty: null,
    });
    rerender(<Eso />);
    // Remount to reload
    render(<Eso />);
    await userEvent.click(await screen.findByRole("button", { name: "Install ESO addon" }));
    expect(api.gameEsoAddon).toHaveBeenCalledWith(
      "/home/user/Documents/Elder Scrolls Online/live",
      true,
    );
  });
});

describe("Osrs", () => {
  beforeEach(() => {
    api.gameOsrs.mockReset();
    api.gameOsrsSave.mockReset();
    api.gameOsrsForget.mockReset();
    api.gameOsrsRefresh.mockReset();
  });

  it("empty state and save name", async () => {
    api.gameOsrs.mockResolvedValue({
      status: "Old School RuneScape: add your display name to read Jagex's official hiscores.",
      gains: null,
      hasPlayer: false,
      name: null,
      kind: "Normal",
      kinds: ["Normal", "Ironman", "Hardcore ironman", "Ultimate ironman"],
      note: "",
      nameHint: "Your display name on Jagex's public hiscores. No password or key.",
    });
    api.gameOsrsSave.mockResolvedValue({
      status: "Old School RuneScape (Zezima, ironman): nothing read yet.",
      gains: null,
      hasPlayer: true,
      name: "Zezima",
      kind: "Ironman",
      kinds: ["Normal", "Ironman", "Hardcore ironman", "Ultimate ironman"],
      note: "Read from the hiscores at most once an hour.",
      nameHint: "Your display name on Jagex's public hiscores. No password or key.",
    });
    render(<Osrs />);
    expect(await screen.findByText(/add your display name/)).toBeVisible();
    await userEvent.type(screen.getByLabelText("Old School RuneScape name"), "Zezima");
    await userEvent.selectOptions(screen.getByLabelText("Account type"), "Ironman");
    await userEvent.click(screen.getByRole("button", { name: "Save name" }));
    expect(api.gameOsrsSave).toHaveBeenCalledWith("Zezima", "Ironman");
  });
});

describe("Destiny", () => {
  beforeEach(() => {
    for (const k of [
      "gameDestiny",
      "gameDestinyRefresh",
      "gameDestinyStartKey",
      "gameDestinyAnswerKey",
      "gameDestinySaveName",
      "gameDestinyRemove",
    ] as const) {
      api[k].mockReset();
    }
  });

  it("empty state asks for key and name", async () => {
    api.gameDestiny.mockResolvedValue({
      status:
        "Destiny 2: add your own Bungie.net API key and Bungie Name to read your public profile.",
      hasKey: false,
      hasName: false,
      bungieName: null,
      profileLine: null,
      note: "",
      howTo: "Register an application at bungie.net.",
      keyHint: "From your Bungie.net application page.",
      nameHint: "Your name with its code, like Guardian#1234.",
    });
    render(<Destiny />);
    expect(await screen.findByText(/add your own Bungie.net API key/)).toBeVisible();
    expect(screen.getByLabelText("Bungie.net API key")).toBeVisible();
    expect(screen.getByLabelText("Bungie Name")).toBeVisible();
  });
});

describe("session screens", () => {
  beforeEach(() => {
    api.gamePoe.mockReset();
    api.gameElite.mockReset();
    api.gameStardew.mockReset();
    api.gameMinecraft.mockReset();
    api.gameRl.mockReset();
    api.gameFactorio.mockReset();
  });

  it("renders empty fixtures", async () => {
    api.gamePoe.mockResolvedValue({
      title: "Path of Exile",
      appId: POE_APP_ID,
      status: "",
      empty: "Play with the game watcher on.",
      note: "",
    });
    api.gameElite.mockResolvedValue({
      title: "Elite Dangerous",
      status: "",
      empty: "Play with the game watcher on.",
      note: "",
    });
    api.gameStardew.mockResolvedValue({
      title: "Stardew Valley",
      status: "",
      empty: "No farm save read yet.",
      note: "",
    });
    api.gameMinecraft.mockResolvedValue({
      title: "Minecraft: Java Edition",
      status: "",
      empty: "Play with the game watcher on.",
      note: "",
    });
    api.gameRl.mockResolvedValue({
      title: "Rocket League",
      status: "Rocket League's Stats API is off.",
      howTo: true,
      iniLine: "PacketSendRate=30",
      note: "",
    });
    api.gameFactorio.mockResolvedValue({
      status: "",
      warning: null,
      action: null,
      note: "",
      empty: "Factorio hasn't written its user folder yet.",
      userDir: null,
    });

    render(<Poe appId={POE_APP_ID} />);
    expect(await screen.findByText("Path of Exile")).toBeVisible();
    render(<Elite />);
    expect(await screen.findByText("Elite Dangerous")).toBeVisible();
    render(<Stardew />);
    expect(await screen.findByText(/No farm save read yet/)).toBeVisible();
    render(<Minecraft />);
    expect(await screen.findByText("Minecraft: Java Edition")).toBeVisible();
    render(<RocketLeague />);
    expect(await screen.findByRole("button", { name: "Copy the setting" })).toBeVisible();
    render(<Factorio />);
    expect(await screen.findByText(/hasn't written its user folder/)).toBeVisible();
  });
});
