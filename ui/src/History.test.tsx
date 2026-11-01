import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatRow } from "./api";

const api = vi.hoisted(() => ({
  listChats: vi.fn<() => Promise<ChatRow[]>>(),
  searchChats: vi.fn<(query: string) => Promise<ChatRow[]>>(),
  renameChat: vi.fn<(id: string, title: string) => Promise<void>>(),
  deleteChat: vi.fn<(id: string) => Promise<void>>(),
}));

vi.mock("./api", () => api);

import { grouped, groupOf, History, HISTORY_EMPTY, HISTORY_NO_HITS, SEARCH_DELAY_MS } from "./History";

// Wednesday Oct 28 2026, 3 PM local.
const NOW = new Date(2026, 9, 28, 15, 0);
const at = (days: number, hour = 10) => new Date(2026, 9, 28 - days, hour, 0).getTime();

const row = (id: string, title: string, updated: number): ChatRow => ({
  id,
  title,
  created: updated - 60_000,
  updated,
  model: "grok-4.7",
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
    expect(grouped([TODAY, WEEK, OLD], NOW).map(([g, rows]) => [g, rows.map((r) => r.id)])).toEqual([
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
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("lists the saved chats under Today, This week and Older", async () => {
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    const today = await screen.findByRole("region", { name: "Today" });
    expect(within(today).getByRole("listitem", { name: "Fix the build" })).toBeVisible();
    expect(within(screen.getByRole("region", { name: "This week" })).getByText("Plan a trip")).toBeVisible();
    expect(within(screen.getByRole("region", { name: "Older" })).getByText("Paint colours")).toBeVisible();
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
    await userEvent.click(await screen.findByRole("button", { name: /^Plan a trip/ }));
    expect(onOpen).toHaveBeenCalledWith("shell-b");
  });

  it("searches once typing pauses, and only the newest search fills the page", async () => {
    api.searchChats.mockImplementation((q) => Promise.resolve(q === "trip" ? [WEEK] : []));
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
    await userEvent.click(await screen.findByRole("button", { name: "More for Plan a trip" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    const ask = screen.getByRole("group", { name: "Delete Plan a trip?" });
    expect(api.deleteChat).not.toHaveBeenCalled();
    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(screen.queryByRole("group", { name: "Delete Plan a trip?" })).toBeNull();
    expect(api.deleteChat).not.toHaveBeenCalled();

    api.listChats.mockResolvedValue([TODAY, OLD]);
    await userEvent.click(screen.getByRole("button", { name: "More for Plan a trip" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await userEvent.click(
      within(screen.getByRole("group", { name: "Delete Plan a trip?" })).getByRole("button", { name: "Delete" }),
    );
    expect(api.deleteChat).toHaveBeenCalledWith("shell-b");
    expect(onDeleted).toHaveBeenCalledWith("shell-b");
    await waitFor(() => expect(screen.queryByText("Plan a trip")).toBeNull());
  });

  it("Rename saves the new name and shows it", async () => {
    render(<History onOpen={() => {}} onDeleted={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "More for Fix the build" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Rename" }));
    const name = screen.getByRole("textbox", { name: "Chat name" });
    await userEvent.clear(name);
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    api.listChats.mockResolvedValue([{ ...TODAY, title: "CI fix" }, WEEK, OLD]);
    await userEvent.type(name, "CI fix{Enter}");
    expect(api.renameChat).toHaveBeenCalledWith("shell-a", "CI fix");
    expect(await screen.findByRole("button", { name: /^CI fix/ })).toBeVisible();
  });

  it("shows a failed delete as an error and keeps the row", async () => {
    api.deleteChat.mockRejectedValue("disk full");
    const onDeleted = vi.fn();
    render(<History onOpen={() => {}} onDeleted={onDeleted} />);
    await userEvent.click(await screen.findByRole("button", { name: "More for Fix the build" }));
    await userEvent.click(screen.getByRole("menuitem", { name: "Delete" }));
    await userEvent.click(
      within(screen.getByRole("group", { name: "Delete Fix the build?" })).getByRole("button", { name: "Delete" }),
    );
    expect(await screen.findByText("disk full")).toHaveClass("status--error");
    expect(onDeleted).not.toHaveBeenCalled();
    expect(screen.getByText("Fix the build")).toBeVisible();
  });
});
