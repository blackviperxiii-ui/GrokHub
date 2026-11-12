import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CompanionFrame } from "./api";

const api = vi.hoisted(() => ({
  companionFrame: vi.fn<
    (noteId: number, lastPlayed: number, steamAppId?: number | null) => Promise<CompanionFrame>
  >(),
  companionSetNote: vi.fn<(appId: number, text: string) => Promise<string>>(),
}));

vi.mock("./api", () => api);
vi.mock("./GameWatch", () => ({ GameWatch: () => <p>Game watcher compact</p> }));

import { GameCompanion, NEWS_UNAVAILABLE, NO_NEWS, noteIdFor } from "./GameCompanion";

describe("GameCompanion", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("keys notes by Steam app id or Battle.net seed", () => {
    expect(noteIdFor({ kind: "steam", appId: 620, lastPlayed: 1 })).toBe(620);
    expect(noteIdFor({ kind: "battlenet", seed: 42 })).toBe(42);
  });

  it("renders notes, news, guides and the empty game-screen slot", async () => {
    api.companionFrame.mockResolvedValue({
      note: "try co-op",
      news: [{ title: "Patch notes", url: "https://example.com/p" }],
      guidesUrl: "https://steamcommunity.com/app/620/guides/?browsefilter=toprated",
    });
    render(<GameCompanion target={{ kind: "steam", appId: 620, lastPlayed: 1_700_000_000 }} />);
    await screen.findByDisplayValue("try co-op");
    expect(api.companionFrame).toHaveBeenCalledWith(620, 1_700_000_000, 620);
    expect(screen.getByRole("link", { name: "Patch notes" })).toHaveAttribute(
      "href",
      "https://example.com/p",
    );
    expect(screen.getByRole("link", { name: "Top-rated community guides" })).toBeVisible();
    expect(screen.getByLabelText("Game screen")).toHaveTextContent(
      "Game details land here in a later update.",
    );

    api.companionSetNote.mockResolvedValue("finish it");
    await userEvent.clear(screen.getByLabelText("Game note"));
    await userEvent.type(screen.getByLabelText("Game note"), "finish it");
    await userEvent.click(screen.getByRole("button", { name: "Save note" }));
    expect(api.companionSetNote).toHaveBeenCalledWith(620, "finish it");
    expect(await screen.findByRole("status")).toHaveTextContent("Note saved.");
  });

  it("shows empty news for Steam and unavailable news for Battle.net", async () => {
    api.companionFrame.mockResolvedValue({ note: "", news: [], guidesUrl: null });
    const { rerender } = render(
      <GameCompanion target={{ kind: "steam", appId: 620, lastPlayed: 0 }} />,
    );
    expect(await screen.findByText(NO_NEWS)).toBeVisible();

    api.companionFrame.mockResolvedValue({ note: "", news: [], guidesUrl: null });
    rerender(<GameCompanion target={{ kind: "battlenet", seed: 9 }} />);
    await waitFor(() => expect(api.companionFrame).toHaveBeenCalledWith(9, 0, null));
    expect(await screen.findByText(NEWS_UNAVAILABLE)).toBeVisible();
  });

  it("mounts children into the game-screen slot", async () => {
    api.companionFrame.mockResolvedValue({ note: "", news: [], guidesUrl: null });
    render(
      <GameCompanion target={{ kind: "battlenet", seed: 1 }}>
        <p>WoW screen</p>
      </GameCompanion>,
    );
    expect(await screen.findByText("WoW screen")).toBeVisible();
  });
});
