import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PulsePost } from "./api";

const api = vi.hoisted(() => ({ pulseImage: vi.fn<(name: string) => Promise<string>>() }));
const ext = vi.hoisted(() => ({ openExternal: vi.fn<(url: string) => Promise<void>>() }));

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./api")>()),
  ...api,
}));
vi.mock("./external", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./external")>()),
  ...ext,
}));

import { CardButtons, FeedCard, type FeedCardProps } from "./FeedCard";

beforeEach(() => {
  api.pulseImage.mockReset();
  api.pulseImage.mockImplementation((name) => Promise.resolve(`data:image/jpeg;base64,${name}`));
  ext.openExternal.mockReset();
  ext.openExternal.mockResolvedValue();
});

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function reduced(matches: boolean) {
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches, addEventListener: () => {}, removeEventListener: () => {} })),
  );
}

function row(liked: boolean, onLike = vi.fn()) {
  return render(
    <CardButtons liked={liked} busy={false} onLike={onLike} onDislike={vi.fn()}>
      <button type="button">Discuss</button>
    </CardButtons>,
  );
}

describe("CardButtons like → heart", () => {
  it("shows ♡ Like when not liked, and Like is the first button", () => {
    reduced(false);
    row(false);
    const group = screen.getByRole("group", { name: "Card actions" });
    const buttons = within(group).getAllByRole("button");
    expect(buttons[0]).toHaveAccessibleName("Like");
    expect(buttons[0]).toHaveAttribute("aria-pressed", "false");
    expect(buttons[0].textContent).toBe("♡ Like");
  });

  it("a liked card shows only the filled heart and stays pressed", () => {
    reduced(false);
    row(true);
    const like = screen.getByRole("button", { name: "Like" });
    expect(like).toHaveAttribute("aria-pressed", "true");
    expect(like.textContent).toBe("♥");
    expect(like).not.toHaveClass("card-like-pop");
  });

  it("tapping Like adds the pop class; once liked the heart stays without pop; unlike has no pop", async () => {
    reduced(false);
    const onLike = vi.fn();
    const { rerender } = row(false, onLike);
    const like = screen.getByRole("button", { name: "Like" });
    await userEvent.click(like);
    expect(onLike).toHaveBeenCalledTimes(1);
    expect(like).toHaveClass("card-like-pop");
    expect(like.textContent).toBe("♥");

    rerender(
      <CardButtons liked={true} busy={false} onLike={onLike} onDislike={vi.fn()}>
        <button type="button">Discuss</button>
      </CardButtons>,
    );
    const heart = screen.getByRole("button", { name: "Like" });
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart.textContent).toBe("♥");
    expect(heart).not.toHaveClass("card-like-pop");
    await userEvent.click(heart);
    expect(onLike).toHaveBeenCalledTimes(2);
    expect(heart).not.toHaveClass("card-like-pop");
  });

  it("reduced motion skips the pop class and swaps state instantly", async () => {
    reduced(true);
    const onLike = vi.fn();
    const { rerender } = row(false, onLike);
    const like = screen.getByRole("button", { name: "Like" });
    await userEvent.click(like);
    expect(onLike).toHaveBeenCalledTimes(1);
    expect(like).not.toHaveClass("card-like-pop");
    rerender(
      <CardButtons liked={true} busy={false} onLike={onLike} onDislike={vi.fn()}>
        <button type="button">Discuss</button>
      </CardButtons>,
    );
    const heart = screen.getByRole("button", { name: "Like" });
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart.textContent).toBe("♥");
    expect(heart).not.toHaveClass("card-like-pop");
  });
});

const LINK = "https://blog.rust-lang.org/1.99.html";
const TITLE = "Async closures are stable in Rust 1.99";

function post(over: Partial<PulsePost> = {}): PulsePost {
  return {
    id: "post-1",
    source: "Rust Blog",
    title: TITLE,
    summary: "Rust 1.99 makes [async closures stable](https://blog.rust-lang.org/1.99.html) and **faster**.",
    because: "",
    icon: "🦀",
    why: "",
    images: [],
    createdAt: Date.UTC(2026, 9, 26, 15, 4),
    ago: "2h ago",
    link: LINK,
    model: "",
    loved: false,
    verdict: null,
    more: "",
    items: [],
    chips: [],
    snooze: false,
    delete: false,
    save: false,
    saved: false,
    ...over,
  };
}

function card(p: PulsePost, props: Partial<FeedCardProps> = {}) {
  return render(
    <ul>
      <FeedCard
        post={p}
        busy={false}
        onLove={vi.fn()}
        onDiscuss={vi.fn()}
        onDislike={vi.fn()}
        {...props}
      />
    </ul>,
  );
}

const REPORT = "It stopped with a segfault each time.\n\n**What was checked**\n- 4 crash reports since 9 AM";

describe("FeedCard", () => {
  it("a title click opens the source once and counts as opened", async () => {
    const onOpen = vi.fn();
    card(post(), { onOpen });
    const title = screen.getByRole("link", { name: TITLE });
    expect(title).toHaveAttribute("href", LINK);
    expect(title).toHaveClass("feed-title");
    await userEvent.click(title);
    expect(ext.openExternal.mock.calls).toEqual([[LINK]]);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("a title with no web link is plain text", () => {
    card(post({ link: "" }));
    expect(screen.queryByRole("link", { name: TITLE })).toBeNull();
    expect(screen.getByText(TITLE).tagName).toBe("DIV");
  });

  it("a source page that will not open says why", async () => {
    ext.openExternal.mockRejectedValue("No browser to open it in.");
    const onOpenFailed = vi.fn();
    card(post(), { onOpen: vi.fn(), onOpenFailed });
    await userEvent.click(screen.getByRole("button", { name: "Read it" }));
    await vi.waitFor(() => expect(onOpenFailed).toHaveBeenCalledWith("No browser to open it in."));
  });

  it("Show more opens the report from the keyboard and becomes Show less", async () => {
    card(post({ more: REPORT }));
    const more = screen.getByRole("button", { name: "Show more" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(more).toHaveAttribute("aria-controls", "more-post-1");
    expect(screen.queryByText("What was checked")).toBeNull();
    more.focus();
    await userEvent.keyboard("{Enter}");
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(more).toHaveAccessibleName("Show less");
    const region = screen.getByRole("region", { name: `More about ${TITLE}` });
    expect(region).toHaveAttribute("id", "more-post-1");
    expect(region).toHaveTextContent("What was checked");
    expect(region).toHaveTextContent("4 crash reports since 9 AM");
    await userEvent.keyboard(" ");
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("What was checked")).toBeNull();
  });

  it("a click on the snippet text opens the card; a cite link only opens its page", async () => {
    card(post({ more: REPORT }));
    const more = screen.getByRole("button", { name: "Show more" });
    await userEvent.click(screen.getByRole("link", { name: "async closures stable" }));
    expect(ext.openExternal.mock.calls).toEqual([[LINK]]);
    expect(more).toHaveAttribute("aria-expanded", "false");
    await userEvent.click(screen.getByText(/and/, { selector: ".snippet" }));
    expect(more).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: `More about ${TITLE}` })).toHaveTextContent(
      "What was checked",
    );
  });

  it("shows why the card is here behind Show more", async () => {
    card(post({ why: "You follow Rust." }));
    await userEvent.click(screen.getByRole("button", { name: "Show more" }));
    expect(screen.getByRole("region", { name: `More about ${TITLE}` }).textContent).toBe(
      "Why you're seeing thisYou follow Rust.",
    );
  });

  it("a link with no image gets a tile with its host; no link and no image gets no lead", () => {
    const { container, unmount } = card(post({ icon: "🦄", images: [] }));
    const lead = container.querySelector(".feed-lead")!;
    const tile = lead.querySelector(".feed-tile")!;
    expect(tile).toHaveAttribute("aria-hidden", "true");
    expect(tile.querySelector(".feed-tile-host")).toHaveTextContent("blog.rust-lang.org");
    expect(tile.textContent).toBe("🦄blog.rust-lang.org");
    expect(lead.querySelector("img")).toBeNull();
    expect(lead.querySelector("button")).toBeNull();
    expect(api.pulseImage).not.toHaveBeenCalled();
    unmount();

    const bare = card(post({ link: "", images: [] }));
    expect(bare.container.querySelector(".feed-lead")).toBeNull();
  });

  it("a lead image that fails to load falls back to the tile", async () => {
    api.pulseImage.mockRejectedValue("Not a Pulse image.");
    const { container } = card(post({ images: ["1111111111111111.jpg"] }));
    await vi.waitFor(() =>
      expect(container.querySelector(".feed-lead .feed-tile-host")).toHaveTextContent(
        "blog.rust-lang.org",
      ),
    );
    expect(api.pulseImage.mock.calls).toEqual([["1111111111111111.jpg"]]);
    expect(screen.queryByRole("button", { name: "Open image 1" })).toBeNull();
  });

  it("a failed image keeps the others' numbers and names", async () => {
    api.pulseImage.mockImplementation((name) =>
      name === "3333333333333333.jpg"
        ? Promise.reject("Not a Pulse image.")
        : Promise.resolve(`data:image/webp;base64,${name}`),
    );
    card(post({ images: ["2222222222222222.jpg", "3333333333333333.jpg", "4444444444444444.img"] }));
    await screen.findByRole("button", { name: "Open image 1" });
    await userEvent.click(screen.getByRole("button", { name: "Show more" }));
    await screen.findByRole("button", { name: "Open image 3" });
    expect(screen.queryByRole("button", { name: "Open image 2" })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "Open image 3" }));
    const box = screen.getByRole("dialog", { name: "Image" });
    expect(box.querySelector("img")).toHaveAttribute("src", "data:image/webp;base64,4444444444444444.img");
    expect(within(box).getByRole("link", { name: "Download" })).toHaveAttribute(
      "download",
      "4444444444444444.webp",
    );
  });

  it("a self-review card starts open", () => {
    card(post({ link: "", more: REPORT, review: "Apply change" }), { onReview: vi.fn() });
    const less = screen.getByRole("button", { name: "Show less" });
    expect(less).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("region", { name: `More about ${TITLE}` })).toHaveTextContent(
      "What was checked",
    );
  });

  it("a card with nothing behind it has no Show more, and a body click does nothing", async () => {
    const { container } = card(post());
    expect(screen.queryByRole("button", { name: "Show more" })).toBeNull();
    expect(screen.queryByRole("region")).toBeNull();
    await userEvent.click(screen.getByText(/and/, { selector: ".snippet" }));
    expect(container.querySelector(".feed-card")).not.toHaveClass("open");
  });
});
