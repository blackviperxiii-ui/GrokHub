import "@testing-library/jest-dom/vitest";
import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { StageOutcome } from "./api";
import type { ScreenshotOfferView } from "./screenshotOfferApi";

const api = vi.hoisted(() => ({
  getScreenshotOffer: vi.fn<() => Promise<ScreenshotOfferView>>(),
  dismissScreenshotOffer: vi.fn<() => Promise<ScreenshotOfferView>>(),
  explainScreenshotOffer: vi.fn<(conversationId: string) => Promise<StageOutcome>>(),
  OFFER_ENDED: "That screenshot offer has ended.",
  SHOT_GONE: "That screenshot is gone.",
}));
const shell = vi.hoisted(() => ({ inShell: vi.fn(() => true) }));

vi.mock("./screenshotOfferApi", () => api);
vi.mock("./transport", () => shell);

import { SHOT_POLL_MS, ScreenshotOffer } from "./ScreenshotOffer";

const PROMPT =
  "Explain what you see in this screenshot. Name the app or screen, the important text, and what the user might want to do next.";

const OFFERED: ScreenshotOfferView = {
  enabled: true,
  offer: { name: "Screenshot 1.png", label: "Explain this?", dismiss: "Not now", prompt: PROMPT },
};
const NONE: ScreenshotOfferView = { enabled: true, offer: null };
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

function chip(over: { remote?: boolean; active?: boolean; busy?: boolean } = {}) {
  const onAttached = vi.fn<(outcome: StageOutcome, prompt: string) => void>();
  render(
    <ScreenshotOffer
      conversationId="c1"
      active={over.active ?? true}
      remote={over.remote ?? false}
      busy={over.busy ?? false}
      onAttached={onAttached}
    />,
  );
  return onAttached;
}

describe("ScreenshotOffer", () => {
  beforeEach(() => {
    api.getScreenshotOffer.mockReset().mockResolvedValue(OFFERED);
    api.dismissScreenshotOffer.mockReset().mockResolvedValue(NONE);
    api.explainScreenshotOffer.mockReset().mockResolvedValue(STAGED);
    shell.inShell.mockReturnValue(true);
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows Explain this? and Not now for a fresh screenshot", async () => {
    chip();
    const explain = await screen.findByRole("button", { name: "Explain this?" });
    expect(explain).toHaveAttribute("title", "Screenshot 1.png");
    expect(screen.getByRole("button", { name: "Not now" })).toBeEnabled();
    expect(screen.getByRole("group", { name: "New screenshot" })).toBeInTheDocument();
  });

  it("a tap attaches the screenshot and hands back the prompt without sending", async () => {
    const onAttached = chip();
    await userEvent.click(await screen.findByRole("button", { name: "Explain this?" }));
    expect(api.explainScreenshotOffer).toHaveBeenCalledWith("c1");
    expect(onAttached).toHaveBeenCalledTimes(1);
    expect(onAttached).toHaveBeenCalledWith(STAGED, PROMPT);
    expect(screen.queryByRole("button", { name: "Explain this?" })).toBeNull();
  });

  it("a full tray keeps the chip up and says why", async () => {
    const refused: StageOutcome = { staged: null, error: "Screenshot 1.png: too many attachments" };
    api.explainScreenshotOffer.mockResolvedValue(refused);
    const onAttached = chip();
    await userEvent.click(await screen.findByRole("button", { name: "Explain this?" }));
    expect(onAttached).toHaveBeenCalledWith(refused, PROMPT);
    expect(screen.getByRole("status")).toHaveTextContent("Screenshot 1.png: too many attachments");
    expect(screen.getByRole("button", { name: "Explain this?" })).toBeEnabled();
  });

  it("a screenshot deleted before the tap takes the chip down", async () => {
    api.explainScreenshotOffer.mockRejectedValue("That screenshot is gone.");
    const onAttached = chip();
    await userEvent.click(await screen.findByRole("button", { name: "Explain this?" }));
    expect(onAttached).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Explain this?" })).toBeNull();
  });

  it("Not now takes the chip down and tells the desktop", async () => {
    chip();
    await userEvent.click(await screen.findByRole("button", { name: "Not now" }));
    expect(api.dismissScreenshotOffer).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("group", { name: "New screenshot" })).toBeNull();
  });

  it("shows nothing when there is no offer, and Explain waits while a reply runs", async () => {
    api.getScreenshotOffer.mockResolvedValue(NONE);
    const { unmount } = render(
      <ScreenshotOffer conversationId="c1" active remote={false} busy={false} onAttached={() => {}} />,
    );
    await act(async () => {});
    expect(screen.queryByRole("group", { name: "New screenshot" })).toBeNull();
    unmount();

    api.getScreenshotOffer.mockResolvedValue(OFFERED);
    chip({ busy: true });
    expect(await screen.findByRole("button", { name: "Explain this?" })).toBeDisabled();
  });

  it("never looks from the web app, a hidden Chat tab or outside the desktop app", async () => {
    chip({ remote: true });
    chip({ active: false });
    shell.inShell.mockReturnValue(false);
    chip();
    await act(async () => {});
    expect(api.getScreenshotOffer).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Explain this?" })).toBeNull();
  });

  it("looks again every 30 seconds and when the window gets focus", async () => {
    vi.useFakeTimers();
    api.getScreenshotOffer.mockResolvedValue(NONE);
    chip();
    await act(async () => {});
    expect(api.getScreenshotOffer).toHaveBeenCalledTimes(1);
    expect(SHOT_POLL_MS).toBe(30_000);
    await act(async () => {
      vi.advanceTimersByTime(30_000);
    });
    expect(api.getScreenshotOffer).toHaveBeenCalledTimes(2);
    api.getScreenshotOffer.mockResolvedValue(OFFERED);
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(api.getScreenshotOffer).toHaveBeenCalledTimes(3);
    expect(screen.getByRole("button", { name: "Explain this?" })).toBeInTheDocument();
  });
});
