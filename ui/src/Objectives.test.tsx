import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Mission, MissionPage, Xp } from "./api";

const api = vi.hoisted(() => ({
  getMission: vi.fn<(conversationId?: string) => Promise<MissionPage>>(),
  setMissionDone: vi.fn<(done: boolean, conversationId?: string) => Promise<MissionPage>>(),
  getGoals: vi.fn(() =>
    Promise.resolve({ today: [], mine: [], suggested: [], season: { label: "Oct to Dec 2026", done: 0, trophies: [], badges: [] } }),
  ),
  goalAct: vi.fn(),
}));

vi.mock("./api", () => api);

import { NO_STEPS } from "./Home";
import { dayCaption, Objectives } from "./Objectives";
import { VIEW_LABELS, views } from "./Palette";

const XP: Xp = {
  level: 1,
  title: "Rookie",
  xp: 0,
  nextLevelXp: 200,
  frac: 0,
  today: 0,
  week: { days: [false, false, false, false, false, false, false], today: 2, active: 0, target: 2, inARow: 0, played: 0, freezes: 2 },
};

function mission(over: Partial<Mission> = {}): Mission {
  return {
    conversationId: "shell-m",
    title: "Ship the release",
    status: "active",
    done: 0,
    total: 3,
    canAccept: false,
    accepted: false,
    steps: [
      { id: "1", text: "Write the notes", status: "unverified" },
      { id: "2", text: "Old plan", status: "cancelled" },
      { id: "3", text: "Tag it", status: "attempted" },
      { id: "4", text: "Announce it", status: "planned" },
    ],
    ...over,
  };
}

function view(props: Partial<{ remote: boolean; onOpenChat: (id: string) => void; onHome: () => void }> = {}) {
  return render(
    <Objectives conversationId="shell-m" remote={false} onOpenChat={vi.fn()} onHome={vi.fn()} {...props} />,
  );
}

describe("Objectives", () => {
  beforeEach(() => {
    api.getMission.mockReset();
    api.setMissionDone.mockReset();
  });

  it("lists the open chat's steps with the cabin's tags, the current one marked", async () => {
    api.getMission.mockResolvedValue({ mission: mission(), xp: { ...XP, level: 2, xp: 320, nextLevelXp: 450, frac: 0.48, today: 110 } });
    const onOpenChat = vi.fn();
    view({ onOpenChat });
    const list = await screen.findByRole("list", { name: "Steps" });
    expect(api.getMission).toHaveBeenCalledWith("shell-m");
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual(["Write the notesCHECK", "Tag itIN PROGRESS", "Announce it"]);
    expect(rows[0]).toHaveClass("unverified", "current");
    expect(rows[1]).not.toHaveClass("current");
    expect(screen.getByText("0 / 3")).toBeInTheDocument();
    expect(screen.getByText("320 / 450 XP")).toBeInTheDocument();
    expect(screen.getByText("+110 XP today")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Accept as done" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Open the chat" }));
    expect(onOpenChat).toHaveBeenCalledWith("shell-m");
  });

  it("Accept as done writes through the desktop and shows the checked steps", async () => {
    const closed = mission({
      canAccept: true,
      steps: [
        { id: "1", text: "Write the notes", status: "unverified" },
        { id: "2", text: "Tag it", status: "unverified" },
      ],
      total: 2,
    });
    api.getMission.mockResolvedValue({ mission: closed, xp: XP });
    api.setMissionDone.mockResolvedValue({
      mission: {
        ...closed,
        status: "done",
        done: 2,
        canAccept: false,
        accepted: true,
        steps: closed.steps.map((s) => ({ ...s, status: "verified" as const })),
      },
      xp: { ...XP, xp: 160, frac: 0.8, today: 160 },
    });
    view();
    await userEvent.click(await screen.findByRole("button", { name: "Accept as done" }));
    expect(api.setMissionDone).toHaveBeenCalledWith(true, "shell-m");
    expect(await screen.findByText("2 / 2")).toBeInTheDocument();
    expect(screen.getByText("160 / 200 XP")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Accepted.");
    expect(screen.queryByRole("button", { name: "Accept as done" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(api.setMissionDone).toHaveBeenLastCalledWith(false, "shell-m");
  });

  it("the web app reads the checklist but never accepts it", async () => {
    api.getMission.mockResolvedValue({ mission: mission({ canAccept: true, accepted: true }), xp: XP });
    view({ remote: true });
    expect(await screen.findByText("Tag it")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Accept as done" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("no checklist asks for a plan, and Back goes Home", async () => {
    api.getMission.mockResolvedValue({
      mission: null,
      xp: { ...XP, week: { ...XP.week, days: [true, true, false, false, false, false, false], active: 2, inARow: 3 } },
    });
    const onHome = vi.fn();
    view({ onHome });
    expect(await screen.findByText(NO_STEPS)).toBeInTheDocument();
    expect(screen.getByText("This week: 2 of 2 days")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Back to Home" }));
    expect(onHome).toHaveBeenCalled();
  });

  it("writes the day as the cabin's caption", () => {
    expect(dayCaption(new Date(2026, 9, 28))).toBe("WEDNESDAY · OCT 28");
  });

  it("does not render a Workboards control, and the Workboard tab still exists", async () => {
    api.getMission.mockResolvedValue({ mission: mission(), xp: XP });
    view();
    expect(await screen.findByRole("heading", { name: "Objectives" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /workboards?/i })).toBeNull();
    expect(screen.queryByText(/workboards?/i)).toBeNull();
    const tabs = views({
      desktopControl: false,
      pulse: true,
      localAi: false,
      tray: false,
      webAccess: true,
      plugins: false,
      library: true,
      code: true,
    });
    expect(tabs).toContain("workboard");
    expect(VIEW_LABELS.workboard).toBe("Workboard");
  });
});
