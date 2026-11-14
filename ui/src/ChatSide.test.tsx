import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HomePage, MissionPage, PulsePage } from "./api";

const api = vi.hoisted(() => ({
  getMission: vi.fn<(id?: string) => Promise<MissionPage>>(),
  getHome: vi.fn<(id?: string) => Promise<HomePage>>(),
  getPulse: vi.fn<() => Promise<PulsePage>>(),
}));
vi.mock("./api", () => api);

import { ChatSide } from "./ChatSide";
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

  it("says there is nothing planned yet, and leaves out a PC tile Home doesn't hold", async () => {
    api.getMission.mockResolvedValue({ mission: null, xp: XP });
    api.getHome.mockResolvedValue(home([]));
    render(<ChatSide active pulse onOpen={() => {}} />);
    const objectives = await screen.findByRole("region", { name: "Objectives" });
    expect(within(objectives).getByText(NO_STEPS)).toBeVisible();
    await screen.findByRole("region", { name: "From Pulse" });
    expect(screen.queryByRole("region", { name: "PC loadout" })).toBeNull();
  });

  it("reads no Pulse where the app has none, and nothing while Chat is hidden", async () => {
    const { rerender } = render(<ChatSide active={false} pulse={false} onOpen={() => {}} />);
    expect(api.getMission).not.toHaveBeenCalled();
    expect(api.getHome).not.toHaveBeenCalled();
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
});
