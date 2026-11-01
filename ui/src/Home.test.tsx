import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePage, HomeSlot, Xp } from "./api";

const api = vi.hoisted(() => ({
  getHome: vi.fn<(conversationId?: string) => Promise<HomePage>>(),
}));

vi.mock("./api", () => api);

import { CABIN_NOTE, DESKTOP_NOTE, Home, NO_STEPS, grouped } from "./Home";

const XP: Xp = {
  level: 3,
  xp: 1240,
  nextLevelXp: 1500,
  frac: 0.48,
  today: 110,
  streak: [false, true, false, false, true, false, true],
};

function slot(over: Partial<HomeSlot> & Pick<HomeSlot, "slot" | "module" | "name" | "tile">): HomeSlot {
  return { size: "small", purpose: "", ...over };
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
      tile: { kind: "cabin" },
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
    slot({ slot: "small-4", module: "squad", name: "Squad", tile: { kind: "desktop" } }),
  ],
};

function routes() {
  return { onOpen: vi.fn(), onOpenChat: vi.fn(), onAsk: vi.fn(), onSettings: vi.fn() };
}

const tile = (name: string) => screen.getByRole("listitem", { name });

describe("Home", () => {
  beforeEach(() => {
    api.getHome.mockReset();
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
    expect(tile("Recent Chats")).toHaveClass("medium");
    expect(tile("Automations")).toHaveClass("small");

    const objectives = tile("Objectives");
    expect(objectives).toHaveTextContent("1 / 2");
    expect(objectives).toHaveTextContent("Fix the server");
    expect(within(objectives).getByText("Read the logs")).toHaveClass("done");
    expect(within(objectives).getByText("Restart the service")).not.toHaveClass("done");
    expect(within(objectives).queryByText("Skip it")).toBeNull();
    expect(objectives).toHaveTextContent("Lvl 3");
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
    expect(screen.getByText(/Mission Control layout\. Edit Home in the cabin/)).toBeInTheDocument();
  });

  it("shows the cabin's note on a tile the shell can't fill, and the computer's on the web", async () => {
    api.getHome.mockResolvedValue(PAGE);
    render(<Home {...routes()} remote={false} />);
    const pc = await screen.findByRole("listitem", { name: "PC Loadout" });
    expect(pc).toHaveClass("away");
    expect(within(pc).getByText(CABIN_NOTE)).toBeInTheDocument();
    expect(pc).toHaveTextContent("Health at a glance");
    expect(within(pc).queryByRole("button")).toBeNull();
    const squad = tile("Squad");
    expect(within(squad).getByText(DESKTOP_NOTE)).toBeInTheDocument();
    expect(within(squad).queryByText(CABIN_NOTE)).toBeNull();
    // Filled tiles carry no note.
    expect(within(tile("Automations")).queryByText(CABIN_NOTE)).toBeNull();
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
    await userEvent.click(screen.getByRole("button", { name: /^Memory/ }));
    expect(go.onOpen).toHaveBeenLastCalledWith("memory");
    await userEvent.click(screen.getByRole("button", { name: /^Connectors/ }));
    expect(go.onOpen).toHaveBeenLastCalledWith("connectors");
  });

  it("an empty checklist asks for a plan, and the streak tile counts its days", async () => {
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
    expect(tile("Streak & XP")).toHaveTextContent("3 DAYS");
    // xp_bar off in the layout: no bar over the grid.
    expect(screen.queryByText("1,240 / 1,500 XP")).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Open Personality settings" }));
    expect(go.onSettings).toHaveBeenCalledWith("personality");
  });

  it("says why when Home can't load", async () => {
    api.getHome.mockRejectedValue("home_layout.json: no access");
    render(<Home {...routes()} remote />);
    expect(await screen.findByRole("status")).toHaveTextContent("home_layout.json: no access");
  });

  it("groups XP as the cabin does", () => {
    expect(grouped(2340)).toBe("2,340");
    expect(grouped(0)).toBe("0");
  });
});
