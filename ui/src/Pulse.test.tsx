import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FeedPrompt, PulseAct, PulsePage, PulsePost } from "./api";

const api = vi.hoisted(() => ({
  getPulse: vi.fn<() => Promise<PulsePage>>(),
  pulseAct: vi.fn<(id: string, act: PulseAct) => Promise<PulsePage>>(),
  pulseDiscuss: vi.fn<(id: string) => Promise<string>>(),
  pulseWhy: vi.fn<(id: string) => Promise<string>>(),
  pulseImage: vi.fn<(name: string) => Promise<string>>(),
  getFeedPrompt: vi.fn<() => Promise<FeedPrompt>>(),
  saveFeedPrompt: vi.fn<(text: string) => Promise<FeedPrompt>>(),
}));

vi.mock("./api", () => api);

import { emojiImage, Snippet } from "./FeedCard";
import {
  FEED_EMPTY,
  MORE_LIKE_NOTE,
  NOT_INTERESTED_NOTE,
  PAGE_SIZE,
  Pulse,
  SAVED_NOTE,
} from "./Pulse";

function post(over: Partial<PulsePost> = {}): PulsePost {
  return {
    id: "post-1",
    source: "Rust Blog",
    title: "Async closures are stable in Rust 1.99",
    summary:
      "Rust 1.99 makes [async closures stable](https://blog.rust-lang.org/1.99.html) and **faster**.",
    because: "",
    icon: "🦀",
    why: "You follow Rust.",
    images: [],
    createdAt: Date.UTC(2026, 9, 26, 15, 4),
    ago: "2h ago",
    link: "https://blog.rust-lang.org/1.99.html",
    loved: false,
    ...over,
  };
}

const TITLE = "Async closures are stable in Rust 1.99";

const IDEA = {
  id: "idea-1",
  title: "I can tidy the workboard",
  icon: "",
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
    for (const fn of Object.values(api)) fn.mockReset();
    api.pulseImage.mockImplementation((name) => Promise.resolve(`data:image/jpeg;base64,${name}`));
  });

  it("opens on Feed and switches to Ideas under their headers", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(card).toHaveTextContent("2h ago");
    expect(screen.queryByText("I can tidy the workboard")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "For You" }));
    expect(screen.getByRole("heading", { name: "Productivity" })).toBeInTheDocument();
    const idea = screen.getByRole("listitem", { name: "I can tidy the workboard" });
    expect(idea).toHaveTextContent("Close the done cards");
    expect(screen.queryByRole("listitem", { name: TITLE })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit feed instructions" })).not.toBeInTheDocument();
  });

  it("a card shows its 3D icon, title and cited snippet, and no old buttons", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    const { container } = render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(container.querySelector(".topic-icon")).toHaveAttribute("src", "/emoji3d/1f980.png");
    expect(within(card).getByText(TITLE)).toHaveClass("feed-title");
    const cite = within(card).getByRole("link", { name: "async closures stable" });
    expect(cite).toHaveAttribute("href", "https://blog.rust-lang.org/1.99.html");
    expect(cite).toHaveAttribute("target", "_blank");
    expect(cite).toHaveAttribute("rel", "noopener noreferrer");
    expect(within(card).getByText("faster").tagName).toBe("STRONG");
    for (const gone of ["Snooze", "Not this", "Dismiss"]) {
      expect(within(card).queryByRole("button", { name: gone })).not.toBeInTheDocument();
    }
    // The raw link line is gone; the cite is the link.
    expect(card).not.toHaveTextContent("https://blog.rust-lang.org/1.99.html");
  });

  it("one image is a hero and two or three are a gallery that opens a lightbox", async () => {
    api.getPulse.mockResolvedValue({
      ...PAGE,
      feed: [
        post({ images: ["aaaaaaaaaaaaaaaa.jpg"] }),
        post({
          id: "post-2",
          title: "Tauri 2.5 ships",
          images: ["bbbbbbbbbbbbbbbb.jpg", "cccccccccccccccc.png", "dddddddddddddddd.webp"],
        }),
      ],
    });
    const { container } = render(<Pulse onRun={() => {}} />);

    const hero = await screen.findByRole("listitem", { name: TITLE });
    await within(hero).findByRole("button", { name: "Open image 1" });
    expect(hero.querySelector(".media")).toHaveClass("hero");
    const gallery = screen.getByRole("listitem", { name: "Tauri 2.5 ships" });
    await within(gallery).findByRole("button", { name: "Open image 3" });
    expect(gallery.querySelector(".media")).toHaveClass("gallery");
    expect(gallery.querySelectorAll(".media img")).toHaveLength(3);
    expect(api.pulseImage).toHaveBeenCalledWith("cccccccccccccccc.png");

    await userEvent.click(within(gallery).getByRole("button", { name: "Open image 2" }));
    const box = screen.getByRole("dialog", { name: "Image" });
    expect(box.querySelector("img")).toHaveAttribute("src", "data:image/jpeg;base64,cccccccccccccccc.png");
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute(
      "download",
      "cccccccccccccccc.png",
    );
    await userEvent.click(within(box).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog", { name: "Image" })).not.toBeInTheDocument();
    expect(container.querySelector(".lightbox")).toBeNull();
  });

  it("the menu shows the date, why, and Delete takes the post away", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseWhy.mockResolvedValue("You follow Rust.");
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [] });
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    await userEvent.click(within(card).getByRole("button", { name: "More" }));
    const menu = within(card).getByRole("menu");
    expect(menu).toHaveTextContent("2026");
    await userEvent.click(within(menu).getByRole("menuitem", { name: "Why I created this" }));
    expect(api.pulseWhy).toHaveBeenCalledWith("post-1");
    expect(await within(card).findByRole("note")).toHaveTextContent("You follow Rust.");

    await userEvent.click(within(card).getByRole("button", { name: "More" }));
    await userEvent.click(within(card).getByRole("menuitem", { name: "Delete" }));
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "delete");
    expect(await screen.findByRole("status")).toHaveTextContent("Deleted.");
    expect(screen.queryByRole("listitem", { name: TITLE })).not.toBeInTheDocument();
    expect(screen.getByText(FEED_EMPTY)).toBeInTheDocument();
  });

  it("Love toggles on once and Discuss hands the post to chat", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [post({ loved: true })] });
    api.pulseDiscuss.mockResolvedValue("Let's talk about this post from my Pulse feed.");
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    const love = within(card).getByRole("button", { name: "Love" });
    expect(love).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(love);
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "love");
    await vi.waitFor(() =>
      expect(within(card).getByRole("button", { name: "Love" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    expect(within(card).getByRole("button", { name: "Love" })).toBeDisabled();

    await userEvent.click(within(card).getByRole("button", { name: "Discuss" }));
    expect(api.pulseDiscuss).toHaveBeenCalledWith("post-1");
    expect(onRun).toHaveBeenCalledWith("Let's talk about this post from my Pulse feed.");
  });

  it("feed instructions open from the sliders, save, and look for the new post", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      api.getPulse
        .mockResolvedValueOnce(PAGE)
        .mockResolvedValue({ ...PAGE, feed: [post({ id: "new", title: "Fresh post" }), post()] });
      api.getFeedPrompt.mockResolvedValue({ text: "Default text", isDefault: true, note: "" });
      api.saveFeedPrompt.mockResolvedValue({ text: "Only Rust news.", isDefault: false, note: "" });
      render(<Pulse onRun={() => {}} />);

      await screen.findByRole("listitem", { name: TITLE });
      await userEvent.click(screen.getByRole("button", { name: "Edit feed instructions" }));
      const box = await screen.findByLabelText("What should your feed cover?");
      await vi.waitFor(() => expect(box).toHaveValue("Default text"));
      await userEvent.clear(box);
      await userEvent.type(box, "Only Rust news.");
      await userEvent.click(screen.getByRole("button", { name: "Save" }));
      expect(api.saveFeedPrompt).toHaveBeenCalledWith("Only Rust news.");
      expect(await screen.findByRole("status")).toHaveTextContent(SAVED_NOTE);
      expect(screen.queryByLabelText("What should your feed cover?")).not.toBeInTheDocument();

      await vi.advanceTimersByTimeAsync(4000);
      expect(await screen.findByRole("listitem", { name: "Fresh post" })).toBeInTheDocument();
      expect(screen.queryByRole("status")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("Cancel closes the instructions without saving", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.getFeedPrompt.mockResolvedValue({ text: "Default text", isDefault: true, note: "" });
    render(<Pulse onRun={() => {}} />);

    await userEvent.click(await screen.findByRole("button", { name: "Edit feed instructions" }));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));
    expect(api.saveFeedPrompt).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog", { name: "Edit feed instructions" })).not.toBeInTheDocument();
  });

  it("shows skeleton rows while loading and the first page of posts", async () => {
    let land: (page: PulsePage) => void = () => {};
    api.getPulse.mockReturnValue(new Promise<PulsePage>((resolve) => (land = resolve)));
    const { container } = render(<Pulse onRun={() => {}} />);
    expect(container.querySelectorAll(".skeleton")).toHaveLength(3);

    const many = Array.from({ length: PAGE_SIZE + 5 }, (_, i) =>
      post({ id: `p${i}`, title: `Post ${i}` }),
    );
    // No IntersectionObserver in this test DOM: every post shows.
    land({ ...PAGE, feed: many });
    expect(await screen.findByRole("listitem", { name: "Post 14" })).toBeInTheDocument();
    expect(container.querySelectorAll(".skeleton")).toHaveLength(0);
  });

  it("a click on the idea row is Let's do it: noted, then its action goes to chat", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    const idea = screen.getByRole("listitem", { name: "I can tidy the workboard" });
    expect(idea).toHaveTextContent("Close the done cards");
    // An idea with no emoji of its own gets the bulb.
    expect(idea.querySelector("img.idea-icon")).toHaveAttribute("src", "/emoji3d/1f4a1.png");
    await userEvent.click(within(idea).getByRole("button", { name: /I can tidy the workboard/ }));
    expect(api.pulseAct).toHaveBeenCalledWith("idea-1", "accept");
    expect(onRun).toHaveBeenCalledWith("close every done card on the workboard");
  });

  it("the idea menu holds Let's do it, More like this and Not interested, and no Snooze", async () => {
    const written = { ...IDEA, id: "idea-2", title: "Rust 1.99 upgrade plan", icon: "🦀" };
    const page: PulsePage = {
      feed: [],
      ideas: [
        { label: "", ideas: [written] },
        { label: "Productivity", ideas: [IDEA] },
      ],
    };
    api.getPulse.mockResolvedValue(page);
    api.pulseAct.mockResolvedValue(page);
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    expect(screen.getAllByRole("heading").map((h) => h.textContent)).toEqual(["Productivity"]);
    const row = screen.getByRole("listitem", { name: "Rust 1.99 upgrade plan" });
    expect(row.querySelector("img.idea-icon")).toHaveAttribute("src", "/emoji3d/1f980.png");
    await userEvent.click(within(row).getByRole("button", { name: "More" }));
    expect(within(row).getAllByRole("menuitem").map((b) => b.textContent)).toEqual([
      "Let's do it",
      "More like this",
      "Not interested",
    ]);
    expect(screen.queryByRole("button", { name: "Snooze" })).toBeNull();
    await userEvent.click(within(row).getByRole("menuitem", { name: "More like this" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "moreLike");
    expect(await screen.findByRole("status")).toHaveTextContent(MORE_LIKE_NOTE);

    await userEvent.click(within(row).getByRole("button", { name: "More" }));
    await userEvent.click(within(row).getByRole("menuitem", { name: "Not interested" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "notInterested");
    expect(await screen.findByRole("status")).toHaveTextContent(NOT_INTERESTED_NOTE);

    await userEvent.click(within(row).getByRole("button", { name: "More" }));
    await userEvent.click(within(row).getByRole("menuitem", { name: "Let's do it" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "accept");
    expect(onRun).toHaveBeenCalledWith("close every done card on the workboard");
  });

  it("a failed act says why, reloads, and does not run", async () => {
    api.getPulse
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce({ feed: [], ideas: [] });
    api.pulseAct.mockRejectedValue("That card is gone. The page now shows what is left.");
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    await userEvent.click(screen.getByRole("button", { name: /I can tidy the workboard/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("That card is gone.");
    expect(onRun).not.toHaveBeenCalled();
    expect(
      await screen.findByText("Nothing for you yet. I'll add one when I spot something worth doing."),
    ).toBeInTheDocument();
  });

  it("keeps the buttons off until the reload after a failed act lands", async () => {
    let land: (page: PulsePage) => void = () => {};
    api.getPulse
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce(PAGE)
      .mockReturnValueOnce(new Promise<PulsePage>((resolve) => (land = resolve)));
    api.pulseAct.mockRejectedValue("MEMORY.md is too large to add to. Nothing was changed.");
    render(<Pulse onRun={vi.fn()} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    await userEvent.click(screen.getByRole("button", { name: /I can tidy the workboard/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("MEMORY.md is too large");
    expect(screen.getByRole("button", { name: /I can tidy the workboard/ })).toBeDisabled();
    land(PAGE);
    await vi.waitFor(() =>
      expect(screen.getByRole("button", { name: /I can tidy the workboard/ })).toBeEnabled(),
    );
  });

  it("maps emoji to the bundled 3D set and renders only links and bold", () => {
    expect(emojiImage("🦀")).toBe("/emoji3d/1f980.png");
    expect(emojiImage("⚙️")).toBe("/emoji3d/2699.png");
    expect(emojiImage("🦄")).toBeNull();
    render(<Snippet text={"<b>raw</b> [bad](javascript:alert(1)) and [ok](https://a.example/x)"} />);
    expect(screen.getByText(/<b>raw<\/b>/)).toBeInTheDocument();
    expect(screen.getAllByRole("link")).toHaveLength(1);
    expect(screen.getByRole("link", { name: "ok" })).toHaveAttribute("href", "https://a.example/x");
  });
});
