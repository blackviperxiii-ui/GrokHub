import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatEnvelope, ChatEvent, Settings } from "./api";

// How often Chat draws, subscribes and posts: a question card while a reply
// streams under it, and the chat event subscription.
const seen = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  subscriptions: 0,
  cardRenders: 0,
  /** The conversation each message went to: the id its events must carry. */
  sentTo: [] as string[],
}));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
  ...chatTabApiStubs(),
  quickChips: () => Promise.resolve([]),
  chipOutcome: () => Promise.resolve(true),
  // No chat saved from an earlier launch.
  getStartChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  // The turn stays open, so every event below lands in it.
  sendChat: (conversationId: string) => {
    seen.sentTo.push(conversationId);
    return new Promise<string>(() => {});
  },
  getSettings: () => Promise.resolve({ hasApiKey: true, approvals: "ask" } as Settings),
  closeChat: () => Promise.resolve(),
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    seen.subscriptions += 1;
    seen.handler = handler;
    return Promise.resolve(() => {});
  },
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  onFileDrop: () => Promise.resolve(() => {}),
};
});

vi.mock("./Modes", async (load) => {
  const real = await load<typeof import("./Modes")>();
  return {
    ...real,
    ElicitCard: (props: ComponentProps<typeof real.ElicitCard>) => {
      seen.cardRenders += 1;
      return <real.ElicitCard {...props} />;
    },
  };
});

import { Chat } from "./Chat";

const QUESTION: ChatEvent = {
  kind: "elicit",
  id: "ask-1",
  server: "Question",
  message: "Which color?",
  mode: "form",
  url: "",
  fieldName: "answer",
  fieldTitle: "Your answer",
  secret: false,
};

describe("Chat renders and subscriptions", () => {
  beforeEach(() => {
    seen.handler = null;
    seen.subscriptions = 0;
    seen.cardRenders = 0;
    seen.sentTo.length = 0;
  });

  it("draws a question card once while 50 chunks of a reply stream in", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "pick a color{Enter}");
    await waitFor(() => expect(seen.sentTo).toHaveLength(1));
    const conversationId = seen.sentTo[0];

    act(() => seen.handler?.({ conversationId, event: QUESTION }));
    expect(screen.getByText("Which color?")).toBeInTheDocument();
    for (let i = 0; i < 50; i += 1) {
      act(() => seen.handler?.({ conversationId, event: { kind: "text", text: `${i} ` } }));
    }

    expect(screen.getByText(/^0 1 2 .* 49$/)).toBeInTheDocument();
    expect(seen.cardRenders).toBe(1);
  });

  it("a prefill without a conversation fills the box and keeps the one subscription", () => {
    const { rerender } = render(<Chat onOpenSettings={() => {}} prefill={{ text: "first", n: 1 }} />);
    rerender(<Chat onOpenSettings={() => {}} prefill={{ text: "second", n: 2 }} />);
    expect(screen.getByLabelText("Message")).toHaveValue("second");
    expect(seen.subscriptions).toBe(1);
  });

  it("a prefill for another conversation subscribes to that one once", () => {
    const { rerender } = render(<Chat onOpenSettings={() => {}} />);
    rerender(
      <Chat onOpenSettings={() => {}} prefill={{ text: "Mission: Q", n: 1, conversationId: "shell-m1" }} />,
    );
    rerender(
      <Chat onOpenSettings={() => {}} prefill={{ text: "Mission: Q again", n: 2, conversationId: "shell-m1" }} />,
    );
    expect(screen.getByLabelText("Message")).toHaveValue("Mission: Q again");
    expect(seen.subscriptions).toBe(2);
  });
});
