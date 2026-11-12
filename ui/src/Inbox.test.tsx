import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Decision } from "./api";

const api = vi.hoisted(() => ({
  decisionInbox: vi.fn<() => Promise<Decision[]>>(),
  answerInbox: vi.fn<(chatId: string, id: string, kind: Decision["kind"], approve: boolean) => Promise<string>>(),
  listChats: vi.fn(() => Promise.resolve([{ id: "chat-1", title: "Port fix", created: 0, updated: 0 }])),
}));

vi.mock("./api", () => ({
  decisionInbox: api.decisionInbox,
  answerInbox: api.answerInbox,
  listChats: api.listChats,
}));

import { Inbox, needsLine } from "./Inbox";

const PERMISSION: Decision = {
  id: "c1",
  kind: "permission",
  chatId: "chat-1",
  what: "run_command: cargo test",
  hard: false,
};

const PARK: Decision = {
  id: "park-1",
  kind: "park",
  chatId: "",
  what: "Delete the file report.txt",
  hard: true,
};

describe("Inbox", () => {
  beforeEach(() => {
    api.decisionInbox.mockReset().mockResolvedValue([]);
    api.answerInbox.mockReset().mockResolvedValue("answered");
    api.listChats.mockClear();
  });

  it("stays hidden while nothing waits", async () => {
    const { container } = render(<Inbox onOpenChat={() => {}} />);
    await waitFor(() => expect(api.decisionInbox).toHaveBeenCalled());
    expect(container.firstChild).toBeNull();
  });

  it("shows the needs-attention line and answers a row through its card's path", async () => {
    api.decisionInbox.mockResolvedValue([PERMISSION, PARK]);
    const onOpenChat = vi.fn();
    render(<Inbox onOpenChat={onOpenChat} />);
    expect(await screen.findByText("2 things need a decision. Everything else is on track.")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: /2 things need a decision/ }));
    expect(screen.getByText("run_command: cargo test")).toBeVisible();
    expect(screen.getByText("Port fix")).toBeVisible();
    expect(screen.getByText("Delete the file report.txt")).toBeVisible();
    expect(screen.getByText("Desktop session")).toBeVisible();
    // No Always anywhere in the inbox.
    expect(screen.queryByRole("button", { name: /always/i })).toBeNull();
    const approves = screen.getAllByRole("button", { name: "Approve" });
    await userEvent.click(approves[0]);
    await waitFor(() => expect(api.answerInbox).toHaveBeenCalledWith("chat-1", "c1", "permission", true));
    await userEvent.click(approves[1]);
    await waitFor(() => expect(api.answerInbox).toHaveBeenCalledWith("", "park-1", "park", true));
  });

  it("an elicit row opens its chat, and its Deny cancels the ask", async () => {
    api.decisionInbox.mockResolvedValue([
      { id: "e1", kind: "elicit", chatId: "chat-1", what: "A plan is ready for your OK", hard: false },
    ]);
    const onOpenChat = vi.fn();
    render(<Inbox onOpenChat={onOpenChat} />);
    await userEvent.click(await screen.findByRole("button", { name: /1 thing needs a decision/ }));
    await userEvent.click(screen.getByRole("button", { name: "Open" }));
    expect(onOpenChat).toHaveBeenCalledWith("chat-1");
    expect(api.answerInbox).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(api.answerInbox).toHaveBeenCalledWith("chat-1", "e1", "elicit", false));
  });

  it("words the line like the cabin", () => {
    expect(needsLine(1)).toBe("1 thing needs a decision");
    expect(needsLine(3)).toBe("3 things need a decision");
  });
});
