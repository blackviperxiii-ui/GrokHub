import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRow, CodeProjects as ProjectList } from "./api";

const api = vi.hoisted(() => ({
  codeProjects: vi.fn<() => Promise<ProjectList>>(),
  codeProjectOpen: vi.fn<(path: string) => Promise<ProjectList>>(),
  codeProjectPick: vi.fn<() => Promise<ProjectList | null>>(),
  codeProjectNew: vi.fn<(name: string) => Promise<ProjectList>>(),
  listChats: vi.fn<() => Promise<ChatRow[]>>(),
}));

vi.mock("./api", () => api);
vi.mock("./History", () => ({ whenLabel: (updated: number) => `at ${updated}` }));

import {
  CodeProjects,
  folderName,
  projectSessions,
  PROJECTS_AGENT_BUSY,
  PROJECTS_EMPTY,
  PROJECTS_MISSING,
  PROJECTS_NONE_OPEN,
  PROJECTS_NOT_LISTED,
  SESSIONS_EMPTY,
} from "./CodeProjects";

const harbor = { id: "p1", name: "Harbor", path: "/work/harbor", current: true, missing: false };
const dawn = { id: "p2", name: "Dawn", path: "/work/dawn", current: false, missing: false };
const lost = { id: "p3", name: "Lost", path: "/work/lost", current: false, missing: true };

function onHarbor(): ProjectList {
  return { bound: "/work/harbor", projects: [harbor, dawn, lost] };
}

function onDawn(): ProjectList {
  return { bound: "/work/dawn", projects: [{ ...harbor, current: false }, { ...dawn, current: true }, lost] };
}

function chat(id: string, project: string | null, updated: number): ChatRow {
  return { id, title: `Chat ${id}`, created: 1, updated, project };
}

function show() {
  const calls = { onSwitched: vi.fn(), onNewSession: vi.fn(), onOpenSession: vi.fn() };
  render(<CodeProjects {...calls} />);
  return calls;
}

describe("CodeProjects", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.codeProjects.mockResolvedValue(onHarbor());
    api.listChats.mockResolvedValue([chat("a", "p1", 10), chat("b", "p2", 30), chat("c", "p1", 20), chat("d", null, 40)]);
  });

  it("names the open project and lists only its sessions, newest first", async () => {
    const calls = show();
    expect(await screen.findByText("Harbor")).toBeInTheDocument();
    expect(screen.getByText("/work/harbor")).toBeInTheDocument();
    const rows = within(await screen.findByRole("list", { name: "Sessions" })).getAllByRole("button");
    expect(rows.map((r) => r.textContent)).toEqual(["Chat cat 20", "Chat aat 10"]);
    await userEvent.click(rows[0]);
    expect(calls.onOpenSession).toHaveBeenCalledWith("c");
    await userEvent.click(screen.getByRole("button", { name: "New session" }));
    expect(calls.onNewSession).toHaveBeenCalledTimes(1);
    // The project list stays folded until asked for.
    expect(screen.queryByRole("list", { name: "Projects" })).not.toBeInTheDocument();
  });

  it("switches to another project, shows its sessions and tells the tab", async () => {
    api.codeProjectOpen.mockResolvedValue(onDawn());
    const calls = show();
    await userEvent.click(await screen.findByRole("button", { name: "Switch project" }));
    const list = screen.getByRole("list", { name: "Projects" });
    expect(within(list).getByRole("button", { name: /Harbor/ })).toBeDisabled();
    expect(within(list).getByRole("button", { name: /Lost/ })).toHaveTextContent(PROJECTS_MISSING);
    await userEvent.click(within(list).getByRole("button", { name: /Dawn/ }));
    expect(api.codeProjectOpen).toHaveBeenCalledWith("/work/dawn");
    await waitFor(() => expect(calls.onSwitched).toHaveBeenCalledTimes(1));
    expect(screen.queryByRole("list", { name: "Projects" })).not.toBeInTheDocument();
    const rows = within(screen.getByRole("list", { name: "Sessions" })).getAllByRole("button");
    expect(rows.map((r) => r.textContent)).toEqual(["Chat bat 30"]);
    expect(screen.getByText("/work/dawn")).toBeInTheDocument();
  });

  it("opens a picked folder, and a closed picker changes nothing", async () => {
    api.codeProjectPick.mockResolvedValueOnce(null);
    const calls = show();
    await userEvent.click(await screen.findByRole("button", { name: "Open folder…" }));
    await waitFor(() => expect(api.codeProjectPick).toHaveBeenCalledTimes(1));
    expect(calls.onSwitched).not.toHaveBeenCalled();
    expect(screen.getByText("/work/harbor")).toBeInTheDocument();

    api.codeProjectPick.mockResolvedValueOnce(onDawn());
    await userEvent.click(screen.getByRole("button", { name: "Open folder…" }));
    expect(await screen.findByText("/work/dawn")).toBeInTheDocument();
    expect(calls.onSwitched).toHaveBeenCalledTimes(1);
  });

  it("makes a new project from a name and opens it", async () => {
    api.codeProjectNew.mockResolvedValue({
      bound: "/work/night-watch",
      projects: [{ ...harbor, current: false }, { id: "p9", name: "Night watch", path: "/work/night-watch", current: true, missing: false }],
    });
    const calls = show();
    await userEvent.click(await screen.findByRole("button", { name: "New project" }));
    expect(screen.getByRole("button", { name: "Create" })).toBeDisabled();
    await userEvent.type(screen.getByLabelText("Project name"), "  Night watch {Enter}");
    expect(api.codeProjectNew).toHaveBeenCalledWith("Night watch");
    expect(await screen.findByText("/work/night-watch")).toBeInTheDocument();
    expect(screen.queryByLabelText("Project name")).not.toBeInTheDocument();
    expect(screen.getByText(SESSIONS_EMPTY)).toBeInTheDocument();
    expect(calls.onSwitched).toHaveBeenCalledTimes(1);
  });

  it("starts on the project list when no project is open", async () => {
    api.codeProjects.mockResolvedValue({ bound: "", projects: [{ ...harbor, current: false }] });
    show();
    expect(await screen.findByText(PROJECTS_NONE_OPEN)).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "Projects" })).getByRole("button", { name: /Harbor/ })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "Switch project" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "New session" })).not.toBeInTheDocument();
  });

  it("says so when there are no projects at all", async () => {
    api.codeProjects.mockResolvedValue({ bound: "", projects: [] });
    show();
    expect(await screen.findByText(PROJECTS_EMPTY)).toBeInTheDocument();
  });

  it("offers to add an open folder that is not in the list", async () => {
    api.codeProjects.mockResolvedValue({ bound: "/tmp/scratch", projects: [{ ...harbor, current: false }] });
    api.codeProjectOpen.mockResolvedValue({
      bound: "/tmp/scratch",
      projects: [{ ...harbor, current: false }, { id: "p8", name: "scratch", path: "/tmp/scratch", current: true, missing: false }],
    });
    const calls = show();
    expect(await screen.findByText(PROJECTS_NOT_LISTED, { exact: false })).toBeInTheDocument();
    expect(screen.getByText("scratch")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add to projects" }));
    expect(api.codeProjectOpen).toHaveBeenCalledWith("/tmp/scratch");
    expect(await screen.findByRole("button", { name: "New session" })).toBeInTheDocument();
    // The folder did not change, so the rest of the tab is left as it is.
    expect(calls.onSwitched).not.toHaveBeenCalled();
  });

  it("shows why a switch failed and stays on the open project", async () => {
    api.codeProjectOpen.mockRejectedValue("The project list couldn't be read, so nothing was changed.");
    const calls = show();
    await userEvent.click(await screen.findByRole("button", { name: "Switch project" }));
    await userEvent.click(screen.getByRole("button", { name: /Dawn/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("The project list couldn't be read, so nothing was changed.");
    expect(screen.getByText("/work/harbor")).toBeInTheDocument();
    expect(calls.onSwitched).not.toHaveBeenCalled();
  });

  it("folds a long session list until Show all", async () => {
    api.listChats.mockResolvedValue(Array.from({ length: 8 }, (_, i) => chat(`s${i}`, "p1", i + 1)));
    show();
    const list = await screen.findByRole("list", { name: "Sessions" });
    expect(within(list).getAllByRole("button")).toHaveLength(6);
    await userEvent.click(screen.getByRole("button", { name: "Show all 8" }));
    expect(within(list).getAllByRole("button")).toHaveLength(8);
  });

  it("holds the project and the other sessions while the agent replies", async () => {
    const calls = { onSwitched: vi.fn(), onNewSession: vi.fn(), onOpenSession: vi.fn() };
    render(<CodeProjects {...calls} agentBusy openSession="c" />);
    const rows = within(await screen.findByRole("list", { name: "Sessions" })).getAllByRole("button");
    expect(rows[0]).toHaveAttribute("aria-current", "true");
    expect(rows[0]).toBeEnabled();
    expect(rows[1]).not.toHaveAttribute("aria-current");
    expect(rows[1]).toBeDisabled();
    expect(screen.getByRole("button", { name: "New session" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Switch project" }));
    await userEvent.click(within(screen.getByRole("list", { name: "Projects" })).getByRole("button", { name: /Dawn/ }));
    expect(screen.getByRole("status")).toHaveTextContent(PROJECTS_AGENT_BUSY);
    expect(api.codeProjectOpen).not.toHaveBeenCalled();
    expect(calls.onSwitched).not.toHaveBeenCalled();
  });

  it("tells the tab the app's folder, and reads the project and its sessions again when told", async () => {
    const calls = { onSwitched: vi.fn(), onNewSession: vi.fn(), onOpenSession: vi.fn() };
    const onFolder = vi.fn();
    const view = render(<CodeProjects {...calls} onFolder={onFolder} refresh={1} />);
    await waitFor(() => expect(onFolder).toHaveBeenCalledWith("/work/harbor"));
    expect(api.listChats).toHaveBeenCalledTimes(1);
    // A command typed in the agent window moved the app to Dawn meanwhile.
    api.codeProjects.mockResolvedValue(onDawn());
    api.listChats.mockResolvedValue([chat("z", "p2", 50)]);
    view.rerender(<CodeProjects {...calls} onFolder={onFolder} refresh={2} />);
    expect(await screen.findByRole("button", { name: /Chat z/ })).toBeInTheDocument();
    expect(api.listChats).toHaveBeenCalledTimes(2);
    expect(api.codeProjects).toHaveBeenCalledTimes(2);
    expect(onFolder).toHaveBeenLastCalledWith("/work/dawn");
    expect(calls.onSwitched).not.toHaveBeenCalled();
  });
});

describe("helpers", () => {
  it("folderName takes the last part of either kind of path", () => {
    expect(folderName("/work/harbor/")).toBe("harbor");
    expect(folderName("C:\\Users\\user\\dawn")).toBe("dawn");
  });

  it("projectSessions keeps one project's chats, newest first", () => {
    const rows = [chat("a", "p1", 10), chat("b", "p2", 30), chat("c", "p1", 20)];
    expect(projectSessions(rows, "p1").map((r) => r.id)).toEqual(["c", "a"]);
    expect(projectSessions(rows, "p4")).toEqual([]);
  });

});
