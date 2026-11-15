import "@testing-library/jest-dom/vitest";
import { act, render, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CompanionNote } from "./api";

const api = vi.hoisted(() => ({
  companionNote: vi.fn<(appId: number) => Promise<CompanionNote>>(),
  companionSetNote: vi.fn<(appId: number, text: string) => Promise<string>>(),
}));
vi.mock("./api", () => api);

import { GameNote, NOTE_STATUS_MS, useNoteStatus } from "./GameNote";

describe("GameNote", () => {
  beforeEach(() => {
    api.companionNote.mockReset();
    api.companionSetNote.mockReset();
    api.companionNote.mockResolvedValue({ game: "Hades", note: "try the spear" });
  });

  it("opens with the game's name and its saved note, and Save keeps the new one", async () => {
    api.companionSetNote.mockResolvedValue("beat Megaera next");
    const done = vi.fn();
    render(<GameNote appId={7} onDone={done} />);
    const box = await screen.findByDisplayValue("try the spear");
    expect(box).toHaveFocus();
    expect(screen.getByRole("dialog", { name: "Note for Hades" })).toBeVisible();
    expect(screen.getByText("Shown on the card the next time you launch it.")).toBeVisible();
    expect(box).toHaveAttribute("placeholder", "remember I was farming X");
    expect(box).toHaveAttribute("maxLength", "500");
    await userEvent.clear(box);
    await userEvent.type(box, "beat Megaera next");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.companionNote).toHaveBeenCalledWith(7);
    expect(api.companionSetNote).toHaveBeenCalledWith(7, "beat Megaera next");
    expect(done).toHaveBeenCalledWith("Saved. It shows on the card next time you launch it.");
  });

  it("Cancel and Esc save nothing", async () => {
    const done = vi.fn();
    render(<GameNote appId={7} onDone={done} />);
    await screen.findByDisplayValue("try the spear");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(done).toHaveBeenLastCalledWith(null);
    await userEvent.keyboard("{Escape}");
    expect(done.mock.calls).toEqual([[null], [null]]);
    expect(api.companionSetNote).not.toHaveBeenCalled();
  });

  it("a refused save says why and keeps the dialog open", async () => {
    api.companionSetNote.mockRejectedValue("Library is off on this device.");
    const done = vi.fn();
    render(<GameNote appId={7} onDone={done} />);
    await screen.findByDisplayValue("try the spear");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Library is off on this device.");
    expect(done).not.toHaveBeenCalled();
  });

  it("saving an empty box removes the note and says so", async () => {
    api.companionSetNote.mockResolvedValue("");
    const done = vi.fn();
    render(<GameNote appId={7} onDone={done} />);
    await userEvent.clear(await screen.findByDisplayValue("try the spear"));
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(api.companionSetNote).toHaveBeenCalledWith(7, "");
    expect(done).toHaveBeenCalledWith("Removed. The next launch card has no note.");
  });

  it("a note that can't be read leaves the box and Save off and says why", async () => {
    api.companionNote.mockRejectedValue("game_notes.json: no access");
    render(<GameNote appId={7} onDone={vi.fn()} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("game_notes.json: no access");
    expect(screen.getByRole("textbox", { name: "Your note" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
  });
});

describe("useNoteStatus", () => {
  it("clears the status line on its own", () => {
    vi.useFakeTimers();
    try {
      const { result } = renderHook(() => useNoteStatus());
      act(() => result.current[1]("Saved. It shows on the card next time you launch it."));
      expect(result.current[0]).toBe("Saved. It shows on the card next time you launch it.");
      act(() => vi.advanceTimersByTime(NOTE_STATUS_MS - 1));
      expect(result.current[0]).toBe("Saved. It shows on the card next time you launch it.");
      act(() => vi.advanceTimersByTime(1));
      expect(result.current[0]).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("a second save with the same words keeps the line for its own full time", () => {
    vi.useFakeTimers();
    try {
      const saved = "Saved. It shows on the card next time you launch it.";
      const { result } = renderHook(() => useNoteStatus());
      act(() => result.current[1](saved));
      act(() => vi.advanceTimersByTime(NOTE_STATUS_MS - 1000));
      act(() => result.current[1](saved));
      act(() => vi.advanceTimersByTime(NOTE_STATUS_MS - 1));
      expect(result.current[0]).toBe(saved);
      act(() => vi.advanceTimersByTime(1));
      expect(result.current[0]).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
