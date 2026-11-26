import "@testing-library/jest-dom/vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionsBoardPage } from "./api";

const api = vi.hoisted(() => ({
  getSessionsBoard: vi.fn<() => Promise<SessionsBoardPage>>(),
  sessionsBoardStamp: vi.fn<() => Promise<number>>(),
  sessionsBoardPin: vi.fn(),
  sessionsBoardReshape: vi.fn(),
}));

vi.mock("./api", () => api);

const external = vi.hoisted(() => ({
  openExternal: vi.fn<(url: string) => Promise<void>>(),
}));

vi.mock("./external", () => external);

import { SESSIONS_EMPTY, SessionsBoard } from "./SessionsBoard";

function page(over: Partial<SessionsBoardPage> = {}): SessionsBoardPage {
  return {
    name: "Sessions",
    filter: "everyone",
    query: "",
    stamp: 1,
    total: 1,
    shown: 1,
    columns: [
      {
        id: "working",
        label: "Working",
        rule: "running",
        fallback: false,
        tiles: [
          {
            id: "shell-a",
            title: "Ship Sessions board",
            preview: "Working on placement rules",
            run: "running",
            prs: [{ url: "https://github.com/o/r/pull/1", state: "open" }],
            updatedMs: 1000,
            pinned: false,
            columnId: "working",
          },
        ],
      },
      {
        id: "done",
        label: "Done",
        rule: "idle",
        fallback: true,
        tiles: [],
      },
    ],
    ...over,
  };
}

describe("SessionsBoard", () => {
  beforeEach(() => {
    api.getSessionsBoard.mockReset();
    api.sessionsBoardStamp.mockReset();
    api.sessionsBoardPin.mockReset();
    api.sessionsBoardReshape.mockReset();
    api.getSessionsBoard.mockResolvedValue(page());
    api.sessionsBoardStamp.mockResolvedValue(1);
    external.openExternal.mockReset();
    external.openExternal.mockResolvedValue(undefined);
  });

  it("opens a PR link in the system browser", async () => {
    render(<SessionsBoard onOpenChat={() => {}} />);
    await userEvent.click(await screen.findByRole("link", { name: /PR · open/i }));
    expect(external.openExternal).toHaveBeenCalledTimes(1);
    expect(external.openExternal).toHaveBeenCalledWith("https://github.com/o/r/pull/1");
  });

  it("background reloads keep the applied query, not text still being typed", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      render(<SessionsBoard onOpenChat={() => {}} />);
      await screen.findByText("Ship Sessions board");
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      await user.type(screen.getByLabelText("Filter by title"), "half typed");
      api.getSessionsBoard.mockClear();
      api.sessionsBoardStamp.mockResolvedValue(2);
      await vi.advanceTimersByTimeAsync(3100);
      await waitFor(() => expect(api.getSessionsBoard).toHaveBeenCalled());
      expect(api.getSessionsBoard.mock.calls[0]).toEqual(["everyone", ""]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows run-status tiles and PR links from the page", async () => {
    const open = vi.fn();
    render(<SessionsBoard onOpenChat={open} />);
    expect(await screen.findByText("Ship Sessions board")).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Working" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /PR · open/i })).toHaveAttribute(
      "href",
      "https://github.com/o/r/pull/1",
    );
    await userEvent.click(screen.getByRole("button", { name: /Ship Sessions board/i }));
    expect(open).toHaveBeenCalledWith("shell-a");
  });

  it("asks the Board agent to reshape columns", async () => {
    api.sessionsBoardReshape.mockResolvedValue({
      note: "Renamed working to Active.",
      page: page({
        columns: [
          {
            id: "working",
            label: "Active",
            rule: "running",
            fallback: false,
            tiles: [],
          },
        ],
        shown: 0,
        total: 0,
      }),
    });
    render(<SessionsBoard onOpenChat={() => {}} />);
    await screen.findByText("Ship Sessions board");
    await userEvent.type(
      screen.getByLabelText("Board agent"),
      "rename working to Active",
    );
    await userEvent.click(screen.getByRole("button", { name: "Reshape" }));
    await waitFor(() => expect(api.sessionsBoardReshape).toHaveBeenCalled());
    expect(api.sessionsBoardReshape).toHaveBeenCalledWith(
      "rename working to Active",
      "everyone",
      "",
    );
    expect(await screen.findByText("Renamed working to Active.")).toBeInTheDocument();
    expect(screen.getByText(SESSIONS_EMPTY)).toBeInTheDocument();
  });
});
