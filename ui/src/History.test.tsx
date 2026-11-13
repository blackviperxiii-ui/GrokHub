import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Activity, useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  ChatRow,
  DeleteAllResult,
  ProjectNode,
  QuickChip,
  ReadOnlyRow,
  SavedFile,
  Settings,
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
  // What the Chat page calls when it mounts ("Delete all and the open chat").
  getStartChat: vi.fn<() => Promise<Transcript | null>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  quickChips: vi.fn<() => Promise<QuickChip[]>>(),
  slashHits: vi.fn<() => Promise<never[]>>(),
  helperQuestions: vi.fn<() => Promise<never[]>>(),
  onChatEvent: vi.fn<() => Promise<() => void>>(),
  onEventGap: vi.fn<() => Promise<() => void>>(),
  onFileDrop: vi.fn<() => Promise<() => void>>(),
}));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return { ...chatTabApiStubs(), ...api };
});
vi.mock("./autoSetupApi", () => ({ settingAsk: () => Promise.resolve(null) }));

import { Chat } from "./Chat";
import {
  DELETE_ALL_ARM_MS,
  DELETE_ALL_NONE,
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

/** The app's answer to the click that deletes: the chats it deleted. */
const gone = (ids: string[]): DeleteAllResult & { ids: string[] } => ({
  state: "deleted",
  count: ids.length,
  ids,
});

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

  it("Main's menu only exports: no pin, rename or delete", async () => {
    api.listChats.mockResolvedValue([
      { id: "shell-main-1", title: "Main", created: TODAY.created, updated: TODAY.updated, pinned: true, main: true },
      TODAY,
    ]);
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "More for Main" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual(["Export"]);
    await userEvent.click(screen.getByRole("button", { name: "More for Fix the build" }));
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Pin",
      "Rename",
      "Export",
      "Delete",
    ]);
  });

  it("Delete all arms first and deletes nothing until the second click", async () => {
    const onDeleted = vi.fn();
    api.listChats.mockResolvedValue([TODAY, WEEK]);
    api.deleteAllChats
      .mockResolvedValueOnce({ state: "armed", count: 2 })
      .mockResolvedValueOnce(gone(["shell-a", "shell-b"]));
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

  it("with a search typed, Delete all counts and reports the chats the app deletes, not the hits", async () => {
    const onDeleted = vi.fn();
    api.listChats.mockResolvedValue([TODAY, WEEK, OLD]);
    api.searchChats.mockResolvedValue([WEEK]);
    api.deleteAllChats
      .mockResolvedValueOnce({ state: "armed", count: 3 })
      .mockResolvedValueOnce(gone(["shell-a", "shell-b", "shell-c"]));
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await screen.findByText("Fix the build");
    await userEvent.type(
      screen.getByRole("searchbox", { name: "Search chats" }),
      "trip",
    );
    await waitFor(() => expect(screen.queryByText("Fix the build")).toBeNull());
    expect(screen.getByText("Plan a trip")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    expect(
      await screen.findByRole("button", { name: "Delete all 3 chats?" }),
    ).toBeVisible();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "Click again to delete every chat and its saved files, not only the chats this search lists. This can't be undone.",
    );
    await userEvent.click(
      screen.getByRole("button", { name: "Delete all 3 chats?" }),
    );
    await waitFor(() => expect(onDeleted).toHaveBeenCalledTimes(3));
    expect(onDeleted.mock.calls).toEqual([["shell-a"], ["shell-b"], ["shell-c"]]);
  });

  it("a search with no hits leaves Delete all on, and no chats at all says so", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.searchChats.mockResolvedValue([]);
    api.deleteAllChats.mockResolvedValue({ state: "armed", count: 0 });
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await screen.findByText("Fix the build");
    await userEvent.type(
      screen.getByRole("searchbox", { name: "Search chats" }),
      "zz",
    );
    expect(await screen.findByText(HISTORY_NO_HITS)).toBeVisible();
    const button = screen.getByRole("button", { name: "Delete all" });
    expect(button).toBeEnabled();
    await userEvent.click(button);
    expect(await screen.findByText(DELETE_ALL_NONE)).toBeVisible();
    expect(DELETE_ALL_NONE).toBe("There are no saved chats to delete.");
    expect(screen.getByRole("button", { name: "Delete all" })).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(api.cancelDeleteAll).toHaveBeenCalledTimes(1);
  });

  it("leaving the page while Delete all is armed disarms it", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.deleteAllChats.mockResolvedValue({ state: "armed", count: 1 });
    const page = (shown: boolean) => (
      <Activity mode={shown ? "visible" : "hidden"}>
        <History onOpen={() => {}} onDeleted={() => {}} />
      </Activity>
    );
    const { rerender } = render(page(true));
    await screen.findByText("Fix the build");
    // Coming and going with nothing armed asks the app for nothing.
    rerender(page(false));
    rerender(page(true));
    expect(api.cancelDeleteAll).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    await screen.findByRole("button", { name: "Delete all 1 chat?" });
    rerender(page(false));
    rerender(page(true));
    expect(
      await screen.findByRole("button", { name: "Delete all" }),
    ).toBeVisible();
    expect(screen.queryByRole("alert")).toBeNull();
    expect(api.cancelDeleteAll).toHaveBeenCalledTimes(1);
  });

  it("the Delete all button keeps keyboard focus while the app answers, so Enter again confirms", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    let answer: (result: DeleteAllResult) => void = () => {};
    api.deleteAllChats
      .mockImplementationOnce(
        () => new Promise<DeleteAllResult>((resolve) => (answer = resolve)),
      )
      .mockResolvedValueOnce(gone(["shell-a"]));
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await screen.findByText("Fix the build");
    const button = screen.getByRole("button", { name: "Delete all" });
    button.focus();
    await userEvent.keyboard("{Enter}");
    expect(api.deleteAllChats).toHaveBeenCalledTimes(1);
    // A disabled button drops focus to the page in Chromium; this one only
    // says it is busy, and ignores a press until the app has answered.
    expect(button).toBeEnabled();
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(api.deleteAllChats).toHaveBeenCalledTimes(1);
    answer({ state: "armed", count: 1 });
    const armed = await screen.findByRole("button", {
      name: "Delete all 1 chat?",
    });
    expect(armed).toHaveFocus();
    await waitFor(() => expect(armed).toHaveAttribute("aria-disabled", "false"));
    await userEvent.keyboard("{Enter}");
    await waitFor(() => expect(api.deleteAllChats).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(onDeleted).toHaveBeenCalledWith("shell-a"));
  });

  it("a held Enter on Delete all is one press: its key repeats never click", async () => {
    api.listChats.mockResolvedValue([TODAY]);
    api.deleteAllChats.mockResolvedValue({ state: "armed", count: 1 });
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await screen.findByText("Fix the build");
    const button = screen.getByRole("button", { name: "Delete all" });
    // A browser clicks the focused button on every Enter keydown, the
    // repeats of a held key included, unless the keydown is cancelled. A
    // repeat half a second after the first press would be the click that
    // deletes. fireEvent answers false when the keydown was cancelled.
    expect(fireEvent.keyDown(button, { key: "Enter" })).toBe(true);
    expect(fireEvent.keyDown(button, { key: "Enter", repeat: true })).toBe(false);
    await userEvent.click(button);
    const armed = await screen.findByRole("button", {
      name: "Delete all 1 chat?",
    });
    expect(fireEvent.keyDown(armed, { key: "Enter", repeat: true })).toBe(false);
    expect(fireEvent.keyDown(armed, { key: "Enter" })).toBe(true);
    // A held Tab still moves on past the button.
    expect(fireEvent.keyDown(armed, { key: "Tab", repeat: true })).toBe(true);
  });

  it("when Delete all stops part-way, the chats that went are reported and the reason shows", async () => {
    api.listChats.mockResolvedValue([TODAY, WEEK]);
    // The app's answer when the first chat went and the second could not.
    const stopped: DeleteAllResult & { ids: string[]; error: string } = {
      ...gone(["shell-a"]),
      error: "Access is denied.",
    };
    api.deleteAllChats
      .mockResolvedValueOnce({ state: "armed", count: 2 })
      .mockResolvedValueOnce(stopped);
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await screen.findByText("Fix the build");
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    api.listChats.mockResolvedValue([WEEK]);
    await userEvent.click(
      await screen.findByRole("button", { name: "Delete all 2 chats?" }),
    );
    expect(await screen.findByText("Access is denied.")).toBeVisible();
    expect(onDeleted.mock.calls).toEqual([["shell-a"]]);
    expect(screen.getByRole("button", { name: "Delete all" })).toBeVisible();
    expect(screen.getByText("Plan a trip")).toBeVisible();
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

const SETTINGS: Settings = {
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/user",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: true,
};

/** Chat and History side by side, with the `deleted` state App.tsx keeps
 * between them. */
function ChatAndHistory() {
  const [deleted, setDeleted] = useState<{
    id: string;
    ids: string[];
    n: number;
  }>();
  return (
    <>
      <Chat onOpenSettings={() => {}} deleted={deleted} />
      <History
        onOpen={() => {}}
        onDeleted={(id) =>
          setDeleted((now) => ({
            id,
            ids: [...(now?.ids ?? []), id],
            n: (now?.n ?? 0) + 1,
          }))
        }
      />
    </>
  );
}

describe("Delete all and the open chat", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.listProjectNodes.mockResolvedValue([]);
    api.listReadOnlyChats.mockResolvedValue([]);
    api.searchSteps.mockResolvedValue([]);
    api.cancelDeleteAll.mockResolvedValue();
    api.getSettings.mockResolvedValue(SETTINGS);
    api.quickChips.mockResolvedValue([]);
    api.slashHits.mockResolvedValue([]);
    api.helperQuestions.mockResolvedValue([]);
    for (const listen of [api.onChatEvent, api.onEventGap, api.onFileDrop]) {
      listen.mockResolvedValue(() => {});
    }
  });

  it("closes the open chat when it was the newest of several, not only when it was the last row", async () => {
    // The open chat is the newest, so History lists it first.
    api.getStartChat.mockResolvedValue({
      id: "shell-a",
      title: "Fix the build",
      lines: [
        { role: "user", text: "why does the build fail" },
        { role: "assistant", text: "A missing import." },
      ],
      earlier: 0,
    });
    api.listChats.mockResolvedValue([TODAY, WEEK, OLD]);
    api.deleteAllChats
      .mockResolvedValueOnce({ state: "armed", count: 3 })
      .mockResolvedValueOnce(gone(["shell-a", "shell-b", "shell-c"]));
    render(<ChatAndHistory />);
    expect(await screen.findByText("why does the build fail")).toBeVisible();
    await screen.findByRole("listitem", { name: "Fix the build" });
    await userEvent.click(screen.getByRole("button", { name: "Delete all" }));
    api.listChats.mockResolvedValue([]);
    await userEvent.click(
      await screen.findByRole("button", { name: "Delete all 3 chats?" }),
    );
    await waitFor(() =>
      expect(screen.queryByText("why does the build fail")).toBeNull(),
    );
    expect(screen.queryByText("A missing import.")).toBeNull();
  });
});
