import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePage, HomeSlot, PulseAct, PulsePage, QuickChip, TileView, Xp } from "./api";

const api = vi.hoisted(() => ({
  getHome: vi.fn<(conversationId?: string) => Promise<HomePage>>(),
  pulseAct: vi.fn<(id: string, act: PulseAct) => Promise<PulsePage>>(),
  quickChips: vi.fn<(surface: string) => Promise<QuickChip[]>>(),
  chipOutcome: vi.fn<(surface: string, id: string, picked: boolean) => Promise<boolean>>(),
  libraryPlay: vi.fn<(appId: number) => Promise<string>>(),
  homeOpenClip: vi.fn<(index: number) => Promise<void>>(),
  libraryFriends: vi.fn<() => Promise<void>>(),
  librarySteamSettings: vi.fn<() => Promise<void>>(),
  homeRunPlaybook: vi.fn<(id: string) => Promise<{ prompt: string }>>(),
  homeTurnOn: vi.fn<(feature: string) => Promise<void>>(),
  homeSetup: vi.fn<(module: string, undo: boolean) => Promise<string>>(),
  homeSetupSteps: vi.fn<(module: string) => Promise<string>>(),
  homeCatalog: vi.fn(),
  setHomeLayout: vi.fn(),
  openUrl: vi.fn<(url: string) => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { Home, NO_STEPS, grouped, pollSecs, staleLabel } from "./Home";

const XP: Xp = {
  level: 3,
  title: "Rookie",
  xp: 1240,
  nextLevelXp: 1500,
  frac: 0.48,
  today: 110,
  week: {
    days: [false, true, false, false, true, false, true],
    today: 6,
    active: 3,
    target: 4,
    inARow: 5,
    played: 7,
    freezes: 2,
  },
};

function view(over: Partial<TileView>): TileView {
  return {
    state: "ready",
    tag: null,
    text: { big: "", caption: "", detail: null, tone: "normal" },
    rows: [],
    progress: null,
    buttons: [],
    tap: null,
    updatedMs: 1,
    ...over,
  };
}

const PC: TileView = view({
  text: { big: "RTX 4070", caption: "12 GB VRAM · 32 GB RAM", detail: null, tone: "normal" },
  rows: ["GPU driver", "Memory", "Free for models", "Fan", "Extra"].map((label) => ({
    label,
    value: `${label} value`,
    tone: label === "Free for models" ? "warn" : "normal",
    tap: null,
  })),
  buttons: [{ label: "Tune-up", action: { kind: "ask", ask: "Check this PC and suggest a tune-up." }, primary: true }],
});

const SQUAD: TileView = view({
  state: "empty",
  text: { big: "—", caption: "Friends online show here once Steam friends are linked", detail: null, tone: "quiet" },
  buttons: [{ label: "Find a duo", action: { kind: "ask", ask: "Find a duo tonight" }, primary: true }],
  tap: { kind: "ask", ask: "Find a duo tonight" },
});

function slot(over: Partial<HomeSlot> & Pick<HomeSlot, "slot" | "module" | "name" | "tile">): HomeSlot {
  return { size: "small", purpose: "", refreshSecs: null, ...over };
}

/** The Mission Control layout with every tile the shell fills, and two it doesn't. */
const PAGE: HomePage = {
  preset: "mission_control",
  xpBar: true,
  xp: XP,
  glance: { memories: 12, proposed: 2, connectors: 3, connected: 2, failing: 1 },
  slots: [
    slot({
      slot: "large",
      size: "large",
      module: "objectives",
      name: "Objectives",
      tile: {
        kind: "objectives",
        xp: XP,
        mission: {
          conversationId: "shell-m",
          title: "Fix the server",
          status: "active",
          done: 1,
          total: 2,
          canAccept: false,
          accepted: false,
          steps: [
            { id: "1", text: "Read the logs", status: "verified" },
            { id: "2", text: "Skip it", status: "cancelled" },
            { id: "3", text: "Restart the service", status: "attempted" },
          ],
        },
      },
    }),
    slot({
      slot: "medium-1",
      size: "medium",
      module: "pc_loadout",
      name: "PC Loadout",
      purpose: "Health at a glance: score, temps, storage, RAM, startup apps",
      tile: { kind: "view", view: PC },
    }),
    slot({
      slot: "medium-2",
      size: "medium",
      module: "recent_chats",
      name: "Recent Chats",
      tile: {
        kind: "recentChats",
        chats: [
          { id: "shell-a", title: "Trip plan", updated: 2 },
          { id: "shell-b", title: "", updated: 1 },
        ],
      },
    }),
    slot({
      slot: "small-1",
      module: "automations",
      name: "Automations",
      tile: {
        kind: "automations",
        active: 2,
        next: [{ id: "a1", name: "Morning brief", summary: "daily at 08:00 · next today 8:00 AM" }],
      },
    }),
    slot({
      slot: "small-2",
      module: "pulse",
      name: "Pulse · For You",
      tile: {
        kind: "pulse",
        post: { id: "p1", title: "New GPU driver", summary: "", ago: "2h ago" },
        idea: { id: "i1", title: "I can tidy your downloads", action: "tidy my downloads folder" },
      },
    }),
    slot({
      slot: "small-3",
      module: "up_next",
      name: "Up Next",
      tile: { kind: "upNext", games: [{ appId: 10, name: "Portal 2" }] },
    }),
    slot({ slot: "small-4", module: "squad", name: "Squad", tile: { kind: "view", view: SQUAD } }),
  ],
};

function routes() {
  return { onOpen: vi.fn(), onOpenChat: vi.fn(), onAsk: vi.fn(), onSettings: vi.fn() };
}

const tile = (name: string) => screen.getByRole("listitem", { name });

describe("Home", () => {
  beforeEach(() => {
    api.getHome.mockReset();
    api.pulseAct.mockReset().mockResolvedValue({} as PulsePage);
    api.quickChips.mockReset().mockResolvedValue([]);
    api.chipOutcome.mockReset().mockResolvedValue(true);
    api.libraryPlay.mockReset().mockResolvedValue("Starting");
    api.homeOpenClip.mockReset().mockResolvedValue();
    api.libraryFriends.mockReset().mockResolvedValue();
    api.librarySteamSettings.mockReset().mockResolvedValue();
    api.homeRunPlaybook.mockReset().mockResolvedValue({ prompt: "Work on this workboard card: Linux repair" });
    api.homeTurnOn.mockReset().mockResolvedValue();
    api.homeSetup.mockReset().mockResolvedValue("Frame Rate is set up and counting frames.");
    api.homeSetupSteps.mockReset().mockResolvedValue("sudo setcap cap_perfmon+ep /usr/local/lib/loadout/fps-probe");
    api.openUrl.mockReset().mockResolvedValue();
  });

  it("renders the layout's tiles in order from the fixture", async () => {
    api.getHome.mockResolvedValue(PAGE);
    render(<Home {...routes()} conversationId="shell-m" remote={false} />);
    const grid = await screen.findByRole("list", { name: "Tiles" });
    expect(api.getHome).toHaveBeenCalledWith("shell-m");
    const names = within(grid)
      .getAllByRole("heading")
      .map((h) => h.textContent);
    expect(names).toEqual(["Objectives", "PC Loadout", "Recent Chats", "Automations", "Pulse · For You", "Up Next", "Squad"]);
    expect(tile("Objectives")).toHaveClass("home-tile", "large");
    // The amber-washed objective surface follows the module, not the large slot.
    expect(tile("Objectives")).toHaveClass("objective-tile");
    expect(tile("PC Loadout")).not.toHaveClass("objective-tile");
    expect(tile("Recent Chats")).toHaveClass("medium");
    expect(tile("Automations")).toHaveClass("small");

    const objectives = tile("Objectives");
    expect(objectives).toHaveTextContent("1 / 2");
    expect(objectives).toHaveTextContent("Fix the server");
    expect(within(objectives).getByText("Read the logs")).toHaveClass("done");
    expect(within(objectives).getByText("Restart the service")).not.toHaveClass("done");
    expect(within(objectives).queryByText("Skip it")).toBeNull();
    expect(objectives).toHaveTextContent("Lvl 3 · Rookie");
    expect(objectives).toHaveTextContent("1,240 XP");

    expect(within(tile("Recent Chats")).getByRole("button", { name: "Untitled chat" })).toBeInTheDocument();
    expect(tile("Automations")).toHaveTextContent("2 on");
    expect(tile("Automations")).toHaveTextContent("Morning briefdaily at 08:00 · next today 8:00 AM");
    expect(tile("Pulse · For You")).toHaveTextContent("New GPU driver2h ago");
    expect(tile("Up Next")).toHaveTextContent("Portal 2");

    // The page's XP bar and the glance row.
    const bar = screen.getAllByRole("progressbar", { name: "Level 3" })[0];
    expect(bar).toHaveAttribute("aria-valuenow", "48");
    expect(screen.getByText("1,240 / 1,500 XP")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Memory · 12 (2 to review)" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connectors · 2 / 3 connected · 1 failing" })).toBeInTheDocument();
    expect(screen.getByText(/Mission Control layout\./)).toBeInTheDocument();
  });

  it("draws every other module from the shared tile model, cut to its size", async () => {
    api.getHome.mockResolvedValue(PAGE);
    const go = routes();
    render(<Home {...go} remote={false} />);
    const pc = await screen.findByRole("listitem", { name: "PC Loadout" });
    expect(pc).not.toHaveClass("away");
    expect(pc).toHaveTextContent("RTX 4070");
    expect(pc).toHaveTextContent("12 GB VRAM · 32 GB RAM");
    // Medium holds four rows; the fifth waits for a Large slot.
    expect(within(pc).getAllByRole("listitem")).toHaveLength(4);
    expect(pc).not.toHaveTextContent("Extra");
    expect(within(pc).getByText("Free for models value")).toHaveClass("tone-warn");
    await userEvent.click(within(pc).getByRole("button", { name: "Tune-up" }));
    expect(go.onAsk).toHaveBeenLastCalledWith("Check this PC and suggest a tune-up.");
    // A Small empty tile says what it will show and is one tap.
    const squad = tile("Squad");
    expect(squad).toHaveTextContent("Friends online show here");
    expect(within(squad).queryByRole("button", { name: "Find a duo" })).toBeNull();
    await userEvent.click(within(squad).getByRole("button"));
    expect(go.onAsk).toHaveBeenLastCalledWith("Find a duo tonight");
    expect(screen.queryByText(/cabin\b(?! to change)/)).toBeNull();
  });

  it("a clip row plays that clip on the PC, by its place in the list", async () => {
    api.getHome.mockResolvedValue({
      ...PAGE,
      slots: [
        slot({
          slot: "large",
          size: "large",
          module: "clips",
          name: "Clips",
          tile: {
            kind: "view",
            view: view({
              text: { big: "Ace", caption: "Newest clip · 2h ago · 2 this week", detail: null, tone: "normal" },
              rows: [
                { label: "Ace", value: "2h ago", tone: "normal", tap: { kind: "openClip", index: 0 } },
                { label: "Clutch", value: "1d ago", tone: "normal", tap: { kind: "openClip", index: 1 } },
              ],
            }),
          },
        }),
      ],
    });
    render(<Home {...routes()} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: /Clutch/ }));
    expect(api.homeOpenClip).toHaveBeenCalledWith(1);
  });

  it("Turn on switches the internet log in place and reads Home again", async () => {
    const offPage = {
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "speeds",
          name: "Speeds",
          tile: {
            kind: "view",
            view: view({
              state: "off",
              text: { big: "Off", caption: "Turn on the internet log", detail: null, tone: "quiet" },
              buttons: [{ label: "Turn on", action: { kind: "turnOn", feature: "net_log" }, primary: true }],
            }),
          },
        }),
      ],
    };
    api.getHome.mockResolvedValue(offPage);
    const go = routes();
    render(<Home {...go} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Turn on" }));
    expect(api.homeTurnOn).toHaveBeenCalledWith("net_log");
    await vi.waitFor(() => expect(api.getHome.mock.calls.length).toBeGreaterThan(1));
    expect(go.onSettings).not.toHaveBeenCalled();
  });

  it("Edit Home opens the editor under the layout name", async () => {
    api.getHome.mockResolvedValue(PAGE);
    api.homeCatalog.mockReturnValue(new Promise(() => {}));
    render(<Home {...routes()} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Edit Home" }));
    expect(screen.getByRole("region", { name: "Edit Home" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit Home" })).not.toBeInTheDocument();
  });

  it("Frame Rate's Set up runs the setup on the PC, then reads Home again", async () => {
    const setupPage = {
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "frame_rate",
          name: "Frame Rate",
          tile: {
            kind: "view",
            view: view({
              text: { big: "Not read", caption: "Needs a one-time setup", detail: null, tone: "warn" },
              buttons: [
                { label: "Set up", action: { kind: "setup", module: "frame_rate", undo: false }, primary: true },
                { label: "Copy setup", action: { kind: "copySetup", module: "frame_rate" }, primary: false },
              ],
            }),
          },
        }),
      ],
    };
    api.getHome.mockResolvedValue(setupPage);
    const user = userEvent.setup();
    const { unmount } = render(<Home {...routes()} remote={false} />);
    await user.click(await screen.findByRole("button", { name: "Set up" }));
    expect(api.homeSetup).toHaveBeenCalledWith("frame_rate", false);
    await vi.waitFor(() => expect(api.getHome.mock.calls.length).toBeGreaterThan(1));
    await user.click(screen.getByRole("button", { name: "Copy setup" }));
    expect(api.homeSetupSteps).toHaveBeenCalledWith("frame_rate");
    await vi.waitFor(async () =>
      expect(await navigator.clipboard.readText()).toBe("sudo setcap cap_perfmon+ep /usr/local/lib/loadout/fps-probe"),
    );
    unmount();
    render(<Home {...routes()} remote />);
    await user.click(await screen.findByRole("button", { name: "Set up" }));
    expect(api.homeSetup).toHaveBeenCalledTimes(1);
  });

  it("the Squad tile opens Steam's friends list on the PC only", async () => {
    const squadPage = {
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "squad",
          name: "Squad",
          tile: {
            kind: "view",
            view: view({
              text: { big: "14 friends", caption: "On Steam · open the list to see who's on", detail: null, tone: "normal" },
              buttons: [{ label: "Friends list", action: { kind: "steamFriends" }, primary: true }],
            }),
          },
        }),
      ],
    };
    api.getHome.mockResolvedValue(squadPage);
    const { unmount } = render(<Home {...routes()} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Friends list" }));
    expect(api.libraryFriends).toHaveBeenCalledTimes(1);
    unmount();
    render(<Home {...routes()} remote />);
    await userEvent.click(await screen.findByRole("button", { name: "Friends list" }));
    expect(api.libraryFriends).toHaveBeenCalledTimes(1);
  });

  it("a Playbooks row starts it on a card and puts the ask in the chat box; Clips opens Steam's settings", async () => {
    api.getHome.mockResolvedValue({
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "clips",
          name: "Clips",
          tile: {
            kind: "view",
            view: view({
              text: { big: "", caption: "", detail: null, tone: "normal" },
              buttons: [{ label: "Steam settings", action: { kind: "steamSettings" }, primary: true }],
            }),
          },
        }),
        slot({
          slot: "medium-2",
          size: "medium",
          module: "playbooks",
          name: "Playbooks",
          tile: {
            kind: "view",
            view: view({
              buttons: [{ label: "Linux repair", action: { kind: "runPlaybook", id: "linux_repair" }, primary: true }],
            }),
          },
        }),
      ],
    });
    const go = routes();
    const { unmount } = render(<Home {...go} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Steam settings" }));
    expect(api.librarySteamSettings).toHaveBeenCalledTimes(1);
    await userEvent.click(screen.getByRole("button", { name: "Linux repair" }));
    expect(api.homeRunPlaybook).toHaveBeenCalledWith("linux_repair");
    await vi.waitFor(() => expect(go.onAsk).toHaveBeenCalledWith("Work on this workboard card: Linux repair"));
    unmount();
    // A paired browser starts nothing on the PC.
    const far = routes();
    render(<Home {...far} remote />);
    await userEvent.click(await screen.findByRole("button", { name: "Linux repair" }));
    expect(api.homeRunPlaybook).toHaveBeenCalledTimes(1);
    expect(far.onSettings).toHaveBeenCalledWith("playbooks");
  });

  it("a tile this device can't read draws quiet, and Play never starts a game from a browser", async () => {
    api.getHome.mockResolvedValue({
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "storage",
          name: "Storage",
          tile: { kind: "view", view: view({ state: "unsupported", text: { big: "—", caption: "Shows on the PC that runs Loadout", detail: null, tone: "quiet" } }) },
        }),
        slot({
          slot: "medium-2",
          size: "medium",
          module: "what_to_play",
          name: "What To Play",
          tile: {
            kind: "view",
            view: view({
              text: { big: "Portal 2", caption: "Last played 9d ago", detail: null, tone: "normal" },
              buttons: [{ label: "Play", action: { kind: "play", appId: 620 }, primary: true }],
            }),
          },
        }),
      ],
    });
    const { unmount } = render(<Home {...routes()} remote />);
    expect(await screen.findByRole("listitem", { name: "Storage" })).toHaveClass("away");
    await userEvent.click(within(tile("What To Play")).getByRole("button", { name: "Play" }));
    expect(api.libraryPlay).not.toHaveBeenCalled();
    unmount();
    render(<Home {...routes()} remote={false} />);
    await userEvent.click(within(await screen.findByRole("listitem", { name: "What To Play" })).getByRole("button", { name: "Play" }));
    expect(api.libraryPlay).toHaveBeenCalledWith(620);
  });

  it("on a paired browser a tile link opens a web page in a new tab, and nothing else", async () => {
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    const links = (url: string): HomePage => ({
      ...PAGE,
      slots: [
        slot({
          slot: "medium-1",
          size: "medium",
          module: "news",
          name: "News",
          tile: { kind: "view", view: view({ buttons: [{ label: "Read", action: { kind: "openUrl", url }, primary: true }] }) },
        }),
      ],
    });
    try {
      api.getHome.mockResolvedValue(links("javascript:alert(1)"));
      const { unmount } = render(<Home {...routes()} remote />);
      await userEvent.click(within(await screen.findByRole("listitem", { name: "News" })).getByRole("button", { name: "Read" }));
      expect(open).not.toHaveBeenCalled();
      unmount();

      api.getHome.mockResolvedValue(links("https://news.example/post"));
      render(<Home {...routes()} remote />);
      await userEvent.click(within(await screen.findByRole("listitem", { name: "News" })).getByRole("button", { name: "Read" }));
      expect(open.mock.calls).toEqual([["https://news.example/post", "_blank", "noopener,noreferrer"]]);
      expect(api.openUrl).not.toHaveBeenCalled();
    } finally {
      open.mockRestore();
    }
  });

  it("a tile click opens its page, a chat row its chat, and an idea fills the chat box", async () => {
    api.getHome.mockResolvedValue(PAGE);
    const go = routes();
    render(<Home {...go} remote={false} />);
    await userEvent.click(await screen.findByRole("button", { name: "Open Objectives" }));
    expect(go.onOpen).toHaveBeenLastCalledWith("objectives");
    await userEvent.click(screen.getByRole("button", { name: "Open Automations" }));
    expect(go.onOpen).toHaveBeenLastCalledWith("automations");
    await userEvent.click(screen.getByRole("button", { name: "Open Pulse" }));
    expect(go.onOpen).toHaveBeenLastCalledWith("pulse");
    await userEvent.click(screen.getByRole("button", { name: "Open Library" }));
    expect(go.onOpen).toHaveBeenLastCalledWith("library");
    await userEvent.click(screen.getByRole("button", { name: "Trip plan" }));
    expect(go.onOpenChat).toHaveBeenCalledWith("shell-a");
    await userEvent.click(screen.getByRole("button", { name: "I can tidy your downloads" }));
    expect(go.onAsk).toHaveBeenCalledWith("tidy my downloads folder");
    expect(api.pulseAct).toHaveBeenCalledWith("i1", "accept");
    await userEvent.click(screen.getByRole("button", { name: /^Memory/ }));
    expect(go.onOpen).toHaveBeenLastCalledWith("memory");
    await userEvent.click(screen.getByRole("button", { name: /^Connectors/ }));
    expect(go.onOpen).toHaveBeenLastCalledWith("connectors");
  });

  it("an empty checklist asks for a plan, and the streak tile shows weeks in a row", async () => {
    api.getHome.mockResolvedValue({
      ...PAGE,
      xpBar: false,
      slots: [
        slot({ slot: "large", size: "large", module: "objectives", name: "Objectives", tile: { kind: "objectives", mission: null, xp: XP } }),
        slot({ slot: "small-1", module: "streak", name: "Streak & XP", tile: { kind: "streak", xp: XP } }),
        slot({ slot: "small-2", module: "personality", name: "Personality", tile: { kind: "personality", name: "Viper", picture: null } }),
      ],
    });
    const go = routes();
    render(<Home {...go} remote={false} />);
    expect(await screen.findByText(NO_STEPS)).toBeInTheDocument();
    expect(tile("Streak & XP")).toHaveTextContent("Weeks in a row5");
    expect(tile("Streak & XP")).toHaveTextContent("This week: 3 of 4 days");
    expect(tile("Streak & XP")).toHaveTextContent("Lvl 3 · Rookie");
    // xp_bar off in the layout: no bar over the grid.
    expect(screen.queryByText("1,240 / 1,500 XP")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Open Personality settings" }));
    expect(go.onSettings).toHaveBeenCalledWith("personality");
  });

  it("the objectives tile shows a goal follow-up whose Plan it fills the box", async () => {
    const followUp = {
      id: "g2",
      title: "Elden Ring · Finish",
      line: "Want to pick a night for your next Elden Ring session?",
      prompt: "Help me pick a night this week for my next Elden Ring session. My goal there: Finish.",
      stop: false,
    };
    api.getHome.mockResolvedValue({
      ...PAGE,
      slots: [slot({ slot: "large", size: "large", module: "objectives", name: "Objectives", tile: { kind: "objectives", mission: null, xp: XP, followUp } })],
    });
    const go = routes();
    render(<Home {...go} remote={false} />);
    expect(await screen.findByText(followUp.line)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Plan it" }));
    expect(go.onAsk).toHaveBeenCalledWith(followUp.prompt);
    expect(go.onOpen).not.toHaveBeenCalled();
  });

  it("Home's chip row fills the box or opens a page, and teaches what was picked", async () => {
    api.getHome.mockResolvedValue(PAGE);
    api.quickChips.mockResolvedValue([
      { id: "pred-1", label: "Plan my day", value: "Plan my day", action: "ask", hint: "You often start here", primary: true },
      { id: "nav-1", label: "Automations", value: "__nav:night", action: "go", hint: "", primary: false, view: "automations" },
    ]);
    const go = routes();
    render(<Home {...go} remote={false} />);
    const row = await screen.findByRole("group", { name: "Suggestions" });
    expect(api.quickChips).toHaveBeenCalledWith("home", undefined, "", undefined);
    await userEvent.click(within(row).getByRole("button", { name: "Plan my day" }));
    expect(go.onAsk).toHaveBeenCalledWith("Plan my day");
    expect(api.chipOutcome).toHaveBeenCalledWith("home", "pred-1", true);
    await userEvent.click(await screen.findByRole("button", { name: "Automations" }));
    expect(go.onOpen).toHaveBeenCalledWith("automations");
  });

  it("says why when Home can't load, and tries again on a tap", async () => {
    api.getHome.mockRejectedValue("home_layout.json: no access");
    render(<Home {...routes()} remote />);
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("home_layout.json: no access");
    api.getHome.mockResolvedValue(PAGE);
    await userEvent.click(within(status).getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("listitem", { name: "PC Loadout" })).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("asks again at its fastest tile's pace and keeps the last read when offline", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      api.getHome.mockResolvedValue({
        ...PAGE,
        slots: PAGE.slots.map((s) => (s.module === "pc_loadout" ? { ...s, refreshSecs: 30 } : s)),
      });
      render(<Home {...routes()} remote={false} />);
      await screen.findByRole("listitem", { name: "PC Loadout" });
      expect(api.getHome).toHaveBeenCalledTimes(1);
      api.getHome.mockRejectedValue("offline");
      await vi.advanceTimersByTimeAsync(29_000);
      expect(api.getHome).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(2_000);
      await waitFor(() => expect(api.getHome).toHaveBeenCalledTimes(2));
      expect(await screen.findByRole("status")).toHaveTextContent("show what Home last read");
      expect(screen.getByRole("listitem", { name: "PC Loadout" })).toHaveTextContent("RTX 4070");
    } finally {
      vi.useRealTimers();
    }
  });

  it("polls no faster than ten seconds and marks data past twice its age", () => {
    const at = (refreshSecs: number | null) => slot({ slot: "small-1", module: "x", name: "X", tile: { kind: "view", view: PC }, refreshSecs });
    expect(pollSecs([at(2), at(60)])).toBe(10);
    expect(pollSecs([at(60), at(null)])).toBe(60);
    expect(pollSecs([at(null)])).toBeNull();
    const now = 10_000_000;
    expect(staleLabel(now - 100_000, 60, now)).toBeNull();
    expect(staleLabel(now - 300_000, 60, now)).toBe("Updated 5 min ago");
    expect(staleLabel(now - 3 * 3_600_000, 60, now)).toBe("Updated 3 h ago");
    expect(staleLabel(null, 60, now)).toBeNull();
  });

  it("groups XP as the cabin does", () => {
    expect(grouped(2340)).toBe("2,340");
    expect(grouped(0)).toBe("0");
  });
});
