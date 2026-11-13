import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { BoardCard, BoardPage, BoardSay, ChatEnvelope, Transcript } from "./api";

const api = vi.hoisted(() => ({
  workboardSay:
    vi.fn<(id: string, text: string, conversationId: string) => Promise<BoardSay>>(),
  sendChat: vi.fn<(id: string, text: string) => Promise<string>>(),
  cancelChat: vi.fn<(id: string) => Promise<void>>(),
  loadChat: vi.fn<(id: string, limit?: number) => Promise<Transcript>>(),
  answerPermission: vi.fn<(id: string, call: string, answer: string) => Promise<void>>(),
  onChatEvent: vi.fn<(h: (e: ChatEnvelope) => void) => Promise<() => void>>(),
}));

vi.mock("./api", () => api);
vi.mock("./Chat", () => ({ newConversationId: () => "shell-new" }));

import { CardChat, chatHint, ownChat } from "./WorkboardChat";

function card(over: Partial<BoardCard> = {}): BoardCard {
  return {
    id: "w-1",
    title: "Fix the printer",
    detail: "",
    notes: "",
    priority: "",
    fresh: false,
    report: "",
    result: "",
    owner: "Agent",
    kind: null,
    leftOff: null,
    steps: [],
    checkpoint: null,
    progress: [0, 0],
    canResume: false,
    running: false,
    chat: null,
    updatedMs: 0,
    archivedMs: 0,
    queued: null,
    flow: null,
    userSteps: false,
    stepsText: "",
    ...over,
  };
}

const PAGE = { columns: [], archived: [], stamp: 2, swept: null, queueNote: null } as BoardPage;

let emit: (e: ChatEnvelope) => void = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  api.loadChat.mockResolvedValue({ id: "", title: "", lines: [], earlier: 0 });
  api.onChatEvent.mockImplementation(async (h) => {
    emit = h;
    return () => {};
  });
  api.cancelChat.mockResolvedValue();
  api.answerPermission.mockResolvedValue();
});

describe("ownChat and chatHint", () => {
  it("keeps only this app's chats and words the hint as the cabin does", () => {
    expect(ownChat(card({ chat: "shell-1" }))).toBe("shell-1");
    expect(ownChat(card({ chat: "cabin-1" }))).toBeNull();
    expect(chatHint(true, false)).toBe("Reply to the agent…");
    expect(chatHint(false, true)).toBe("Ask the agent about this card…");
    expect(chatHint(false, false)).toBe("Tell the agent how to work on this…");
  });
});

describe("CardChat", () => {
  it("Work on it starts the card in a new chat and shows Stop while the turn runs", async () => {
    let finish: (stop: string) => void = () => {};
    api.workboardSay.mockResolvedValue({ conversationId: "shell-new", message: "Work on this", page: PAGE });
    api.sendChat.mockImplementation(() => new Promise((r) => (finish = r)));
    const onPage = vi.fn();
    render(<CardChat card={card()} done={false} onPage={onPage} onError={vi.fn()} />);

    await userEvent.type(screen.getByLabelText("Message the card's agent"), "start with the driver");
    await userEvent.click(screen.getByRole("button", { name: "Work on it" }));

    expect(api.workboardSay).toHaveBeenCalledWith("w-1", "start with the driver", "shell-new");
    expect(onPage).toHaveBeenCalledWith(PAGE);
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith("shell-new", "Work on this"));
    expect(screen.getByText("start with the driver")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(api.cancelChat).toHaveBeenCalledWith("shell-new");

    await act(async () => finish("cancelled"));
    expect(await screen.findByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("streams the reply, asks for an OK and answers it, and Enter sends", async () => {
    api.loadChat.mockResolvedValue({
      id: "shell-1",
      title: "",
      lines: [{ role: "assistant", text: "Driver found." }],
      earlier: 0,
    });
    api.workboardSay.mockResolvedValue({ conversationId: "shell-1", message: "go on", page: PAGE });
    api.sendChat.mockResolvedValue("end_turn");
    render(
      <CardChat card={card({ chat: "shell-1", running: true })} done={false} onPage={vi.fn()} onError={vi.fn()} />,
    );
    expect(await screen.findByText("Driver found.")).toBeInTheDocument();
    expect(api.loadChat).toHaveBeenCalledWith("shell-1", 12);
    expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();

    await waitFor(() => expect(api.onChatEvent).toHaveBeenCalled());
    act(() => emit({ conversationId: "shell-1", event: { kind: "text", text: "Installing" } }));
    act(() => emit({ conversationId: "other", event: { kind: "text", text: "not mine" } }));
    expect(screen.getByText("Installing")).toBeInTheDocument();
    expect(screen.queryByText("not mine")).not.toBeInTheDocument();

    act(() =>
      emit({
        conversationId: "shell-1",
        event: { kind: "permission", id: "c1", title: "Run the installer", action: "", reason: "" },
      }),
    );
    await userEvent.click(screen.getByRole("button", { name: "Allow" }));
    expect(api.answerPermission).toHaveBeenCalledWith("shell-1", "c1", "allow");
    expect(screen.queryByText(/Run the installer/)).not.toBeInTheDocument();
  });

  it("Enter sends a reply on a linked card and Shift+Enter does not", async () => {
    api.workboardSay.mockResolvedValue({ conversationId: "shell-1", message: "go on", page: PAGE });
    api.sendChat.mockResolvedValue("end_turn");
    render(<CardChat card={card({ chat: "shell-1" })} done={false} onPage={vi.fn()} onError={vi.fn()} />);
    const box = screen.getByLabelText("Message the card's agent");
    expect(box).toHaveAttribute("placeholder", "Reply to the agent…");
    await userEvent.type(box, "go{Shift>}{Enter}{/Shift}");
    expect(api.workboardSay).not.toHaveBeenCalled();
    await userEvent.type(box, " on{Enter}");
    await waitFor(() => expect(api.workboardSay).toHaveBeenCalledWith("w-1", "go\n on", "shell-new"));
    await waitFor(() => expect(api.sendChat).toHaveBeenCalledWith("shell-1", "go on"));
  });
});
