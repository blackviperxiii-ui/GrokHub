import "@testing-library/jest-dom/vitest";
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { InterestAct, InterestPage, InterestTopic } from "./api";

const api = vi.hoisted(() => ({
  getInterests: vi.fn<() => Promise<InterestPage>>(),
  addInterest: vi.fn<(text: string) => Promise<InterestPage>>(),
  interestAct: vi.fn<(topic: string, act: InterestAct) => Promise<InterestPage>>(),
}));

vi.mock("./api", () => api);

import { INTERESTS_EMPTY, Interests } from "./Interests";

function topic(over: Partial<InterestTopic> = {}): InterestTopic {
  return { topic: "sourdough", pinned: false, muted: false, source: "From chats", ...over };
}

function row(name: string) {
  const list = within(screen.getByRole("list", { name: "Topics" }));
  return within(list.getByRole("listitem", { name }));
}

describe("Interests", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
  });

  it("says what to do when empty and adds a topic", async () => {
    api.getInterests.mockResolvedValue({ topics: [], note: "" });
    api.addInterest.mockResolvedValue({
      topics: [topic({ topic: "air compressors", source: "Added by you" })],
      note: "Pulse will look for air compressors.",
    });
    render(<Interests />);
    expect(await screen.findByText(INTERESTS_EMPTY)).toBeVisible();
    const box = screen.getByLabelText("Add a topic");
    await userEvent.type(box, "air compressors{Enter}");
    expect(api.addInterest).toHaveBeenCalledWith("air compressors");
    expect(box).toHaveValue("");
    expect(screen.getByRole("status")).toHaveTextContent("Pulse will look for air compressors.");
    expect(row("air compressors").getByText("Added by you")).toBeVisible();
  });

  it("pins, mutes and removes a topic", async () => {
    api.getInterests.mockResolvedValue({ topics: [topic()], note: "" });
    api.interestAct
      .mockResolvedValueOnce({ topics: [topic({ pinned: true })], note: "Pinned sourdough." })
      .mockResolvedValueOnce({ topics: [topic({ pinned: true, muted: true })], note: "Muted sourdough." })
      .mockResolvedValueOnce({ topics: [], note: "Removed sourdough. It won't come back." });
    render(<Interests />);
    await screen.findByRole("list", { name: "Topics" });
    expect(row("sourdough").getByText("From chats")).toBeVisible();

    await userEvent.click(row("sourdough").getByRole("button", { name: "Pin" }));
    expect(api.interestAct).toHaveBeenLastCalledWith("sourdough", { kind: "pin", on: true });
    expect(row("sourdough").getByText("Pinned · From chats")).toBeVisible();

    await userEvent.click(row("sourdough").getByRole("button", { name: "Mute" }));
    expect(api.interestAct).toHaveBeenLastCalledWith("sourdough", { kind: "mute", on: true });
    expect(row("sourdough").getByText("Muted")).toBeVisible();
    expect(row("sourdough").getByRole("button", { name: "Unmute" })).toBeVisible();

    await userEvent.click(row("sourdough").getByRole("button", { name: "Remove" }));
    expect(api.interestAct).toHaveBeenLastCalledWith("sourdough", { kind: "remove" });
    expect(screen.getByRole("status")).toHaveTextContent("Removed sourdough. It won't come back.");
    expect(screen.getByText(INTERESTS_EMPTY)).toBeVisible();
  });
});
