import "@testing-library/jest-dom/vitest";
import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRow, HomePage, MissionPage, PulsePage, TileView } from "./api";

const api = vi.hoisted(() => ({
  getMission: vi.fn<(id?: string) => Promise<MissionPage>>(),
  getHome: vi.fn<(id?: string) => Promise<HomePage>>(),
  getPcSnapshot: vi.fn<() => Promise<TileView>>(),
  getPulse: vi.fn<() => Promise<PulsePage>>(),
  listChats: vi.fn<() => Promise<ChatRow[]>>(),
}));
vi.mock("./api", () => api);

// Labs → Desktop companion, for the left column's companion card.
const desk = vi.hoisted(() => ({
  desktopNyxGet: vi.fn(() => Promise.resolve({ enabled: false, undocked: false, outfit: null, outfits: [] })),
  desktopNyxUndock: vi.fn(() => Promise.resolve({ enabled: true, undocked: true, outfit: null, outfits: [] })),
  desktopNyxDock: vi.fn(),
  desktopNyxSetOutfit: vi.fn(),
  onDesktopNyx: vi.fn(() => Promise.resolve(() => {})),
}));
vi.mock("./desktopNyxApi", () => desk);
vi.mock("./transport", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./transport")>()),
  inShell: () => true,
}));

import { ChatLeft, ChatSide, ChatSnapshot, RECENT_EMPTY, recentChats } from "./ChatSide";
import { NO_STEPS } from "./Home";

const XP = { level: 14, title: "Grinder", xp: 2340, nextLevelXp: 2600, frac: 0.9, today: 0, week: { days: [], streak: 0 } } as unknown as MissionPage["xp"];

const MISSION: MissionPage = {
  xp: XP,
  mission: {
    conversationId: "c1",
    title: "Diamond by Sunday",
    status: "active",
    done: 1,
    total: 3,
    canAccept: false,
    accepted: false,
    steps: [
      { id: "1", text: "Aim routine 12/20", status: "verified" },
      { id: "2", text: "Three ranked wins", status: "attempted" },
      { id: "3", text: "Skip this one", status: "cancelled" },
      { id: "4", text: "Review one replay", status: "pending" },
    ],
  } as unknown as MissionPage["mission"],
};

const PC_SLOT = {
  slot: "medium-1",
  size: "medium",
  module: "pc_loadout",
  name: "PC",
  purpose: "",
  refreshSecs: 5,
  tile: {
    kind: "view",
    view: {
      state: "ok",
      tag: null,
      text: { big: "92%", caption: "GPU 74° · driver latest", detail: null, tone: "normal" },
      rows: [
        { label: "GPU", value: "74°", tone: "warn", tap: null },
        { label: "Driver", value: "Latest", tone: "normal", tap: null },
        { label: "Memory", value: "32 GB", tone: "normal", tap: null },
      ],
      progress: 92,
      buttons: [{ label: "Tune-up", action: { kind: "ask", ask: "Tune up my PC" }, primary: true }],
      tap: null,
      updatedMs: null,
    },
  },
};

const home = (slots: unknown[]) => ({ preset: "mission_control", xpBar: true, slots, xp: XP }) as unknown as HomePage;

const PULSE = {
  feed: [
    { id: "p1", title: "New GPU driver", ago: "2h ago" },
    { id: "p2", title: "Season patch notes", ago: "3h ago" },
    { id: "p3", title: "A third post", ago: "5h ago" },
  ],
  today: 2,
  note: "",
  ideas: [],
} as unknown as PulsePage;

describe("ChatSide", () => {
  beforeEach(() => {
    api.getMission.mockReset().mockResolvedValue(MISSION);
    api.getHome.mockReset().mockResolvedValue(home([PC_SLOT]));
    api.getPcSnapshot.mockReset().mockResolvedValue(PC_SLOT.tile.view as unknown as TileView);
    api.getPulse.mockReset().mockResolvedValue(PULSE);
  });

  it("shows the open chat's mission, the PC and the newest two posts, each with a way to its page", async () => {
    const onOpen = vi.fn();
    render(<ChatSide active conversationId="c1" pulse onOpen={onOpen} />);
    const objectives = await screen.findByRole("region", { name: "Objectives" });
    expect(within(objectives).getByText("1 / 3")).toBeVisible();
    expect(within(objectives).getByText("Diamond by Sunday")).toBeVisible();
    expect(within(objectives).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Aim routine 12/20",
      "Three ranked wins",
      "Review one replay",
    ]);
    expect(api.getMission).toHaveBeenCalledWith("c1");

    const pc = await screen.findByRole("region", { name: "PC loadout" });
    expect(within(pc).getByText("92%")).toBeVisible();
    expect(within(pc).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["GPU74°", "DriverLatest"]);
    // The Tune-up CTA stays on Home: amber is the chat's Send here.
    expect(within(pc).queryByRole("button", { name: "Tune-up" })).toBeNull();

    const pulse = await screen.findByRole("region", { name: "From Pulse" });
    expect(within(pulse).getByText("2 new")).toBeVisible();
    expect(within(pulse).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "New GPU driver2h ago",
      "Season patch notes3h ago",
    ]);

    await userEvent.click(within(objectives).getByRole("button", { name: "Open Objectives" }));
    await userEvent.click(within(pc).getByRole("button", { name: "Open Home" }));
    await userEvent.click(within(pulse).getByRole("button", { name: "Open Pulse" }));
    expect(onOpen.mock.calls).toEqual([["objectives"], ["home"], ["pulse"]]);
  });

  it("says there is nothing planned yet, and shows this PC without reading Home", async () => {
    api.getMission.mockResolvedValue({ mission: null, xp: XP });
    api.getHome.mockResolvedValue(home([]));
    render(<ChatSide active pulse onOpen={() => {}} />);
    const objectives = await screen.findByRole("region", { name: "Objectives" });
    expect(within(objectives).getByText(NO_STEPS)).toBeVisible();
    await screen.findByRole("region", { name: "From Pulse" });
    // The PC card is this PC's own tile, whether or not Home holds it.
    const pc = await screen.findByRole("region", { name: "PC loadout" });
    expect(within(pc).getByText("GPU 74° · driver latest")).toBeVisible();
    expect(api.getPcSnapshot).toHaveBeenCalledTimes(1);
    expect(api.getHome).not.toHaveBeenCalled();
  });

  it("a paired browser takes the PC card from the host's Home, and none when Home lacks it", async () => {
    const { unmount } = render(<ChatSide active pulse remote onOpen={() => {}} />);
    const pc = await screen.findByRole("region", { name: "PC loadout" });
    expect(within(pc).getByText("92%")).toBeVisible();
    expect(api.getHome).toHaveBeenCalledWith(undefined);
    unmount();
    api.getHome.mockResolvedValue(home([]));
    render(<ChatSide active pulse remote onOpen={() => {}} />);
    await screen.findByRole("region", { name: "From Pulse" });
    await act(async () => {});
    expect(screen.queryByRole("region", { name: "PC loadout" })).toBeNull();
    expect(api.getPcSnapshot).not.toHaveBeenCalled();
    expect(desk.desktopNyxGet).not.toHaveBeenCalled();
  });

  it("leaves the PC card out on a phone, which has no PC to show", async () => {
    api.getPcSnapshot.mockResolvedValue({ ...(PC_SLOT.tile.view as unknown as TileView), state: "unsupported" });
    render(<ChatSide active pulse onOpen={() => {}} />);
    await screen.findByRole("region", { name: "From Pulse" });
    await act(async () => {});
    expect(api.getPcSnapshot).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("region", { name: "PC loadout" })).toBeNull();
  });

  it("reads no Pulse where the app has none, and nothing while Chat is hidden", async () => {
    vi.useFakeTimers();
    const { rerender } = render(<ChatSide active={false} pulse={false} onOpen={() => {}} />);
    render(<ChatSide active={false} pulse={false} remote onOpen={() => {}} />);
    // Let a read that a hidden column wrongly starts land, and a poll tick pass.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(31_000);
    });
    vi.useRealTimers();
    expect(api.getMission).not.toHaveBeenCalled();
    expect(api.getHome).not.toHaveBeenCalled();
    expect(api.getPcSnapshot).not.toHaveBeenCalled();
    rerender(<ChatSide active pulse={false} onOpen={() => {}} />);
    await screen.findByRole("region", { name: "Objectives" });
    expect(api.getPulse).not.toHaveBeenCalled();
    expect(screen.queryByRole("region", { name: "From Pulse" })).toBeNull();
  });

  it("leaves a card out when its read fails", async () => {
    api.getMission.mockRejectedValue(new Error("refused"));
    api.getPulse.mockRejectedValue(new Error("refused"));
    render(<ChatSide active pulse onOpen={() => {}} />);
    await screen.findByRole("region", { name: "PC loadout" });
    expect(screen.queryByRole("region", { name: "Objectives" })).toBeNull();
    expect(screen.queryByRole("region", { name: "From Pulse" })).toBeNull();
  });

  it("drops the PC card when this PC's read starts failing", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<ChatSide active pulse={false} onOpen={() => {}} />);
      expect(await screen.findByRole("region", { name: "PC loadout" })).toBeVisible();
      api.getPcSnapshot.mockRejectedValue(new Error("refused"));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(30_000);
      });
      expect(api.getPcSnapshot).toHaveBeenCalledTimes(2);
      expect(screen.queryByRole("region", { name: "PC loadout" })).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});

const chat = (id: string, title: string, updated: number, extra: Partial<ChatRow> = {}): ChatRow => ({
  id,
  title,
  created: 0,
  updated,
  ...extra,
});

describe("Objectives beside chat", () => {
  beforeEach(() => {
    for (const fn of Object.values(desk)) fn.mockClear();
    api.getMission.mockReset().mockResolvedValue(MISSION);
    api.getPcSnapshot.mockReset().mockResolvedValue(PC_SLOT.tile.view as TileView);
    api.listChats
      .mockReset()
      .mockResolvedValue([
        chat("main", "Main", 900, { main: true }),
        chat("c1", "Open", 800),
        chat("c2", "Fan curve tweak", 300),
        chat("c3", "Backlog sort", 700, { pinned: true }),
        chat("c4", "  ", 500),
      ]);
  });

  it("lists the objective's steps with the current one marked and done ones struck", async () => {
    render(<ChatLeft active conversationId="c1" onOpen={() => {}} onOpenChat={() => {}} />);
    const steps = await screen.findByRole("region", { name: "Steps" });
    const items = await within(steps).findAllByRole("listitem");
    expect(items.map((li) => li.textContent)).toEqual(["Aim routine 12/20", "Three ranked wins", "Review one replay"]);
    expect(items.map((li) => li.getAttribute("aria-current"))).toEqual([null, "step", null]);
    expect(items.map((li) => li.className)).toEqual(["done", "current", ""]);
    expect(within(steps).getByText("1 / 3")).toBeVisible();
    expect(within(steps).getByText("Diamond by Sunday")).toBeVisible();
    expect(api.getMission).toHaveBeenCalledWith("c1");
  });

  it("lists other chats newest first, without the open one or Main, and opens a pick", async () => {
    const onOpenChat = vi.fn();
    render(<ChatLeft active conversationId="c1" onOpen={() => {}} onOpenChat={onOpenChat} />);
    const recent = await screen.findByRole("region", { name: "Recent chats" });
    const rows = await within(recent).findAllByRole("button");
    expect(rows.map((b) => b.textContent)).toEqual(["Backlog sort", "Untitled chat", "Fan curve tweak"]);
    await userEvent.click(rows[0]);
    expect(onOpenChat).toHaveBeenCalledWith("c3");
  });

  it("keeps five recent chats at most", () => {
    const rows = Array.from({ length: 8 }, (_, i) => chat(`r${i}`, `Chat ${i}`, i));
    expect(recentChats(rows, "r7").map((r) => r.id)).toEqual(["r6", "r5", "r4", "r3", "r2"]);
  });

  it("says so when there is no objective or no other chat", async () => {
    api.getMission.mockResolvedValue({ xp: XP, mission: null });
    api.listChats.mockResolvedValue([chat("c1", "Open", 800)]);
    const onOpen = vi.fn();
    render(<ChatLeft active conversationId="c1" onOpen={onOpen} onOpenChat={() => {}} />);
    expect(await screen.findByText(NO_STEPS)).toBeVisible();
    expect(await screen.findByText(RECENT_EMPTY)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Open Objectives" }));
    expect(onOpen).toHaveBeenCalledWith("objectives");
  });

  it("shows no companion card while Labs → Desktop companion is off", async () => {
    render(<ChatLeft active conversationId="c1" onOpen={() => {}} onOpenChat={() => {}} />);
    await screen.findByRole("region", { name: "Steps" });
    await act(async () => {});
    expect(screen.queryByRole("region", { name: "Nyx" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Put her on the desktop" })).toBeNull();
  });

  it("shows the companion card with its controls when Labs → Desktop companion is on", async () => {
    desk.desktopNyxGet.mockResolvedValueOnce({ enabled: true, undocked: false, outfit: null, outfits: [] });
    render(<ChatLeft active conversationId="c1" onOpen={() => {}} onOpenChat={() => {}} />);
    const card = await screen.findByRole("region", { name: "Nyx" });
    expect(screen.getByRole("complementary", { name: "Objectives" })).toContainElement(card);
    await userEvent.click(within(card).getByRole("button", { name: "Put her on the desktop" }));
    expect(desk.desktopNyxUndock).toHaveBeenCalledTimes(1);
    expect(await within(card).findByRole("button", { name: "Bring her back" })).toBeVisible();
    expect(within(card).getByLabelText("Outfit")).toHaveValue("");
  });

  it("shows the companion card on the way back once the Lab is turned on", async () => {
    const left = (active: boolean) => (
      <ChatLeft active={active} conversationId="c1" onOpen={() => {}} onOpenChat={() => {}} />
    );
    const view = render(left(true));
    await screen.findByRole("region", { name: "Steps" });
    await act(async () => {});
    expect(screen.queryByRole("region", { name: "Nyx" })).toBeNull();

    // Off to Settings → Labs, where Desktop companion is turned on.
    view.rerender(left(false));
    desk.desktopNyxGet.mockResolvedValueOnce({ enabled: true, undocked: false, outfit: null, outfits: [] });
    view.rerender(left(true));
    const card = await screen.findByRole("region", { name: "Nyx" });
    expect(within(card).getByRole("button", { name: "Put her on the desktop" })).toBeVisible();
    expect(desk.desktopNyxGet).toHaveBeenCalledTimes(2);
  });

  it("reads nothing while Chat is hidden", async () => {
    render(<ChatLeft active={false} conversationId="c1" onOpen={() => {}} onOpenChat={() => {}} />);
    render(<ChatSnapshot active={false} onOpen={() => {}} />);
    await act(async () => {});
    expect(api.getMission).not.toHaveBeenCalled();
    expect(api.listChats).not.toHaveBeenCalled();
    expect(api.getPcSnapshot).not.toHaveBeenCalled();
  });

  it("shows three readings from this PC, the main one first", async () => {
    const onOpen = vi.fn();
    render(<ChatSnapshot active onOpen={onOpen} />);
    const snap = await screen.findByRole("complementary", { name: "Snapshot" });
    const tiles = within(snap).getAllByRole("listitem");
    expect(tiles.map((li) => li.textContent)).toEqual(["92%GPU 74° · driver latest", "74°GPU", "LatestDriver"]);
    expect(tiles[0]).toHaveClass("lead");
    expect(within(tiles[1]).getByText("74°")).toHaveClass("tone-warn");
    await userEvent.click(within(snap).getByRole("button", { name: "Open Home" }));
    expect(onOpen).toHaveBeenCalledWith("home");
  });

  it("shows no snapshot on a phone or when the read fails", async () => {
    api.getPcSnapshot.mockResolvedValue({ ...(PC_SLOT.tile.view as TileView), state: "unsupported" });
    const { unmount } = render(<ChatSnapshot active onOpen={() => {}} />);
    await act(async () => {});
    expect(screen.queryByRole("complementary", { name: "Snapshot" })).toBeNull();
    unmount();
    api.getPcSnapshot.mockRejectedValue(new Error("no"));
    render(<ChatSnapshot active onOpen={() => {}} />);
    await act(async () => {});
    expect(screen.queryByRole("complementary", { name: "Snapshot" })).toBeNull();
    expect(api.getPcSnapshot).toHaveBeenCalledTimes(2);
  });
});
