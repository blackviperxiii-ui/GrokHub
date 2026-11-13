import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { GoalAct, GoalBoard, GoalCard, GoalsSaved } from "./api";

const api = vi.hoisted(() => ({
  getGoals: vi.fn<() => Promise<GoalBoard>>(),
  goalAct: vi.fn<(acts: GoalAct[]) => Promise<GoalsSaved>>(),
}));

vi.mock("./api", () => api);

import { doneLine, FollowUpRow, GoalAsk, Goals, MINE_EMPTY, PLAN_IT, SHELF_EMPTY, TODAY_EMPTY, YOUR_WEEK } from "./Goals";

const EMPTY: GoalBoard = { today: [], mine: [], suggested: [], season: { label: "Oct to Dec 2026", done: 0, trophies: [], badges: [] } };

const FISHING: GoalCard = {
  id: "g1",
  title: "Old School RuneScape · 99 Fishing",
  detail: "93 / 99 · 6 levels to 99",
  frac: 0.94,
  plusOne: true,
};

const ELDEN: GoalCard = { id: "g2", title: "Elden Ring · Finish", detail: "Not finished yet", frac: 0, plusOne: false };

describe("Goals", () => {
  beforeEach(() => {
    api.getGoals.mockReset();
    api.goalAct.mockReset();
  });

  it("shows a follow-up on Today whose Plan it puts the ask in the chat box", async () => {
    const ask = "Help me pick a night this week for my next Elden Ring session. My goal there: Finish.";
    api.getGoals.mockResolvedValue({
      ...EMPTY,
      mine: [ELDEN],
      followUp: { id: "g2", title: ELDEN.title, line: "Want to pick a night for your next Elden Ring session?", prompt: ask, stop: false },
    });
    const onAsk = vi.fn();
    render(<Goals onAsk={onAsk} />);
    expect(await screen.findByText("Want to pick a night for your next Elden Ring session?")).toBeInTheDocument();
    expect(screen.queryByText(TODAY_EMPTY)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: PLAN_IT }));
    expect(onAsk).toHaveBeenCalledWith(ask);
    expect(api.goalAct).not.toHaveBeenCalled();
  });

  it("shows the last follow-up as a note with no button", () => {
    const line = "I'll stop asking about Elden Ring, just say the word.";
    render(<FollowUpRow followUp={{ id: "g2", title: ELDEN.title, line, prompt: "", stop: true }} onAsk={vi.fn()} />);
    expect(screen.getByText(line)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("says what to do when there are no goals yet", async () => {
    api.getGoals.mockResolvedValue(EMPTY);
    render(<Goals />);
    expect(await screen.findByText(TODAY_EMPTY)).toBeInTheDocument();
    expect(screen.getByText(MINE_EMPTY)).toBeInTheDocument();
    expect(screen.getByText(SHELF_EMPTY)).toBeInTheDocument();
    expect(screen.getByText("0 goals done this season")).toBeInTheDocument();
    expect(screen.getByText("Oct to Dec 2026")).toBeInTheDocument();
  });

  it("adds a typed goal, clears the box and tells the page", async () => {
    api.getGoals.mockResolvedValue(EMPTY);
    api.goalAct.mockResolvedValue({ board: { ...EMPTY, mine: [FISHING], today: [FISHING] } });
    const onChange = vi.fn();
    render(<Goals onChange={onChange} />);
    const box = await screen.findByRole("textbox", { name: "New goal" });
    await userEvent.type(box, "99 fishing in osrs{Enter}");
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "add", text: "99 fishing in osrs" }]);
    const list = await screen.findByRole("list", { name: "Goals" });
    expect(within(list).getByText("Old School RuneScape · 99 Fishing")).toBeInTheDocument();
    expect(within(list).getByText("93 / 99 · 6 levels to 99")).toBeInTheDocument();
    expect(box).toHaveValue("");
    expect(onChange).toHaveBeenCalledTimes(1);
  });

  it("keeps the words and shows why when a goal is refused", async () => {
    api.getGoals.mockResolvedValue(EMPTY);
    api.goalAct.mockResolvedValue({ board: EMPTY, note: "Say what you want to do first." });
    render(<Goals />);
    const box = await screen.findByRole("textbox", { name: "New goal" });
    await userEvent.type(box, "x");
    await userEvent.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Say what you want to do first.");
    expect(box).toHaveValue("x");
  });

  it("gives +1 only to step goals and sends each press by id", async () => {
    api.getGoals.mockResolvedValue({ ...EMPTY, mine: [FISHING, ELDEN] });
    api.goalAct.mockResolvedValue({ board: EMPTY });
    render(<Goals />);
    const list = await screen.findByRole("list", { name: "Goals" });
    const [fish, elden] = within(list).getAllByRole("listitem");
    expect(within(elden).queryByRole("button", { name: "+1" })).not.toBeInTheDocument();
    await userEvent.click(within(fish).getByRole("button", { name: "+1" }));
    expect(api.goalAct).toHaveBeenLastCalledWith([{ act: "bump", id: "g1" }]);
    await screen.findByText(MINE_EMPTY);
  });

  it("shows a pick-up prompt on Today, the suggested tray and the trophy shelf", async () => {
    api.getGoals.mockResolvedValue({
      today: [FISHING, { ...ELDEN, prompt: "Pick up Elden Ring tonight?" }],
      mine: [FISHING, ELDEN],
      suggested: [{ id: "g3", title: "Hades · 50 achievements", detail: "", frac: 0, plusOne: true }],
      season: { label: "Oct to Dec 2026", done: 1, trophies: [{ ...ELDEN, id: "g4", doneOn: "Nov 2" }], badges: [] },
    });
    api.goalAct.mockResolvedValue({ board: EMPTY });
    render(<Goals />);
    expect(await screen.findByText("Pick up Elden Ring tonight?")).toBeInTheDocument();
    expect(screen.getByText("1 goal done this season")).toBeInTheDocument();
    const shelf = screen.getByRole("list", { name: "Trophies" });
    expect(shelf).toHaveTextContent("Elden Ring · Finish · Nov 2");
    const tray = screen.getByRole("list", { name: "Suggested" });
    expect(tray).toHaveTextContent("Track goal: Hades · 50 achievements?");
    await userEvent.click(within(tray).getByRole("button", { name: "Not now" }));
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "notNow", id: "g3" }]);
  });

  it("shows Monday's week, its Track, and the season badge, line and bonus", async () => {
    api.getGoals.mockResolvedValue({
      ...EMPTY,
      week: { lines: ["Reached level 5 · Regular", "Goal done: Hades · Finish"], suggestion: { ...FISHING, id: "g7" } },
      season: {
        label: "Oct to Dec 2026",
        done: 1,
        trophies: [],
        badges: ["Jul to Sep 2026"],
        badgeLine: "2 more goals for the Oct to Dec 2026 badge",
        bonus: "First goal this season: +50 bonus XP",
      },
    });
    api.goalAct.mockResolvedValue({ board: EMPTY });
    render(<Goals />);
    const week = await screen.findByLabelText(YOUR_WEEK);
    expect(within(week).getByText("Reached level 5 · Regular")).toBeInTheDocument();
    expect(screen.queryByText(TODAY_EMPTY)).not.toBeInTheDocument();
    expect(screen.getByText("2 more goals for the Oct to Dec 2026 badge")).toBeInTheDocument();
    expect(screen.getByText("First goal this season: +50 bonus XP")).toBeInTheDocument();
    expect(screen.getByText("Jul to Sep 2026 badge")).toBeInTheDocument();
    await userEvent.click(within(week).getByRole("button", { name: "Track" }));
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "track", id: "g7" }]);
  });

  it("counts goals in plain words", () => {
    expect(doneLine(0)).toBe("0 goals done this season");
    expect(doneLine(1)).toBe("1 goal done this season");
  });

  it("Not now under a reply hides the goal and closes the ask", async () => {
    api.goalAct.mockResolvedValue({ board: EMPTY });
    const done = vi.fn();
    render(<GoalAsk id="g2" title={ELDEN.title} onDone={done} />);
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(api.goalAct).toHaveBeenCalledWith([{ act: "notNow", id: "g2" }]);
    expect(done).toHaveBeenCalled();
  });
});
