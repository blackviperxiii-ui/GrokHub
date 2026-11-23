import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings, StageOutcome } from "./api";
import type { ScreenshotOfferView } from "./screenshotOfferApi";

// The composer's screenshot chip inside Chat: Explain attaches the picture
// and fills an empty box, and nothing is sent until the person sends.
const seen = vi.hoisted(() => ({ sent: 0 }));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return {
  ...chatTabApiStubs(),
  quickChips: () => Promise.resolve([]),
  chipOutcome: () => Promise.resolve(true),
  getStartChat: () => Promise.resolve(null),
  loadChat: vi.fn(),
  sendChat: () => {
    seen.sent += 1;
    return new Promise<string>(() => {});
  },
  getSettings: () => Promise.resolve({ hasApiKey: true, approvals: "ask" } as Settings),
  closeChat: () => Promise.resolve(),
  onChatEvent: () => Promise.resolve(() => {}),
  helperQuestions: () => Promise.resolve([]),
  onEventGap: () => Promise.resolve(() => {}),
  onFileDrop: () => Promise.resolve(() => {}),
  removeAttachment: () => Promise.resolve(),
};
});

const PROMPT =
  "Explain what you see in this screenshot. Name the app or screen, the important text, and what the user might want to do next.";

const shots = vi.hoisted(() => ({
  getScreenshotOffer: vi.fn<() => Promise<ScreenshotOfferView>>(),
  dismissScreenshotOffer: vi.fn<() => Promise<ScreenshotOfferView>>(),
  explainScreenshotOffer: vi.fn<(conversationId: string) => Promise<StageOutcome>>(),
  OFFER_ENDED: "That screenshot offer has ended.",
  SHOT_GONE: "That screenshot is gone.",
}));
vi.mock("./screenshotOfferApi", () => shots);
vi.mock("./transport", async (load) => ({
  ...(await load<typeof import("./transport")>()),
  inShell: () => true,
}));

import { Chat } from "./Chat";

const STAGED: StageOutcome = {
  staged: {
    path: "/chat/attach/Screenshot 1.png",
    name: "Screenshot 1.png",
    kind: "image",
    label: "Picture Screenshot 1.png",
    status: null,
  },
  error: null,
};

describe("Chat screenshot chip", () => {
  beforeEach(() => {
    seen.sent = 0;
    shots.getScreenshotOffer.mockReset().mockResolvedValue({
      enabled: true,
      offer: { name: "Screenshot 1.png", label: "Explain this?", dismiss: "Not now", prompt: PROMPT },
    });
    shots.explainScreenshotOffer.mockReset().mockResolvedValue(STAGED);
    shots.dismissScreenshotOffer.mockReset().mockResolvedValue({ enabled: true, offer: null });
  });

  it("Explain attaches the picture and fills the empty box without sending", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Explain this?" }));
    expect(screen.getByLabelText("Message")).toHaveValue(PROMPT);
    const tray = screen.getByRole("group", { name: "Attachments" });
    expect(tray).toHaveTextContent("Picture Screenshot 1.png");
    expect(screen.queryByRole("button", { name: "Explain this?" })).toBeNull();
    expect(seen.sent).toBe(0);
  });

  it("a message already typed is kept", async () => {
    render(<Chat onOpenSettings={() => {}} />);
    await userEvent.type(screen.getByLabelText("Message"), "what is this error");
    await userEvent.click(await screen.findByRole("button", { name: "Explain this?" }));
    expect(screen.getByLabelText("Message")).toHaveValue("what is this error");
    expect(screen.getByRole("group", { name: "Attachments" })).toHaveTextContent("Picture Screenshot 1.png");
    expect(seen.sent).toBe(0);
  });
});
