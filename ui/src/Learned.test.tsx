import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { LearnedAct, LearnedRow, LearnedTrust, LearnedView, TrustRow } from "./api";

const api = vi.hoisted(() => ({
  getLearned: vi.fn<() => Promise<LearnedView>>(),
  learnedAct: vi.fn<(act: LearnedAct) => Promise<LearnedView>>(),
  learnedSay: vi.fn<(line: string) => Promise<string>>(),
}));

vi.mock("./api", () => api);

import { ASK_NO, ASK_YES, DECISIONS_EMPTY, IMPORT_WARN, LEARNED_EMPTY, LEARNED_PAUSED, Learned, TRUST_ASKING, TRUST_RESTING, scoreLine, waitingLine } from "./Learned";

function row(over: Partial<LearnedRow> = {}): LearnedRow {
  return {
    key: "card:suggestion",
    title: "Suggestion cards",
    says: "You take these about 8 times in 10.",
    evidence: 10,
    sure: 54,
    stage: "suggesting",
    lastMs: 1_800_000_000_000,
    ...over,
  };
}

function dial(area: TrustRow["area"], label: string, to: TrustRow["dial"], over: Partial<TrustRow> = {}): TrustRow {
  return { area, label, dial: to, went: null, before: null, trend: "too_early", resting: false, ...over };
}

function trust(over: Partial<LearnedTrust> = {}): LearnedTrust {
  return {
    dials: [
      dial("settings", "Settings", "act"),
      dial("router", "Model choice", "watch"),
    ],
    goals: [],
    now: { quiet: false, busy: false, halted: false, gaming: false, onBattery: false },
    askingFirst: false,
    leftToday: 5,
    asks: [],
    decisions: [],
    ...over,
  };
}

function line(name: string) {
  const list = within(screen.getByRole("list", { name: "Learned" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Learned", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("says so when nothing is learned, with no Forget everything", async () => {
    api.getLearned.mockResolvedValue({ rows: [], singleChips: 0, paused: false, trust: trust() });
    render(<Learned />);
    expect(await screen.findByText(LEARNED_EMPTY)).toBeVisible();
    expect(screen.queryByRole("button", { name: "Forget everything…" })).toBeNull();
  });

  it("lists each belief with what it says and how sure it is", async () => {
    api.getLearned.mockResolvedValue({
      rows: [row(), row({ key: "model:grok-code/run-ms", title: "How long grok-code takes", says: "A run takes about 3.0 seconds.", evidence: 1, sure: 27, stage: "measuring" })],
      singleChips: 4,
      paused: false,
      trust: trust(),
    });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    expect(line("Suggestion cards").getByText("You take these about 8 times in 10.")).toBeVisible();
    expect(line("Suggestion cards").getByText("Seen 10 times · 54% sure · Sure enough to suggest")).toBeVisible();
    expect(line("How long grok-code takes").getByText("Seen once · 27% sure · A measurement")).toBeVisible();
    expect(screen.getByText("Plus counts for 4 single quick chips, which rank the chip rows.")).toBeVisible();
  });

  it("forgets one line and shows the list that comes back", async () => {
    api.getLearned.mockResolvedValue({ rows: [row(), row({ key: "skill:tidy", title: "Skill tidy" })], singleChips: 0, paused: false, trust: trust() });
    api.learnedAct.mockResolvedValue({ rows: [row({ key: "skill:tidy", title: "Skill tidy" })], singleChips: 0, paused: false, trust: trust() });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(line("Suggestion cards").getByRole("button", { name: "Forget" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "forget", key: "card:suggestion" });
    expect(screen.getByRole("status")).toHaveTextContent("Forgot Suggestion cards.");
    expect(screen.queryByRole("listitem", { name: "Suggestion cards" })).toBeNull();
    expect(line("Skill tidy").getByRole("button", { name: "Forget" })).toBeVisible();
  });

  it("marks a line wrong, which leaves it listed without buttons", async () => {
    api.getLearned.mockResolvedValue({ rows: [row()], singleChips: 0, paused: false, trust: trust() });
    api.learnedAct.mockResolvedValue({
      rows: [row({ says: "You said this is wrong. It is not counted or used.", evidence: 0, sure: 0, stage: "wrong" })],
      singleChips: 0,
      paused: false,
      trust: trust(),
    });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(line("Suggestion cards").getByRole("button", { name: "That's wrong" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "wrong", key: "card:suggestion" });
    expect(line("Suggestion cards").getByText("Marked wrong")).toBeVisible();
    expect(line("Suggestion cards").queryByRole("button")).toBeNull();
  });

  it("asks before forgetting everything, and Cancel changes nothing", async () => {
    api.getLearned.mockResolvedValue({ rows: [row()], singleChips: 0, paused: false, trust: trust() });
    api.learnedAct.mockResolvedValue({ rows: [], singleChips: 0, paused: false, trust: trust() });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    await userEvent.click(screen.getByRole("button", { name: "Forget everything…" }));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.learnedAct).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Forget everything…" }));
    await userEvent.click(screen.getByRole("button", { name: "Forget everything" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "forgetAll" });
    expect(screen.getByRole("status")).toHaveTextContent("Forgot everything.");
    expect(screen.getByText(LEARNED_EMPTY)).toBeVisible();
  });

  it("makes a line the user's rule, which drops Always and the numbers", async () => {
    api.getLearned.mockResolvedValue({ rows: [row(), row({ key: "model:grok-code/run-ms", title: "How long grok-code takes", stage: "measuring" })], singleChips: 0, paused: false, trust: trust() });
    api.learnedAct.mockResolvedValue({
      rows: [row({ says: "You said this is always so. Nothing counts against it.", evidence: 100, sure: 100, stage: "pinned" })],
      singleChips: 0,
      paused: false,
      trust: trust(),
    });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    expect(line("How long grok-code takes").queryByRole("button", { name: "Always" })).toBeNull();
    await userEvent.click(line("Suggestion cards").getByRole("button", { name: "Always" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "always", key: "card:suggestion" });
    expect(screen.getByRole("status")).toHaveTextContent("Suggestion cards is your rule now.");
    expect(line("Suggestion cards").getByText("You said always")).toBeVisible();
    expect(line("Suggestion cards").queryByRole("button", { name: "Always" })).toBeNull();
    expect(line("Suggestion cards").getByRole("button", { name: "Forget" })).toBeVisible();
  });

  it("pauses and resumes learning", async () => {
    api.getLearned.mockResolvedValue({ rows: [row()], singleChips: 0, paused: false, trust: trust() });
    api.learnedAct.mockResolvedValueOnce({ rows: [row()], singleChips: 0, paused: true, trust: trust() }).mockResolvedValueOnce({ rows: [row()], singleChips: 0, paused: false, trust: trust() });
    render(<Learned />);
    await screen.findByRole("list", { name: "Learned" });
    expect(screen.queryByText(LEARNED_PAUSED)).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Pause learning" }));
    expect(api.learnedAct).toHaveBeenLastCalledWith({ kind: "pause" });
    expect(screen.getByText(LEARNED_PAUSED)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Resume learning" }));
    expect(api.learnedAct).toHaveBeenLastCalledWith({ kind: "resume" });
    expect(screen.queryByText(LEARNED_PAUSED)).toBeNull();
    expect(screen.getByRole("button", { name: "Pause learning" })).toBeVisible();
  });

  it("sets how far an area may go and lists what it decided, newest first", async () => {
    const view = { rows: [], singleChips: 0, paused: false };
    api.getLearned.mockResolvedValue({ ...view, trust: trust() });
    api.learnedAct.mockResolvedValue({
      ...view,
      trust: trust({
        dials: [
          dial("settings", "Settings", "act"),
          dial("router", "Model choice", "offer", { went: 53, before: 90, trend: "worse", resting: true }),
        ],
        leftToday: 1,
        decisions: [
          { atMs: 1_800_000_060_000, title: "Skill tidy in the morning", did: "Did it", why: "It was sure, you said yes to this before, and it can be undone." },
          { atMs: 1_800_000_000_000, title: "Settings text size", did: "Asked", why: "This area is set to offer only." },
        ],
      }),
    });
    render(<Learned />);
    expect(await screen.findByText(DECISIONS_EMPTY)).toBeVisible();
    expect(screen.getByLabelText("Settings")).toHaveValue("act");
    expect(screen.getByLabelText("Model choice")).toHaveValue("watch");
    expect(screen.getByText("It may make 5 more changes on its own today.")).toBeVisible();
    expect(screen.queryByText(TRUST_ASKING)).toBeNull();
    await userEvent.selectOptions(screen.getByLabelText("Model choice"), "Offer only");
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "trust", area: "router", dial: "offer" });
    expect(screen.getByRole("status")).toHaveTextContent("Model choice is set to offer only.");
    expect(screen.getByLabelText("Model choice")).toHaveValue("offer");
    expect(screen.getByText("It may make 1 more change on its own today.")).toBeVisible();
    expect(screen.getByText("Model choice: 53% of what it offered or changed here went your way in the last four weeks, down from 90%. It asks first here for a month.")).toBeVisible();
    expect(screen.getByText(TRUST_RESTING)).toBeVisible();
    expect(screen.getByRole("button", { name: "Carry on" })).toBeVisible();
    const lines = within(screen.getByRole("list", { name: "Decisions" })).getAllByRole("listitem");
    expect(lines.map((li) => li.getAttribute("aria-label"))).toEqual(["Did it: Skill tidy in the morning", "Asked: Settings text size"]);
    expect(within(lines[1]).getByText("This area is set to offer only.")).toBeVisible();
  });

  it("words an area's score by its trend", () => {
    expect(scoreLine(dial("chips", "Quick chips", "act"))).toBeNull();
    expect(scoreLine(dial("chips", "Quick chips", "act", { went: 80 }))).toBe("80% of what it offered or changed here went your way in the last four weeks.");
    expect(scoreLine(dial("chips", "Quick chips", "act", { went: 80, before: 60, trend: "better" }))).toBe("80% of what it offered or changed here went your way in the last four weeks, up from 60%.");
    expect(scoreLine(dial("chips", "Quick chips", "act", { went: 80, before: 78, trend: "steady" }))).toBe("80% of what it offered or changed here went your way in the last four weeks.");
  });

  it("sets what matters most, keeping the other goals after it", async () => {
    const view = { rows: [], singleChips: 0, paused: false };
    const goals = [
      { goal: "cheapest" as const, label: "Lowest cost" },
      { goal: "quiet" as const, label: "Fewest interruptions" },
    ];
    api.getLearned.mockResolvedValue({ ...view, trust: trust({ goals }) });
    api.learnedAct.mockResolvedValue({ ...view, trust: trust({ goals: [goals[1], goals[0]] }) });
    render(<Learned />);
    expect(await screen.findByLabelText("What matters most")).toHaveValue("cheapest");
    await userEvent.selectOptions(screen.getByLabelText("What matters most"), "Fewest interruptions");
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "goals", goals: ["quiet", "cheapest"] });
    expect(screen.getByRole("status")).toHaveTextContent("Fewest interruptions comes first now.");
    expect(screen.getByLabelText("What matters most")).toHaveValue("quiet");
    await userEvent.selectOptions(screen.getByLabelText("What matters most"), "Nothing set");
    expect(api.learnedAct).toHaveBeenLastCalledWith({ kind: "goals", goals: [] });
  });

  it("says why changes wait right now", () => {
    const clear = trust().now;
    expect(waitingLine(clear)).toBeNull();
    expect(waitingLine({ ...clear, onBattery: true })).toBeNull();
    expect(waitingLine({ ...clear, gaming: true, quiet: true })).toBe("A game is running, so changes wait.");
    expect(waitingLine({ ...clear, quiet: true })).toBe("It's quiet hours, so changes wait.");
    expect(waitingLine({ ...clear, busy: true })).toBe("You're in the middle of something, so changes wait.");
    expect(waitingLine({ ...clear, halted: true, gaming: true })).toBe("Halt is on, so changes wait.");
  });

  it("offers Carry on while everything asks first", async () => {
    const view = { rows: [], singleChips: 0, paused: false };
    api.getLearned.mockResolvedValue({ ...view, trust: trust({ askingFirst: true, leftToday: 0 }) });
    api.learnedAct.mockResolvedValue({ ...view, trust: trust() });
    render(<Learned />);
    expect(await screen.findByText(TRUST_ASKING)).toBeVisible();
    expect(screen.getByText("It has made all its changes for today, so it asks for the rest.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Carry on" }));
    expect(api.learnedAct).toHaveBeenCalledWith({ kind: "carryOn" });
    expect(screen.queryByText(TRUST_ASKING)).toBeNull();
    expect(screen.queryByRole("button", { name: "Carry on" })).toBeNull();
  });

  it("saves a backup and shows the fault report without reading the list again", async () => {
    api.getLearned.mockResolvedValue({ rows: [], singleChips: 0, paused: false, trust: trust() });
    api.learnedSay.mockResolvedValueOnce("Saved a backup to learned-backup.json.").mockResolvedValueOnce("Nothing to report.");
    render(<Learned />);
    await userEvent.click(await screen.findByRole("button", { name: "Save a backup" }));
    expect(api.learnedSay).toHaveBeenLastCalledWith("backup");
    expect(screen.getByRole("status", { name: "Backup and report" })).toHaveTextContent("Saved a backup to learned-backup.json.");
    await userEvent.click(screen.getByRole("button", { name: "Show the fault report" }));
    expect(api.learnedSay).toHaveBeenLastCalledWith("report");
    expect(screen.getByRole("status", { name: "Backup and report" })).toHaveTextContent("Nothing to report.");
    expect(api.getLearned).toHaveBeenCalledTimes(1);
  });

  it("asks before taking in a backup, then shows the list that came with it", async () => {
    api.getLearned
      .mockResolvedValueOnce({ rows: [], singleChips: 0, paused: false, trust: trust() })
      .mockResolvedValueOnce({ rows: [row({ key: "skill:tidy", title: "Skill tidy" })], singleChips: 0, paused: false, trust: trust() });
    api.learnedSay.mockResolvedValue("Took in the backup.");
    render(<Learned />);
    await userEvent.click(await screen.findByRole("button", { name: "Take in a backup…" }));
    expect(api.learnedSay).not.toHaveBeenCalled();
    expect(screen.getByText(IMPORT_WARN)).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Take in the backup" }));
    expect(api.learnedSay).toHaveBeenCalledWith("import");
    expect(await screen.findByRole("listitem", { name: "Skill tidy" })).toBeVisible();
    expect(screen.queryByText(IMPORT_WARN)).toBeNull();
    expect(screen.getByRole("status", { name: "Backup and report" })).toHaveTextContent("Took in the backup.");
  });

  it("shows why a put-back could not run", async () => {
    api.getLearned.mockResolvedValue({ rows: [], singleChips: 0, paused: false, trust: trust() });
    api.learnedSay.mockRejectedValue("no reach");
    render(<Learned />);
    await userEvent.click(await screen.findByRole("button", { name: "Put back what was here" }));
    expect(api.learnedSay).toHaveBeenCalledWith("restore");
    expect(screen.getByRole("status", { name: "Backup and report" })).toHaveTextContent("no reach");
  });

  it("answers a waiting model-choice change with a yes or a no", async () => {
    const ask = { key: "rule:router/chat:default/order", title: "Router: which model goes first for chat:default", why: "This area is set to offer only." };
    api.getLearned.mockResolvedValue({ rows: [], singleChips: 0, paused: false, trust: trust({ asks: [ask] }) });
    api.learnedAct.mockResolvedValueOnce({ rows: [], singleChips: 0, paused: false, trust: trust({ asks: [ask] }) }).mockResolvedValueOnce({ rows: [], singleChips: 0, paused: false, trust: trust() });
    render(<Learned />);
    const waiting = () => within(within(screen.getByRole("list", { name: "Waiting for your yes" })).getByRole("listitem", { name: ask.title }));
    await screen.findByRole("list", { name: "Waiting for your yes" });
    expect(waiting().getByText("This area is set to offer only.")).toBeVisible();
    await userEvent.click(waiting().getByRole("button", { name: "Yes" }));
    expect(api.learnedAct).toHaveBeenLastCalledWith({ kind: "answer", key: ask.key, yes: true });
    expect(screen.getByRole("status")).toHaveTextContent(ASK_YES);
    await userEvent.click(waiting().getByRole("button", { name: "No" }));
    expect(api.learnedAct).toHaveBeenLastCalledWith({ kind: "answer", key: ask.key, yes: false });
    expect(screen.getByRole("status")).toHaveTextContent(ASK_NO);
    expect(screen.queryByRole("list", { name: "Waiting for your yes" })).toBeNull();
  });

  it("shows why when the list can't load", async () => {
    api.getLearned.mockRejectedValue("The learning files can't be read.");
    render(<Learned />);
    expect(await screen.findByRole("status")).toHaveTextContent("The learning files can't be read.");
  });
});
