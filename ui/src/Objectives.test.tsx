import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Mission, MissionPage, Xp } from "./api";

const api = vi.hoisted(() => ({
  getMission: vi.fn<(conversationId?: string) => Promise<MissionPage>>(),
  setMissionDone: vi.fn<(done: boolean, conversationId?: string) => Promise<MissionPage>>(),
}));

vi.mock("./api", () => api);

import { NO_STEPS } from "./Home";
import { dayCaption, Objectives } from "./Objectives";

const XP: Xp = { level: 1, xp: 0, nextLevelXp: 500, frac: 0, today: 0, streak: [false, false, false, false, false, false, false] };

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
    api.getMission.mockResolvedValue({ mission: mission(), xp: { ...XP, xp: 320, frac: 0.64, today: 110 } });
    const onOpenChat = vi.fn();
    view({ onOpenChat });
    const list = await screen.findByRole("list", { name: "Steps" });
    expect(api.getMission).toHaveBeenCalledWith("shell-m");
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent)).toEqual(["Write the notesCHECK", "Tag itIN PROGRESS", "Announce it"]);
    expect(rows[0]).toHaveClass("unverified", "current");
    expect(rows[1]).not.toHaveClass("current");
    expect(screen.getByText("0 / 3")).toBeInTheDocument();
    expect(screen.getByText("320 / 500 XP")).toBeInTheDocument();
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
      xp: { ...XP, xp: 160, frac: 0.32, today: 160 },
    });
    view();
    await userEvent.click(await screen.findByRole("button", { name: "Accept as done" }));
    expect(api.setMissionDone).toHaveBeenCalledWith(true, "shell-m");
    expect(await screen.findByText("2 / 2")).toBeInTheDocument();
    expect(screen.getByText("160 / 500 XP")).toBeInTheDocument();
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
    api.getMission.mockResolvedValue({ mission: null, xp: { ...XP, streak: [true, false, false, false, false, false, true] } });
    const onHome = vi.fn();
    view({ onHome });
    expect(await screen.findByText(NO_STEPS)).toBeInTheDocument();
    expect(screen.getByText("2 DAYS")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Back to Home" }));
    expect(onHome).toHaveBeenCalled();
  });

  it("writes the day as the cabin's caption", () => {
    expect(dayCaption(new Date(2026, 9, 28))).toBe("WEDNESDAY · OCT 28");
  });
});
