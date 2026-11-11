import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatRow,
  DeleteAllResult,
  ProjectNode,
  ReadOnlyRow,
  SavedFile,
  StepRow,
  Transcript,
} from "./api";

const api = vi.hoisted(() => ({
  listChats: vi.fn<() => Promise<ChatRow[]>>(),
  searchChats: vi.fn<(query: string) => Promise<ChatRow[]>>(),
  renameChat: vi.fn<(id: string, title: string) => Promise<void>>(),
  deleteChat: vi.fn<(id: string) => Promise<void>>(),
  exportChat: vi.fn<(id: string) => Promise<SavedFile>>(),
  openSaved: vi.fn<(token: string) => Promise<void>>(),
  revealSaved: vi.fn<(token: string) => Promise<void>>(),
  pinChat: vi.fn<(id: string) => Promise<boolean>>(),
  listProjectNodes: vi.fn<() => Promise<ProjectNode[]>>(),
  listReadOnlyChats: vi.fn<(query: string) => Promise<ReadOnlyRow[]>>(),
  viewReadOnlyChat: vi.fn<(id: string) => Promise<Transcript>>(),
  searchSteps: vi.fn<(query: string) => Promise<StepRow[]>>(),
  deleteAllChats: vi.fn<() => Promise<DeleteAllResult>>(),
  cancelDeleteAll: vi.fn<() => Promise<void>>(),
}));

vi.mock("./api", () => api);

import {
  DELETE_ALL_ARM_MS,
  grouped,
  groupOf,
  History,
  HISTORY_EMPTY,
  HISTORY_NO_HITS,
  projectTree,
  SEARCH_DELAY_MS,
} from "./History";

// Wednesday Oct 28 2026, 3 PM local.
const NOW = new Date(2026, 9, 28, 15, 0);
const at = (days: number, hour = 10) =>
  new Date(2026, 9, 28 - days, hour, 0).getTime();

const row = (id: string, title: string, updated: number): ChatRow => ({
  id,
  title,
  created: updated - 60_000,
  updated,
});

const TODAY = row("shell-a", "Fix the build", at(0, 9));
const WEEK = row("shell-b", "Plan a trip", at(3));
const OLD = row("shell-c", "Paint colours", at(20));

describe("grouping", () => {
  it("waits like the cabin's History search", () => {
    expect(SEARCH_DELAY_MS).toBe(250);
  });

  it("splits at local midnight and six days before it", () => {
    expect(groupOf(at(0, 0), NOW)).toBe("Today");
    expect(groupOf(at(1, 23), NOW)).toBe("This week");
    expect(groupOf(at(6, 0), NOW)).toBe("This week");
    expect(groupOf(at(7, 23), NOW)).toBe("Older");
    expect(
      grouped([TODAY, WEEK, OLD], NOW).map(([g, rows]) => [
        g,
        rows.map((r) => r.id),
      ]),
    ).toEqual([
      ["Today", ["shell-a"]],
      ["This week", ["shell-b"]],
      ["Older", ["shell-c"]],
    ]);
    expect(grouped([OLD], NOW).map(([g]) => g)).toEqual(["Older"]);
  });
});

describe("History", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
    for (const fn of Object.values(api)) fn.mockReset();
    api.listChats.mockResolvedValue([TODAY, WEEK, OLD]);
    api.renameChat.mockResolvedValue();
    api.deleteChat.mockResolvedValue();
    api.listProjectNodes.mockResolvedValue([]);
    api.listReadOnlyChats.mockResolvedValue([]);
    api.searchSteps.mockResolvedValue([]);
    api.cancelDeleteAll.mockResolvedValue();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("lists the saved chats under Today, This week and Older", async () => {
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    const today = await screen.findByRole("region", { name: "Today" });
    expect(
      within(today).getByRole("listitem", { name: "Fix the build" }),
    ).toBeVisible();
    expect(
      within(screen.getByRole("region", { name: "This week" })).getByText(
        "Plan a trip",
      ),
    ).toBeVisible();
    expect(
      within(screen.getByRole("region", { name: "Older" })).getByText(
        "Paint colours",
      ),
    ).toBeVisible();
    expect(api.searchChats).not.toHaveBeenCalled();
  });

  it("says so when there are none", async () => {
    api.listChats.mockResolvedValue([]);
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    expect(await screen.findByText(HISTORY_EMPTY)).toBeVisible();
  });

  it("a click opens the chat", async () => {
    const onOpen = vi.fn();
    render(<History onOpen={onOpen} onDeleted={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: /^Plan a trip/ }),
    );
    expect(onOpen).toHaveBeenCalledWith("shell-b");
  });

  it("searches once typing pauses, and only the newest search fills the page", async () => {
    api.searchChats.mockImplementation((q) =>
      Promise.resolve(q === "trip" ? [WEEK] : []),
    );
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await screen.findByText("Fix the build");
    const box = screen.getByRole("searchbox", { name: "Search chats" });
    await userEvent.type(box, "trip");
    // Not on every key: the search waits for the pause.
    expect(api.searchChats).not.toHaveBeenCalled();
    await waitFor(() => expect(api.searchChats).toHaveBeenCalledWith("trip"));
    expect(api.searchChats).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(screen.queryByText("Fix the build")).toBeNull());
    expect(screen.getByText("Plan a trip")).toBeVisible();

    await userEvent.type(box, "zz");
    expect(await screen.findByText(HISTORY_NO_HITS)).toBeVisible();
    expect(api.searchChats).toHaveBeenLastCalledWith("tripzz");

    await userEvent.clear(box);
    expect(await screen.findByText("Fix the build")).toBeVisible();
  });

  it("Delete asks first, and Keep deletes nothing", async () => {
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "More for Plan a trip" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    const ask = screen.getByRole("group", { name: "Delete Plan a trip?" });
    expect(api.deleteChat).not.toHaveBeenCalled();
    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(
      screen.queryByRole("group", { name: "Delete Plan a trip?" }),
    ).toBeNull();
    expect(api.deleteChat).not.toHaveBeenCalled();

    api.listChats.mockResolvedValue([TODAY, OLD]);
    await userEvent.click(
      screen.getByRole("button", { name: "More for Plan a trip" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await userEvent.click(
      within(
        screen.getByRole("group", { name: "Delete Plan a trip?" }),
      ).getByRole("button", { name: "Delete" }),
    );
    expect(api.deleteChat).toHaveBeenCalledWith("shell-b");
    expect(onDeleted).toHaveBeenCalledWith("shell-b");
    await waitFor(() => expect(screen.queryByText("Plan a trip")).toBeNull());
  });

  it("Rename saves the new name and shows it", async () => {
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "More for Fix the build" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
    const name = screen.getByRole("textbox", { name: "Chat name" });
    await userEvent.clear(name);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    api.listChats.mockResolvedValue([{ ...TODAY, title: "CI fix" }, WEEK, OLD]);
    await userEvent.type(name, "CI fix{Enter}");
    expect(api.renameChat).toHaveBeenCalledWith("shell-a", "CI fix");
    expect(
      await screen.findByRole("button", { name: /^CI fix/ }),
    ).toBeVisible();
  });

  it("shows a failed delete as an error and keeps the row", async () => {
    api.deleteChat.mockRejectedValue("disk full");
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "More for Fix the build" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await userEvent.click(
      within(
        screen.getByRole("group", { name: "Delete Fix the build?" }),
      ).getByRole("button", { name: "Delete" }),
    );
    expect(await screen.findByText("disk full")).toHaveClass("status--error");
    expect(onDeleted).not.toHaveBeenCalled();
    expect(screen.getByText("Fix the build")).toBeVisible();
  });
  it("Export saves the chat and the card opens or shows the file by its token", async () => {
    api.exportChat.mockResolvedValue({
      token: "saved-1",
      name: "Plan-a-trip-20261028-1500.md",
      folder: "Chats",
      path: "/home/user/Documents/Loadout/Chats/Plan-a-trip-20261028-1500.md",
      line: "Saved Plan-a-trip-20261028-1500.md to Chats",
      revealLabel: "Show in folder",
    });
    api.openSaved.mockResolvedValue();
    api.revealSaved.mockResolvedValue();
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "More for Plan a trip" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Export" }));
    expect(api.exportChat).toHaveBeenCalledWith("shell-b");
    const card = await screen.findByRole("status", {
      name: "Saved Plan-a-trip-20261028-1500.md to Chats",
    });
    await userEvent.click(within(card).getByRole("button", { name: "Open" }));
    expect(api.openSaved).toHaveBeenCalledWith("saved-1");
    await userEvent.click(
      within(card).getByRole("button", { name: "Show in folder" }),
    );
    expect(api.revealSaved).toHaveBeenCalledWith("saved-1");
    await userEvent.click(within(card).getByRole("button", { name: "Close" }));
    expect(
      screen.queryByRole("status", { name: /Saved Plan-a-trip/ }),
    ).toBeNull();
  });

  it("a failed export says why and leaves no card", async () => {
    api.exportChat.mockRejectedValue(
      "Couldn't save the chat: the disk is full. Nothing was saved.",
    );
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await userEvent.click(
      await screen.findByRole("button", { name: "More for Fix the build" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Export" }));
    expect(
      await screen.findByText(
        "Couldn't save the chat: the disk is full. Nothing was saved.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open" })).toBeNull();
  });
});

const node = (
  id: string,
  name: string,
  kind: ProjectNode["kind"],
  parent: string | null = null,
): ProjectNode => ({
  id,
  name,
  kind,
  parent,
  open: true,
});

describe("Projects tree, Pinned group and read-only chats", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(NOW);
    for (const fn of Object.values(api)) fn.mockReset();
    api.listProjectNodes.mockResolvedValue([]);
    api.listReadOnlyChats.mockResolvedValue([]);
    api.searchSteps.mockResolvedValue([]);
    api.cancelDeleteAll.mockResolvedValue();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("builds the tree from parents, with a stray row at the top", () => {
    const tree = projectTree([
      node("f", "Boats", "folder"),
      node("p", "Pier", "project", "f"),
      node("q", "Loose", "project", "gone"),
    ]);
    expect(
      tree.map((t) => [t.node.name, t.children.map((c) => c.node.name)]),
    ).toEqual([
      ["Boats", ["Pier"]],
      ["Loose", []],
    ]);
  });

  it("lists a project's chats under it, and keeps them out of the day groups", async () => {
    api.listProjectNodes.mockResolvedValue([
      node("f", "Boats", "folder"),
      node("p", "Pier", "project", "f"),
    ]);
    api.listChats.mockResolvedValue([{ ...TODAY, project: "p" }, WEEK]);
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    const projects = await screen.findByRole("region", { name: "Projects" });
    expect(
      within(projects).getByRole("button", { name: /Boats/ }),
    ).toHaveAttribute("aria-expanded", "true");
    expect(
      within(projects).getByRole("button", { name: "▾ Pier (1)" }),
    ).toBeVisible();
    expect(
      within(projects).getByRole("listitem", { name: "Fix the build" }),
    ).toBeVisible();
    expect(
      within(screen.getByRole("region", { name: "This week" })).getByText(
        "Plan a trip",
      ),
    ).toBeVisible();
    expect(screen.queryByRole("region", { name: "Today" })).toBeNull();
    await userEvent.click(
      within(projects).getByRole("button", { name: /Boats/ }),
    );
    expect(screen.queryByText("Fix the build")).toBeNull();
  });

  it("lists pinned chats first, and the menu pins and unpins", async () => {
    api.listChats.mockResolvedValue([{ ...OLD, pinned: true }, TODAY]);
    api.pinChat.mockResolvedValue(false);
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    const pinned = await screen.findByRole("region", { name: "Pinned" });
    expect(within(pinned).getByText("Paint colours")).toBeVisible();
    expect(screen.queryByRole("region", { name: "Older" })).toBeNull();
    expect(
      within(screen.getByRole("region", { name: "Today" })).getByText(
        "Fix the build",
      ),
    ).toBeVisible();
    await userEvent.click(
      screen.getByRole("button", { name: "More for Paint colours" }),
    );
    await userEvent.click(screen.getByRole("menuitem", { name: "Unpin" }));
    expect(api.pinChat).toHaveBeenCalledWith("shell-c");
    await userEvent.click(
      screen.getByRole("button", { name: "More for Fix the build" }),
    );
    expect(screen.getByRole("menuitem", { name: "Pin" })).toBeVisible();
  });

  it("Delete all arms first and deletes nothing until the second click", async () => {
    const onDeleted = vi.fn();
    api.listChats.mockResolvedValue([TODAY, WEEK]);
    api.deleteAllChats
      .mockResolvedValueOnce({ state: "armed", count: 2 })
      .mockResolvedValueOnce({ state: "deleted", count: 2 });
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await screen.findByText("Fix the build");
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    expect(
      await screen.findByRole("button", { name: "Delete all 2 chats?" }),
    ).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Click again to delete every chat",
    );
    expect(onDeleted).not.toHaveBeenCalled();
    api.listChats.mockResolvedValue([]);
    await userEvent.click(
      screen.getByRole("button", { name: "Delete all 2 chats?" }),
    );
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(2));
    expect(onDeleted).toHaveBeenCalledWith("shell-a");
    expect(onDeleted).toHaveBeenCalledWith("shell-b");
    expect(await screen.findByText(HISTORY_EMPTY)).toBeVisible();
    expect(screen.getByRole("button", { name: "Delete all" })).toBeDisabled();
  });

  it("a too-quick second click stays armed, and Cancel disarms", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.deleteAllChats.mockResolvedValue({ state: "armed", count: 1 });
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await screen.findByText("Fix the build");
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    await userEvent.click(
      await screen.findByRole("button", { name: "Delete all 1 chat?" }),
    );
    expect(api.deleteAllChats).toHaveBeenCalledTimes(2);
    expect(
      screen.getByRole("button", { name: "Delete all 1 chat?" }),
    ).toBeVisible();
    expect(onDeleted).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.cancelDeleteAll).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Delete all" })).toBeVisible();
  });

  it("disarms by itself when the window lapses", async () => {
    vi.useRealTimers();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    api.listChats.mockResolvedValue([TODAY]);
    api.deleteAllChats.mockResolvedValue({ state: "armed", count: 1 });
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await screen.findByText("Fix the build");
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    await screen.findByRole("button", { name: "Delete all 1 chat?" });
    await vi.advanceTimersByTimeAsync(DELETE_ALL_ARM_MS + 100);
    expect(
      await screen.findByRole("button", { name: "Delete all" }),
    ).toBeVisible();
    expect(api.cancelDeleteAll).toHaveBeenCalledTimes(1);
    expect(api.deleteAllChats).toHaveBeenCalledTimes(1);
  });

  it("shows read-only CLI and cabin chats that open to look at, with no menu", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.listReadOnlyChats.mockResolvedValue([
      {
        id: "0198f2a4-7b1c-7d2e-9a3f-5c6d7e8f9a0b",
        title: "Sketch the harbor wall",
        updated: 0,
        source: "cli",
      },
      { id: "cabin-1", title: "Cabin thread", updated: at(2), source: "cabin" },
    ]);
    api.viewReadOnlyChat.mockResolvedValue({
      id: "0198f2a4-7b1c-7d2e-9a3f-5c6d7e8f9a0b",
      title: "Sketch the harbor wall",
      lines: [
        { role: "user", text: "Sketch the harbor wall" },
        { role: "assistant", text: "Here is a sketch." },
      ],
      earlier: 0,
    });
    const onOpen = vi.fn();
    render(<History onOpen={onOpen} onDeleted={() => {}} />);
    const group = await screen.findByRole("region", {
      name: "Read only chats",
    });
    expect(within(group).getByText("Grok CLI · Read only")).toBeVisible();
    expect(within(group).getByText(/^Cabin · Read only · /)).toBeVisible();
    expect(
      within(group).queryByRole("button", { name: /^More for/ }),
    ).toBeNull();
    await userEvent.click(
      within(group).getByRole("button", { name: /^Sketch the harbor wall/ }),
    );
    const view = await screen.findByRole("region", {
      name: "Sketch the harbor wall (read only)",
    });
    expect(within(view).getByText("Here is a sketch.")).toBeVisible();
    expect(api.viewReadOnlyChat).toHaveBeenCalledWith(
      "0198f2a4-7b1c-7d2e-9a3f-5c6d7e8f9a0b",
    );
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("a search lists matching tool steps, and a click opens that chat", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.searchChats.mockResolvedValue([]);
    api.searchSteps.mockResolvedValue([
      {
        chat: "shell-a",
        turn: 2,
        line: "Fix the build · turn 2 · write_file · allow",
      },
    ]);
    const onOpen = vi.fn();
    render(<History onOpen={onOpen} onDeleted={() => {}} />);
    await screen.findByText("Fix the build");
    await userEvent.type(
      screen.getByRole("searchbox", { name: "Search chats" }),
      "write notes",
    );
    await waitFor(() =>
      expect(api.searchSteps).toHaveBeenCalledWith("write notes"),
    );
    const steps = await screen.findByRole("region", { name: "Tool steps" });
    await userEvent.click(
      within(steps).getByRole("button", {
        name: "Fix the build · turn 2 · write_file · allow",
      }),
    );
    expect(onOpen).toHaveBeenCalledWith("shell-a");
    expect(screen.queryByText(HISTORY_NO_HITS)).toBeNull();
  });
});
