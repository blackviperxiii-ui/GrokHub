import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AgentProfile, AgentTask, AgentTeam, PairedComputer, TeamPage } from "./teamApi";

const api = vi.hoisted(() => ({
  getTeam: vi.fn<() => Promise<TeamPage>>(),
  pairedComputers: vi.fn<() => Promise<PairedComputer[]>>(),
  saveAgent: vi.fn<(agent: AgentProfile, add: boolean) => Promise<TeamPage>>(),
  removeAgent: vi.fn<(id: string) => Promise<TeamPage>>(),
  saveTeam: vi.fn<(team: AgentTeam, add: boolean) => Promise<TeamPage>>(),
  removeTeam: vi.fn<(id: string) => Promise<TeamPage>>(),
  getAgentTasks: vi.fn<() => Promise<AgentTask[]>>(),
  clearAgentTasks: vi.fn<() => Promise<void>>(),
}));

vi.mock("./teamApi", () => api);

import {
  AGENTS_EMPTY,
  GONE_COMPUTER,
  idFor,
  KEPT_FOLDER,
  NO_COMPUTERS,
  SHARE_LEGEND,
  SHARE_PAUSED,
  Team,
  TASKS_EMPTY,
} from "./Team";

function agent(over: Partial<AgentProfile> = {}): AgentProfile {
  return {
    id: "writer",
    name: "Writer",
    kind: "role",
    access: "ask",
    about: "Drafts posts.\nShort ones.",
    desktop: false,
    share: false,
    ...over,
  };
}

const PAGE: TeamPage = {
  agents: [
    agent({ team: "desk", talks_to: ["planner"] }),
    agent({ id: "planner", name: "Planner", team: "desk" }),
  ],
  teams: [{ id: "desk", name: "Desk", lead: "planner" }],
  problem: null,
  tiers: ["workhorse"],
};

const EMPTY: TeamPage = { agents: [], teams: [], problem: null, tiers: [] };

async function card(name: string) {
  return within(await screen.findByRole("listitem", { name }));
}

beforeEach(() => {
  vi.clearAllMocks();
  api.getTeam.mockResolvedValue(PAGE);
  api.getAgentTasks.mockResolvedValue([]);
  api.pairedComputers.mockResolvedValue([]);
});

const LAPTOP: PairedComputer = { id: "laptop-1", name: "Laptop", lastSeen: 0 };
const TABLET: PairedComputer = { id: "tablet-2", name: "Tablet", lastSeen: 0 };
const LAPTOP_ROW = "Laptop · Paired, not used yet";
const TABLET_ROW = "Tablet · Paired, not used yet";

async function editWriter() {
  await userEvent.click((await card("Writer")).getByRole("button", { name: "Edit" }));
  return within(screen.getByRole("form", { name: "Edit Writer" }));
}

describe("sharing an agent with paired computers", () => {
  it("lists the paired computers and saves the ones ticked", async () => {
    api.pairedComputers.mockResolvedValue([LAPTOP, TABLET]);
    const shared = { ...PAGE.agents[0], share: true, share_with: ["laptop-1"] };
    api.saveAgent.mockResolvedValue({ ...PAGE, agents: [shared, PAGE.agents[1]] });
    render(<Team />);
    const form = await editWriter();
    expect(await form.findByRole("group", { name: SHARE_LEGEND })).toBeInTheDocument();
    expect(form.getByLabelText(TABLET_ROW)).not.toBeChecked();
    await userEvent.click(form.getByLabelText(LAPTOP_ROW));
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith(shared, false);
    expect(
      (await card("Writer")).getByText("Role · Ask first · Desk · shared with 1 computer"),
    ).toBeInTheDocument();
  });

  it("stops sharing when the last computer is unticked", async () => {
    const shared = agent({ share: true, share_with: ["laptop-1", "tablet-2"] });
    api.getTeam.mockResolvedValue({ ...PAGE, agents: [shared], teams: [] });
    api.pairedComputers.mockResolvedValue([LAPTOP, TABLET]);
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    expect((await card("Writer")).getByText("Role · Ask first · shared with 2 computers")).toBeInTheDocument();
    const form = await editWriter();
    await userEvent.click(await form.findByLabelText(LAPTOP_ROW));
    await userEvent.click(form.getByLabelText(TABLET_ROW));
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith({ ...shared, share: false, share_with: [] }, false);
  });

  it("keeps a computer that's no longer paired ticked until the person clears it", async () => {
    const shared = agent({ share: true, share_with: ["old-pc", "laptop-1"] });
    const page = { ...PAGE, agents: [shared], teams: [] };
    api.getTeam.mockResolvedValue(page);
    api.pairedComputers.mockResolvedValue([LAPTOP]);
    api.saveAgent.mockResolvedValue(page);
    render(<Team />);
    let form = await editWriter();
    expect(await form.findByLabelText(`${GONE_COMPUTER} · old-pc`)).toBeChecked();
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenLastCalledWith(shared, false);
    form = await editWriter();
    await userEvent.click(await form.findByLabelText(`${GONE_COMPUTER} · old-pc`));
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenLastCalledWith({ ...shared, share_with: ["laptop-1"] }, false);
  });

  it("tells two computers with one name apart by when they last called and their ids", async () => {
    const old = { id: "laptop-old-0001", name: "Laptop", lastSeen: 0 };
    const fresh = { id: "laptop-new-0002", name: "Laptop", lastSeen: Date.now() - 5 * 60_000 };
    api.pairedComputers.mockResolvedValue([old, fresh, TABLET]);
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    const form = await editWriter();
    expect(await form.findByLabelText("Laptop · Paired, not used yet · …d-0001")).not.toBeChecked();
    expect(form.getByLabelText(TABLET_ROW)).not.toBeChecked();
    await userEvent.click(form.getByLabelText("Laptop · Last used 5 min ago · …w-0002"));
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith(
      { ...PAGE.agents[0], share: true, share_with: ["laptop-new-0002"] },
      false,
    );
  });

  it("says when sharing is paused in the file, and a tick shares with just that computer", async () => {
    const held = agent({ share: false, share_with: ["laptop-1", "tablet-2"] });
    api.getTeam.mockResolvedValue({ ...PAGE, agents: [held], teams: [] });
    api.pairedComputers.mockResolvedValue([LAPTOP, TABLET]);
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    expect((await card("Writer")).getByText("Role · Ask first · sharing paused")).toBeInTheDocument();
    const form = await editWriter();
    expect(await form.findByText(SHARE_PAUSED)).toBeInTheDocument();
    expect(form.getByLabelText(LAPTOP_ROW)).not.toBeChecked();
    expect(form.getByLabelText(TABLET_ROW)).not.toBeChecked();
    await userEvent.click(form.getByLabelText(LAPTOP_ROW));
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith({ ...held, share: true, share_with: ["laptop-1"] }, false);
  });

  it("says where to pair a computer when none is paired", async () => {
    render(<Team />);
    const form = await editWriter();
    expect(await form.findByText(NO_COMPUTERS)).toBeInTheDocument();
  });

  it("hides the list where there's no hub to share from, and leaves sharing as it was", async () => {
    const shared = agent({ share: true, share_with: ["laptop-1"] });
    api.getTeam.mockResolvedValue({ ...PAGE, agents: [shared], teams: [] });
    api.pairedComputers.mockRejectedValue("unknown command team_computers");
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    const form = await editWriter();
    await userEvent.click(form.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith(shared, false);
    expect(form.queryByRole("group", { name: SHARE_LEGEND })).toBeNull();
    expect(screen.queryByText("unknown command team_computers")).toBeNull();
  });
});

describe("idFor", () => {
  it("makes a file id from a name, unique against the ids in use", () => {
    expect(idFor("Café Bot!", [])).toBe("cafe-bot");
    expect(idFor("Café Bot!", ["cafe-bot", "cafe-bot-2"])).toBe("cafe-bot-3");
    expect(idFor("✨", [])).toBe("agent");
    expect(idFor(`${"a".repeat(35)} b`, [])).toBe("a".repeat(35));
  });
});

describe("Team", () => {
  it("shows each agent's kind, access, team and who it talks to", async () => {
    render(<Team />);
    const writer = await card("Writer");
    expect(writer.getByText("Role · Ask first · Desk")).toBeInTheDocument();
    expect(writer.getByText("Drafts posts.")).toBeInTheDocument();
    expect(writer.getByText("Talks to Planner")).toBeInTheDocument();
    expect((await card("Desk")).getByText("Led by Planner · 2 on it")).toBeInTheDocument();
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
  });

  it("makes an agent from a kind with that kind's defaults and an id from its name", async () => {
    api.getTeam.mockResolvedValue({ ...PAGE, agents: [agent()], teams: [] });
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    await userEvent.click(await screen.findByRole("button", { name: "New role" }));
    const form = within(screen.getByRole("form", { name: "New role" }));
    await userEvent.type(form.getByLabelText("Name"), " House researcher ");
    await userEvent.type(form.getByLabelText("How it works and what it knows"), "Finds sources.");
    await userEvent.click(form.getByLabelText("Writer"));
    await userEvent.selectOptions(form.getByLabelText("Models"), "workhorse");
    await userEvent.click(form.getByRole("button", { name: "Make role" }));
    expect(api.saveAgent).toHaveBeenCalledWith(
      {
        id: "house-researcher",
        name: "House researcher",
        kind: "role",
        access: "ask",
        about: "Finds sources.",
        desktop: false,
        share: false,
        talks_to: ["writer"],
        model_tier: "workhorse",
      },
      true,
    );
    expect(await screen.findByText("Made House researcher.")).toBeInTheDocument();
    expect(screen.queryByRole("form", { name: "New role" })).toBeNull();
  });

  it("keeps a customer read only, off the desktop and without anyone to message", async () => {
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    // A form opened for another kind gives way to the kind clicked last.
    await userEvent.click(await screen.findByRole("button", { name: "New staff" }));
    await userEvent.click(screen.getByRole("button", { name: "New customer" }));
    expect(screen.queryByRole("form", { name: "New staff" })).toBeNull();
    const form = within(screen.getByRole("form", { name: "New customer" }));
    expect(form.getByLabelText("Access")).toBeDisabled();
    expect(form.getByLabelText("Access")).toHaveValue("read_only");
    expect(form.queryByText("Talks to")).toBeNull();
    expect(form.queryByLabelText(/desktop/)).toBeNull();
    await userEvent.type(form.getByLabelText("Name"), "Front desk");
    await userEvent.click(form.getByRole("button", { name: "Make customer" }));
    expect(api.saveAgent).toHaveBeenCalledWith(
      {
        id: "front-desk",
        name: "Front desk",
        kind: "customer",
        access: "read_only",
        about: "",
        desktop: false,
        share: false,
      },
      true,
    );
  });

  it("doesn't send back an agent or team removed while the form was open", async () => {
    api.removeAgent.mockResolvedValue({ ...PAGE, agents: [PAGE.agents[0]], teams: [] });
    api.saveAgent.mockResolvedValue(PAGE);
    render(<Team />);
    await userEvent.click((await card("Writer")).getByRole("button", { name: "Edit" }));
    await userEvent.click((await card("Planner")).getByRole("button", { name: "Remove" }));
    const ask = within(screen.getByRole("group", { name: "Remove Planner?" }));
    await userEvent.click(ask.getByRole("button", { name: "Remove" }));
    expect(await screen.findByText("Removed Planner.")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith({ ...PAGE.agents[0], talks_to: [], team: "" }, false);
  });

  it("asks before removing an agent and says its folder stays", async () => {
    api.removeAgent.mockResolvedValue({ ...PAGE, agents: [PAGE.agents[1]] });
    render(<Team />);
    await userEvent.click((await card("Writer")).getByRole("button", { name: "Remove" }));
    const ask = within(screen.getByRole("group", { name: "Remove Writer?" }));
    expect(ask.getByText(KEPT_FOLDER, { exact: false })).toBeInTheDocument();
    await userEvent.click(ask.getByRole("button", { name: "Remove" }));
    expect(api.removeAgent).toHaveBeenCalledWith("writer");
    expect(await screen.findByText("Removed Writer.")).toBeInTheDocument();
    expect(screen.queryByRole("listitem", { name: "Writer" })).toBeNull();
  });

  it("shows a save the file refuses, and what a hand edit broke", async () => {
    api.getTeam.mockResolvedValue({ ...PAGE, problem: "`a` is on team `gone`, which doesn't exist." });
    api.saveAgent.mockRejectedValue("`writer` talks to `x`, which isn't an agent.");
    render(<Team />);
    expect(
      await screen.findByText("The team file breaks a rule: `a` is on team `gone`, which doesn't exist."),
    ).toBeInTheDocument();
    await userEvent.click((await card("Writer")).getByRole("button", { name: "Edit" }));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.saveAgent).toHaveBeenCalledWith(PAGE.agents[0], false);
    expect(await screen.findByText("`writer` talks to `x`, which isn't an agent.")).toBeInTheDocument();
    expect(screen.getByRole("form", { name: "Edit Writer" })).toBeInTheDocument();
  });

  it("makes a team led by the agent picked", async () => {
    api.getTeam.mockResolvedValue({ ...PAGE, teams: [], agents: [agent(), agent({ id: "planner", name: "Planner" })] });
    api.saveTeam.mockResolvedValue(PAGE);
    render(<Team />);
    await userEvent.click(await screen.findByRole("button", { name: "New team" }));
    const form = within(screen.getByRole("form", { name: "New team" }));
    await userEvent.type(form.getByLabelText("Name"), "Front desk");
    await userEvent.selectOptions(form.getByLabelText("Lead"), "planner");
    await userEvent.click(form.getByRole("button", { name: "Make team" }));
    expect(api.saveTeam).toHaveBeenCalledWith({ id: "front-desk", name: "Front desk", lead: "planner" }, true);
  });

  it("offers only agents who lead no other team as a lead", async () => {
    render(<Team />);
    await userEvent.click(await screen.findByRole("button", { name: "New team" }));
    const form = within(screen.getByRole("form", { name: "New team" }));
    const leads = within(form.getByLabelText("Lead")).getAllByRole("option");
    expect(leads.map((o) => o.textContent)).toEqual(["Writer"]);
    expect(form.getByLabelText("Lead")).toHaveValue("writer");
    await userEvent.click(form.getByRole("button", { name: "Cancel" }));
    await userEvent.click((await card("Desk")).getByRole("button", { name: "Edit" }));
    const edit = within(screen.getByRole("form", { name: "Edit Desk" }));
    const both = within(edit.getByLabelText("Lead")).getAllByRole("option");
    expect(both.map((o) => o.textContent)).toEqual(["Writer", "Planner"]);
  });

  it("can't start a team when every agent already leads one", async () => {
    api.getTeam.mockResolvedValue({
      ...PAGE,
      agents: [PAGE.agents[1]],
    });
    render(<Team />);
    const add = await screen.findByRole("button", { name: "New team" });
    expect(add).toBeDisabled();
    expect(add).toHaveAttribute("title", "Every agent already leads a team");
  });

  it("lists who asked which agent what, newest first, and clears the log", async () => {
    api.getAgentTasks.mockResolvedValue([
      {
        id: "task-3",
        from: { peer: "d-1a2b" },
        to: "writer",
        state: "TASK_STATE_COMPLETED",
        asked: "When are you open?",
        reply: "9 to 5.",
        started_ms: 3,
        ended_ms: 4,
      },
      {
        id: "task-2",
        from: { agent: "planner" },
        to: "writer",
        state: "TASK_STATE_REJECTED",
        asked: "Draft the launch post.",
        reply: "too many agents are working",
        started_ms: 2,
        ended_ms: 3,
      },
      {
        id: "task-1",
        from: "person",
        to: "writer",
        state: "TASK_STATE_COMPLETED",
        asked: "Which router has the best range?",
        reply: "The mesh one.",
        started_ms: 1,
        ended_ms: 2,
      },
    ]);
    api.clearAgentTasks.mockResolvedValue();
    render(<Team />);
    const log = within(await screen.findByRole("list", { name: "Task log" }));
    const rows = log.getAllByRole("listitem").map((li) => li.getAttribute("aria-label"));
    expect(rows).toEqual([
      "A paired computer asked Writer",
      "Planner asked Writer",
      "You asked Writer",
    ]);
    const refused = within(log.getByRole("listitem", { name: "Planner asked Writer" }));
    expect(refused.getByText(/^Planner asked Writer · Refused · /)).toBeInTheDocument();
    expect(refused.getByText("Why")).toBeInTheDocument();
    const answered = within(log.getByRole("listitem", { name: "You asked Writer" }));
    expect(answered.getByText(/^You asked Writer · Answered · /)).toBeInTheDocument();
    expect(answered.getByText("The mesh one.")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Clear log" }));
    await userEvent.click(screen.getByRole("button", { name: "Clear" }));
    expect(api.clearAgentTasks).toHaveBeenCalledTimes(1);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
  });

  it("reads the log only while the page is shown, and says so when there are no agents", async () => {
    api.getTeam.mockResolvedValue(EMPTY);
    const { rerender } = render(<Team shown={false} />);
    expect(await screen.findByText(AGENTS_EMPTY)).toBeInTheDocument();
    expect(api.getAgentTasks).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "New team" })).toBeDisabled();
    rerender(<Team shown />);
    expect(await screen.findByText(TASKS_EMPTY)).toBeInTheDocument();
    expect(api.getAgentTasks).toHaveBeenCalledTimes(1);
  });
});
