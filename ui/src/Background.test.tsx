import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BackgroundPage } from "./api";

const api = vi.hoisted(() => ({
  getBackground: vi.fn<() => Promise<BackgroundPage>>(),
}));

vi.mock("./api", () => api);

import { Background } from "./Background";

const RUNNER =
  "The Loadout cabin runs automations on their schedule while it's open. Here a job runs when you choose Run now.";

describe("Background", () => {
  beforeEach(() => {
    api.getBackground.mockReset();
  });

  it("says who runs jobs, the next one due and when each last ran", async () => {
    api.getBackground.mockResolvedValue({
      runner: RUNNER,
      nextDue: "Build check · Next check in 12m",
      quiet: "Quiet hours 22:00–07:00",
      budget: "Daily token budget: Off",
      rows: [
        { id: "a", name: "Build check", next: "Next check in 12m", last: "Hasn't run yet", health: null },
        { id: "b", name: "Inbox", next: "Paused", last: "Last ran 2h ago", health: "Last run failed" },
      ],
    });
    render(<Background />);
    expect(await screen.findByText("Next due: Build check · Next check in 12m")).toBeInTheDocument();
    expect(screen.getByText(RUNNER)).toBeInTheDocument();
    expect(screen.getByText("Quiet hours 22:00–07:00 · Daily token budget: Off")).toBeInTheDocument();
    expect(screen.getByText("Paused · Last ran 2h ago")).toBeInTheDocument();
    expect(screen.getByText("Last run failed")).toHaveClass("warn");
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("with no jobs says nothing is due, and reads again on focus", async () => {
    api.getBackground.mockResolvedValue({
      runner: RUNNER,
      nextDue: null,
      quiet: "No quiet hours",
      budget: "Daily token budget: Off",
      rows: [],
    });
    render(<Background />);
    expect(await screen.findByText("No automation is on, so nothing is due.")).toBeInTheDocument();
    fireEvent.focus(window);
    expect(api.getBackground).toHaveBeenCalledTimes(2);
  });

  it("shows a read error", async () => {
    api.getBackground.mockRejectedValue("automations.json: expected value");
    render(<Background />);
    expect(await screen.findByText("automations.json: expected value")).toBeInTheDocument();
  });
});
