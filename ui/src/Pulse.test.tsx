import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PulseAct, PulsePage, PulsePost } from "./api";

const api = vi.hoisted(() => ({
  getPulse: vi.fn<() => Promise<PulsePage>>(),
  pulseAct: vi.fn<(id: string, act: PulseAct) => Promise<PulsePage>>(),
}));

vi.mock("./api", () => api);

import { Pulse, snoozedLine } from "./Pulse";

function post(over: Partial<PulsePost> = {}): PulsePost {
  return {
    id: "post-1",
    source: "Automation",
    title: "Board",
    summary: "summarized 3 cards",
    because: "",
    ago: "2h ago",
    link: "https://example.com/board",
    liked: false,
    ...over,
  };
}

const IDEA = {
  id: "idea-1",
  title: "I can tidy the workboard",
  summary: "Close the done cards",
  kind: "Do",
  action: "close every done card on the workboard",
};

const PAGE: PulsePage = {
  feed: [post()],
  ideas: [{ label: "Productivity", ideas: [IDEA] }],
};

describe("Pulse", () => {
  beforeEach(() => {
    api.getPulse.mockReset();
    api.pulseAct.mockReset();
  });

  it("opens on Feed and switches to Ideas under their headers", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);

    const board = await screen.findByRole("listitem", { name: "Board" });
    expect(board).toHaveTextContent("Automation · 2h ago");
    expect(board).toHaveTextContent("summarized 3 cards");
    expect(board).toHaveTextContent("https://example.com/board");
    expect(screen.queryByText("I can tidy the workboard")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Ideas" }));
    expect(screen.getByRole("heading", { name: "Productivity" })).toBeInTheDocument();
    const idea = screen.getByRole("listitem", { name: "I can tidy the workboard" });
    expect(idea).toHaveTextContent("Close the done cards");
    expect(screen.queryByRole("listitem", { name: "Board" })).not.toBeInTheDocument();
  });

  it("Not this sends the act and shows the page that comes back", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [] });
    render(<Pulse onRun={() => {}} />);

    const board = await screen.findByRole("listitem", { name: "Board" });
    await userEvent.click(within(board).getByRole("button", { name: "Not this" }));
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "notThis");
    expect(await screen.findByRole("status")).toHaveTextContent("Got it. Less like this.");
    expect(screen.queryByRole("listitem", { name: "Board" })).not.toBeInTheDocument();
    expect(
      screen.getByText("No posts yet. The cabin posts here as it notices things."),
    ).toBeInTheDocument();
  });

  it("Like marks the post and is not offered twice", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [post({ liked: true })] });
    render(<Pulse onRun={() => {}} />);

    const board = await screen.findByRole("listitem", { name: "Board" });
    await userEvent.click(within(board).getByRole("button", { name: "Like" }));
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "like");
    expect(await within(board).findByRole("button", { name: "Liked" })).toBeDisabled();
  });

  it("Run notes the idea and hands its action to chat", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "Ideas" }));
    const idea = screen.getByRole("listitem", { name: "I can tidy the workboard" });
    await userEvent.click(within(idea).getByRole("button", { name: "Run" }));
    expect(api.pulseAct).toHaveBeenCalledWith("idea-1", "accept");
    expect(onRun).toHaveBeenCalledWith("close every done card on the workboard");
  });

  it("a failed act says why, reloads, and does not run", async () => {
    api.getPulse
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce({ feed: [], ideas: [] });
    api.pulseAct.mockRejectedValue("That card is gone. The page now shows what is left.");
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "Ideas" }));
    await userEvent.click(screen.getByRole("button", { name: "Run" }));
    expect(await screen.findByRole("status")).toHaveTextContent("That card is gone.");
    expect(onRun).not.toHaveBeenCalled();
    expect(
      await screen.findByText("No ideas yet. I'll add one when I spot something worth doing."),
    ).toBeInTheDocument();
  });

  it("says where Snooze lands from the time of day", () => {
    expect(snoozedLine(new Date(2026, 9, 25, 7, 0))).toBe("Snoozed until 9 AM.");
    expect(snoozedLine(new Date(2026, 9, 25, 14, 30))).toBe("Snoozed until tomorrow 9 AM.");
  });
});
