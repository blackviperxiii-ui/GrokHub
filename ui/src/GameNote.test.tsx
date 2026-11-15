import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CompanionNote } from "./api";

const api = vi.hoisted(() => ({
  companionNote: vi.fn<(appId: number) => Promise<CompanionNote>>(),
  companionSetNote: vi.fn<(appId: number, text: string) => Promise<string>>(),
}));
vi.mock("./api", () => api);

import { GameNote } from "./GameNote";

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
    expect(done).toHaveBeenCalledTimes(2);
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
});
