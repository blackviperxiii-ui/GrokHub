import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  SavedFind,
  FeedPrompt,
  FixView,
  PulseAct,
  PulseIdea,
  PulsePage,
  PulsePost,
  PulseReminded,
  PulseSeed,
} from "./api";

const api = vi.hoisted(() => ({
  getPulse: vi.fn<() => Promise<PulsePage>>(),
  pulseAct: vi.fn<(id: string, act: PulseAct) => Promise<PulsePage>>(),
  pulseDiscuss: vi.fn<(id: string) => Promise<PulseSeed>>(),
  pulseOffer: vi.fn<(id: string, index: number) => Promise<PulseSeed>>(),
  pulseRemind: vi.fn<(id: string) => Promise<PulseReminded>>(),
  pulseWhy: vi.fn<(id: string) => Promise<string>>(),
  pulseImage: vi.fn<(name: string) => Promise<string>>(),
  pulseFix: vi.fn<(id: string) => Promise<FixView>>(),
  getFeedPrompt: vi.fn<() => Promise<FeedPrompt>>(),
  openUrl: vi.fn<(url: string) => Promise<void>>(),
  saveFeedPrompt: vi.fn<(text: string) => Promise<FeedPrompt>>(),
  pulseSaved: vi.fn<() => Promise<SavedFind[]>>(),
  pulseUnsave: vi.fn<(url: string) => Promise<SavedFind[]>>(),
}));

vi.mock("./api", () => api);

import { emojiImage, Snippet } from "./FeedCard";
import {
  ADD_INTERESTS,
  DAY_END,
  FEED_EMPTY,
  IGNORED_NOTE,
  MORE_LIKE_NOTE,
  NOT_INTERESTED_NOTE,
  DWELL_MS,
  DWELL_RATIO,
  PAGE_SIZE,
  Pulse,
  REFRESH_MS,
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

const TITLE = "Async closures are stable in Rust 1.99";

const IDEA: PulseIdea = {
  id: "idea-1",
  title: "I can tidy the workboard",
  icon: "",
  summary: "Close the done cards",
  kind: "Do",
  action: "close every done card on the workboard",
  verdict: null,
  fix: "",
  more: "",
  chips: [],
  loved: false,
};

const PAGE: PulsePage = {
  feed: [post()],
  today: 1,
  ideas: [{ label: "Productivity", ideas: [IDEA] }],
  note: "",
};


type IoEntry = { target: Element; isIntersecting: boolean; intersectionRatio: number };

class MockIO {
  cb: (entries: IoEntry[]) => void;
  els = new Set<Element>();
  constructor(cb: (entries: IoEntry[]) => void, _opts?: IntersectionObserverInit) {
    this.cb = cb;
    observers.push(this);
  }
  observe(el: Element) {
    this.els.add(el);
  }
  unobserve(el: Element) {
    this.els.delete(el);
  }
  disconnect() {
    this.els.clear();
  }
  trigger(el: Element, ratio: number) {
    this.cb([{ target: el, isIntersecting: ratio > 0, intersectionRatio: ratio }]);
  }
}

let observers: MockIO[] = [];

function installMockIO() {
  observers = [];
  vi.stubGlobal("IntersectionObserver", MockIO);
}

function trigger(el: Element, ratio: number) {
  for (const o of observers) {
    if (o.els.has(el)) o.trigger(el, ratio);
  }
}

function setHidden(hidden: boolean) {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    get: () => (hidden ? "hidden" : "visible"),
  });
  Object.defineProperty(document, "hidden", {
    configurable: true,
    get: () => hidden,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}


describe("Pulse", () => {
  beforeEach(() => {
    for (const fn of Object.values(api)) fn.mockReset();
    api.pulseImage.mockImplementation((name) => Promise.resolve(`data:image/jpeg;base64,${name}`));
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
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
    for (const gone of ["Snooze", "Delete", "Not this"]) {
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

  it("the menu holds only Why I created this", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseWhy.mockResolvedValue("You follow Rust.");
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    await userEvent.click(within(card).getByRole("button", { name: "More" }));
    const items = within(card).getAllByRole("menuitem").map((b) => b.textContent);
    expect(items).toEqual(["Why I created this"]);
    await userEvent.click(within(card).getByRole("menuitem", { name: "Why I created this" }));
    expect(api.pulseWhy).toHaveBeenCalledWith("post-1");
    expect(await within(card).findByRole("note")).toHaveTextContent("You follow Rust.");
    expect(within(card).queryByRole("menu")).not.toBeInTheDocument();
  });

  it("Snooze and Delete sit before Not interested when the card suits them", async () => {
    const both = { ...PAGE, feed: [post({ snooze: true, delete: true })] };
    api.getPulse.mockResolvedValue(both);
    api.pulseAct.mockResolvedValue(both);
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    const row = within(card).getByRole("group", { name: "Card actions" });
    const labels = within(row)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(labels).toEqual(["Like", "Discuss", "Read it", "Snooze", "Delete", "Not interested", "Dismiss"]);
    await userEvent.click(within(row).getByRole("button", { name: "Snooze" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("post-1", "snooze");
    // Delete asks first.
    await userEvent.click(within(row).getByRole("button", { name: "Delete" }));
    expect(api.pulseAct).not.toHaveBeenLastCalledWith("post-1", "delete");
    const confirm = within(card).getByRole("group", { name: "Delete this post?" });
    await userEvent.click(within(confirm).getByRole("button", { name: "Delete" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("post-1", "delete");
  });

  it("every card has one row: Like first, its own buttons, Not interested, Dismiss last", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(within(card).queryByRole("menu")).not.toBeInTheDocument();
    const row = within(card).getByRole("group", { name: "Card actions" });
    const labels = within(row)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(labels[0]).toBe("Like");
    expect(labels[labels.length - 2]).toBe("Not interested");
    // One dislike button only.
    expect(labels.filter((l) => l === "Not interested" || l === "I don't like")).toHaveLength(1);
    expect(labels[labels.length - 1]).toBe("Dismiss");
    expect(labels).toContain("Discuss");

    for (const [label, act, note] of [
      ["Dismiss", "dismiss", "Dismissed."],
      ["Not interested", "dislike", "Got it. A few less like that."],
    ] as const) {
      await userEvent.click(within(row).getByRole("button", { name: label }));
      expect(api.pulseAct).toHaveBeenLastCalledWith("post-1", act);
      expect(await screen.findByRole("status")).toHaveTextContent(note);
    }
  });

  it("an error reads as an error, not as a result", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockRejectedValue("That card is gone.");
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    await userEvent.click(within(card).getByRole("button", { name: "Dismiss" }));
    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("That card is gone.");
    expect(status).toHaveClass("status--error");
  });

  it("the time carries the full date", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);
    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(within(card).getByText("2h ago")).toHaveAttribute("title", expect.stringContaining("2026"));
  });

  it("Love toggles on and off and Discuss hands the post to chat", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [post({ loved: true })] });
    api.pulseDiscuss.mockResolvedValue({
      conversationId: "pulse-post-1",
      text: "Let's talk about this post from my Pulse feed.",
    });
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    const love = within(card).getByRole("button", { name: "Like" });
    expect(love).toHaveAttribute("aria-pressed", "false");
    // Every button in the row looks the same.
    expect(within(card).getByRole("button", { name: "Discuss" })).toHaveClass("quiet");
    expect(love).toHaveClass("quiet");
    await userEvent.click(love);
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "love");
    await vi.waitFor(() =>
      expect(within(card).getByRole("button", { name: "Like" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    api.pulseAct.mockResolvedValue(PAGE);
    await userEvent.click(within(card).getByRole("button", { name: "Like" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("post-1", "unlove");
    await vi.waitFor(() =>
      expect(within(card).getByRole("button", { name: "Like" })).toHaveAttribute(
        "aria-pressed",
        "false",
      ),
    );

    await userEvent.click(within(card).getByRole("button", { name: "Discuss" }));
    expect(api.pulseDiscuss).toHaveBeenCalledWith("post-1");
    expect(onRun).toHaveBeenCalledWith(
      "Let's talk about this post from my Pulse feed.",
      "pulse-post-1",
    );
  });

  it("a reported card opens its full report and its chips act on the card", async () => {
    const crash = post({
      id: "crash-1",
      title: "bluetoothd crashed 4 times today",
      summary: "Your Bluetooth service keeps restarting.",
      link: "",
      more: "It stopped with a segfault each time.\n\n**What was checked**\n- 4 crash reports since 9 AM",
      chips: [
        { label: "Restart bluetooth", act: "offer", index: 1 },
        { label: "Discuss", act: "discuss", index: 0 },
        { label: "Ignore for now", act: "ignore", index: 0 },
        { label: "Remind me later", act: "remind", index: 0 },
      ],
    });
    api.getPulse.mockResolvedValue({ ...PAGE, feed: [crash] });
    api.pulseOffer.mockResolvedValue({
      conversationId: "pulse-crash-1",
      text: "Restart the bluetooth service.",
    });
    api.pulseRemind.mockResolvedValue({
      page: { ...PAGE, feed: [] },
      note: "Back tonight, 9:30 PM before you wind down.",
    });
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [crash] });
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    const card = await screen.findByRole("listitem", { name: "bluetoothd crashed 4 times today" });
    // Always fully open: the report shows with no toggle.
    expect(card).toHaveTextContent("It stopped with a segfault each time.");
    expect(card).toHaveTextContent("4 crash reports since 9 AM");
    // Every button in the row looks the same.
    const fix = within(card).getByRole("button", { name: "Restart bluetooth" });
    expect(fix).toHaveClass("quiet");
    expect(within(card).getByRole("button", { name: "Ignore for now" })).toHaveClass("quiet");

    await userEvent.click(fix);
    expect(api.pulseOffer).toHaveBeenCalledWith("crash-1", 1);
    expect(onRun).toHaveBeenCalledWith("Restart the bluetooth service.", "pulse-crash-1");

    await userEvent.click(within(card).getByRole("button", { name: "Ignore for now" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("crash-1", "ignore");
    expect(await screen.findByRole("status")).toHaveTextContent(IGNORED_NOTE);

    await userEvent.click(within(card).getByRole("button", { name: "Remind me later" }));
    expect(api.pulseRemind).toHaveBeenCalledWith("crash-1");
    await vi.waitFor(() =>
      expect(screen.getByRole("status")).toHaveTextContent(
        "Back tonight, 9:30 PM before you wind down.",
      ),
    );
    expect(
      screen.queryByRole("listitem", { name: "bluetoothd crashed 4 times today" }),
    ).not.toBeInTheDocument();
  });

  it("a card says where it is from and why, and Read it opens the source", async () => {
    api.getPulse.mockResolvedValue({
      ...PAGE,
      feed: [post({ source: "Helldivers 2 on Steam", because: "You play Helldivers 2" })],
    });
    api.openUrl.mockResolvedValue();
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(card.querySelector(".feed-source")).toHaveTextContent(
      "Helldivers 2 on Steam · You play Helldivers 2",
    );
    await userEvent.click(within(card).getByRole("button", { name: "Read it" }));
    expect(api.openUrl).toHaveBeenCalledWith("https://blog.rust-lang.org/1.99.html");
    // Reading counts toward card taste, quietly.
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "opened");
  });

  it("a post with no source page has no Read it", async () => {
    api.getPulse.mockResolvedValue({ ...PAGE, feed: [post({ link: "" })] });
    render(<Pulse onRun={() => {}} />);
    const card = await screen.findByRole("listitem", { name: TITLE });
    expect(card.querySelector(".feed-source")).toHaveTextContent("Rust Blog");
    expect(within(card).queryByRole("button", { name: "Read it" })).not.toBeInTheDocument();
  });

  it("today's set ends with That's all for today, then the older posts", async () => {
    api.getPulse.mockResolvedValue({
      ...PAGE,
      feed: [post(), post({ id: "old", title: "Older post" })],
      today: 1,
    });
    render(<Pulse onRun={() => {}} />);
    await screen.findByRole("listitem", { name: "Older post" });
    const items = screen.getAllByRole("listitem").map((li) => li.getAttribute("aria-label"));
    expect(items).toEqual([TITLE, "Older post"]);
    const end = screen.getByRole("separator");
    expect(end).toHaveTextContent(DAY_END);
    expect(end.previousElementSibling).toHaveAttribute("aria-label", TITLE);
  });

  it("an empty feed says what the writer found and offers interests", async () => {
    api.getPulse.mockResolvedValue({
      ...PAGE,
      feed: [],
      today: 0,
      note: "Nothing to look into yet. Add an interest in Settings → Pulse → Interests.",
    });
    const onOpenInterests = vi.fn();
    render(<Pulse onRun={() => {}} onOpenInterests={onOpenInterests} />);
    expect(
      await screen.findByText(
        "Nothing to look into yet. Add an interest in Settings → Pulse → Interests.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByText(FEED_EMPTY)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: ADD_INTERESTS }));
    expect(onOpenInterests).toHaveBeenCalled();
  });

  it("looks for new posts once a minute while open", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      api.getPulse
        .mockResolvedValueOnce(PAGE)
        .mockResolvedValue({ ...PAGE, feed: [post({ id: "new", title: "Fresh post" }), post()] });
      render(<Pulse onRun={() => {}} />);
      await screen.findByRole("listitem", { name: TITLE });
      expect(screen.queryByRole("listitem", { name: "Fresh post" })).not.toBeInTheDocument();
      await vi.advanceTimersByTimeAsync(REFRESH_MS);
      expect(await screen.findByRole("listitem", { name: "Fresh post" })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
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

  it("an idea shows the same row as every card: Like, Let's do it, Not interested, Dismiss", async () => {
    const written = { ...IDEA, id: "idea-2", title: "Rust 1.99 upgrade plan", icon: "🦀" };
    const page: PulsePage = {
      ...PAGE,
      feed: [],
      ideas: [
        { label: "", ideas: [written] },
        { label: "Productivity", ideas: [IDEA] },
      ],
    };
    const likedPage: PulsePage = {
      ...page,
      ideas: [
        { label: "", ideas: [{ ...written, loved: true }] },
        { label: "Productivity", ideas: [IDEA] },
      ],
    };
    api.getPulse.mockResolvedValue(page);
    api.pulseAct.mockImplementation(async (_id, act) => (act === "moreLike" ? likedPage : page));
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    expect(screen.getAllByRole("heading").map((h) => h.textContent)).toEqual(["Productivity"]);
    const row = screen.getByRole("listitem", { name: "Rust 1.99 upgrade plan" });
    expect(row.querySelector("img.idea-icon")).toHaveAttribute("src", "/emoji3d/1f980.png");
    const buttons = within(within(row).getByRole("group", { name: "Card actions" }))
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(buttons).toEqual(["Like", "Let's do it", "Not interested", "Dismiss"]);
    expect(within(row).queryByRole("button", { name: "More" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Snooze" })).toBeNull();
    const main = within(row).getByRole("button", { name: "Let's do it" });

    const like = within(row).getByRole("button", { name: "Like" });
    expect(like).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(like);
    expect(api.pulseAct).toHaveBeenCalledTimes(1);
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "moreLike");
    expect(await screen.findByRole("status")).toHaveTextContent(MORE_LIKE_NOTE);
    const heart = within(row).getByRole("button", { name: "Like" });
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart).toHaveTextContent("♥");

    api.pulseAct.mockImplementation(async (_id, act) => (act === "unlove" ? page : likedPage));
    await userEvent.click(heart);
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "unlove");
    const likeAgain = within(row).getByRole("button", { name: "Like" });
    expect(likeAgain).toHaveAttribute("aria-pressed", "false");
    expect(likeAgain).toHaveTextContent("♡ Like");

    api.pulseAct.mockImplementation(async (_id, act) => (act === "moreLike" ? likedPage : page));
    await userEvent.click(likeAgain);
    expect(api.pulseAct.mock.calls.filter((c) => c[1] === "moreLike")).toHaveLength(2);
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "moreLike");
    expect(within(row).getByRole("button", { name: "Like" })).toHaveAttribute("aria-pressed", "true");

    await userEvent.click(within(row).getByRole("button", { name: "Not interested" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "notInterested");
    expect(await screen.findByRole("status")).toHaveTextContent(NOT_INTERESTED_NOTE);

    await userEvent.click(within(row).getByRole("button", { name: "Dismiss" }));
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "dismiss");

    await userEvent.click(main);
    expect(api.pulseAct).toHaveBeenLastCalledWith("idea-2", "accept");
    expect(onRun).toHaveBeenCalledWith("close every done card on the workboard");
  });

  it("a liked idea keeps its heart when Pulse is read again", async () => {
    const liked = { ...IDEA, id: "idea-2", title: "Rust 1.99 upgrade plan", loved: true };
    api.getPulse.mockResolvedValue({
      ...PAGE,
      feed: [],
      ideas: [{ label: "", ideas: [liked] }],
    });
    render(<Pulse onRun={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    const like = within(screen.getByRole("listitem", { name: "Rust 1.99 upgrade plan" })).getByRole(
      "button",
      { name: "Like" },
    );
    expect(like).toHaveAttribute("aria-pressed", "true");
    expect(like).toHaveTextContent("♥");
    expect(api.pulseAct).not.toHaveBeenCalled();
  });

  it("reduced motion skips the pop when an idea Like turns into a heart", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: true, addEventListener: () => {}, removeEventListener: () => {} })),
    );
    const written = { ...IDEA, id: "idea-2", title: "Rust 1.99 upgrade plan" };
    const page: PulsePage = { ...PAGE, feed: [], ideas: [{ label: "", ideas: [written] }] };
    const likedPage: PulsePage = {
      ...page,
      ideas: [{ label: "", ideas: [{ ...written, loved: true }] }],
    };
    api.getPulse.mockResolvedValue(page);
    api.pulseAct.mockResolvedValue(likedPage);
    render(<Pulse onRun={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    const like = within(screen.getByRole("listitem", { name: "Rust 1.99 upgrade plan" })).getByRole(
      "button",
      { name: "Like" },
    );
    await userEvent.click(like);
    const heart = within(screen.getByRole("listitem", { name: "Rust 1.99 upgrade plan" })).getByRole(
      "button",
      { name: "Like" },
    );
    expect(heart).toHaveAttribute("aria-pressed", "true");
    expect(heart).toHaveTextContent("♥");
    expect(heart).not.toHaveClass("card-like-pop");
  });

  it("a crash idea shows its report and chips, and Discuss opens its own chat", async () => {
    const crash: PulseIdea = {
      ...IDEA,
      id: "crash-1",
      title: "bluetoothd crashed 4 times today",
      summary: "Your Bluetooth service keeps restarting.",
      more: "It stopped with a segfault each time.",
      chips: [
        { label: "Find out why", act: "offer", index: 0 },
        { label: "Discuss", act: "discuss", index: 0 },
        { label: "Ignore for now", act: "ignore", index: 0 },
        { label: "Remind me later", act: "remind", index: 0 },
      ],
    };
    api.getPulse.mockResolvedValue({ ...PAGE, feed: [], ideas: [{ label: "", ideas: [crash] }] });
    api.pulseDiscuss.mockResolvedValue({ conversationId: "pulse-crash-1", text: "Tell me more about this." });
    api.pulseOffer.mockResolvedValue({ conversationId: "pulse-crash-1", text: "Read bluetoothd's crash history." });
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);
    await userEvent.click(await screen.findByRole("button", { name: "For You" }));

    const row = screen.getByRole("listitem", { name: "bluetoothd crashed 4 times today" });
    expect(within(row).queryByRole("button", { name: "Let's do it" })).not.toBeInTheDocument();
    // Not interested is the row's one dislike button.
    expect(within(row).getAllByRole("button", { name: "Not interested" })).toHaveLength(1);
    expect(row).toHaveTextContent("It stopped with a segfault each time.");

    await userEvent.click(within(row).getByRole("button", { name: "Discuss" }));
    expect(onRun).toHaveBeenLastCalledWith("Tell me more about this.", "pulse-crash-1");
    await userEvent.click(within(row).getByRole("button", { name: "Find out why" }));
    expect(api.pulseOffer).toHaveBeenCalledWith("crash-1", 0);
    expect(onRun).toHaveBeenLastCalledWith("Read bluetoothd's crash history.", "pulse-crash-1");

    expect(within(row).getByRole("button", { name: "Not interested" })).toBeInTheDocument();
  });

  it("a PC health card reads its verdict and Fix it opens its options, which only advise", async () => {
    const health: PulseIdea = {
      ...IDEA,
      id: "pc-1",
      title: "PC health after Hades",
      summary: "Max GPU 88°C.",
      action: "I just finished a gaming session (Hades).",
      verdict: { tone: "watch", line: "Warmer than usual: GPU peaked at 88°C." },
      fix: "Fix it",
    };
    const page: PulsePage = { ...PAGE, feed: [], ideas: [{ label: "", ideas: [health] }] };
    api.getPulse.mockResolvedValue(page);
    api.pulseAct.mockResolvedValue(page);
    api.pulseFix.mockResolvedValue({
      title: "PC health after Hades",
      verdict: health.verdict,
      options: [
        { kind: "compare", title: "Compare with your last sessions", why: "See this run next to your recent ones.", steps: [] },
        {
          kind: "fpsCap",
          title: "Cap the frame rate in Hades",
          why: "Frames past your refresh rate are heat you never see.",
          steps: ["Set a frame limit at your monitor's refresh rate."],
        },
        { kind: "ask", title: "Ask Loadout about it", why: "Open a chat with this session's numbers.", steps: [] },
      ],
      compare: [
        { session: "This one", time: "40 min", gpuPeak: "88°C", cpuPeak: "70°C", gpuBusy: "90%" },
        { session: "Hades", time: "35 min", gpuPeak: "78°C", cpuPeak: "68°C", gpuBusy: "85%" },
      ],
    });
    const onRun = vi.fn();
    render(<Pulse onRun={onRun} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    const row = screen.getByRole("listitem", { name: "PC health after Hades" });
    expect(row).toHaveTextContent("Warmer than usual: GPU peaked at 88°C.");
    expect(within(row).queryByRole("button", { name: "Let's do it" })).toBeNull();
    await userEvent.click(within(row).getByRole("button", { name: "Fix it" }));
    expect(api.pulseFix).toHaveBeenCalledWith("pc-1");
    expect(api.pulseAct).not.toHaveBeenCalled();

    const sheet = await screen.findByRole("dialog", { name: "Fix it" });
    expect(sheet).toHaveTextContent("never changes fans, clocks or drivers");
    const fps = await within(sheet).findByRole("listitem", { name: "Cap the frame rate in Hades" });
    await userEvent.click(within(fps).getByRole("button", { name: "Show steps" }));
    expect(fps).toHaveTextContent("Set a frame limit at your monitor's refresh rate.");
    const compare = within(sheet).getByRole("listitem", { name: "Compare with your last sessions" });
    await userEvent.click(within(compare).getByRole("button", { name: "Compare" }));
    expect(within(compare).getAllByRole("row")).toHaveLength(3);

    await userEvent.click(within(sheet).getByRole("button", { name: "Ask" }));
    expect(screen.queryByRole("dialog", { name: "Fix it" })).toBeNull();
    expect(api.pulseAct).toHaveBeenLastCalledWith("pc-1", "accept");
    await vi.waitFor(() => expect(onRun).toHaveBeenCalledWith("I just finished a gaming session (Hades)."));
  });

  it("a failed act says why, reloads, and does not run", async () => {
    api.getPulse
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce(PAGE)
      .mockResolvedValueOnce({ ...PAGE, feed: [], ideas: [] });
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
    api.pulseAct.mockRejectedValue("The Pulse ledger is too large to add to. Nothing was changed.");
    render(<Pulse onRun={vi.fn()} />);

    await userEvent.click(await screen.findByRole("button", { name: "For You" }));
    await userEvent.click(screen.getByRole("button", { name: /I can tidy the workboard/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("The Pulse ledger is too large");
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
  it("Save puts a source post on the Saved page, which only offers Remove from saved", async () => {
    api.getPulse.mockResolvedValue({ ...PAGE, feed: [post({ save: true })] });
    api.pulseAct.mockResolvedValue({ ...PAGE, feed: [post({ save: true, saved: true, loved: true })] });
    const find: SavedFind = {
      title: TITLE,
      url: "https://blog.rust-lang.org/1.99.html",
      sourceName: "Rust Blog",
      summary: "Async closures are stable.",
      because: "You follow Rust",
      savedMs: Date.UTC(2026, 9, 26, 16, 0),
    };
    api.pulseSaved.mockResolvedValue([find]);
    api.pulseUnsave.mockResolvedValue([]);
    render(<Pulse onRun={() => {}} />);

    const card = await screen.findByRole("listitem", { name: TITLE });
    const row = within(card).getByRole("group", { name: "Card actions" });
    const rowNames = within(row)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? b.textContent);
    // Like far left, Save among the card's own actions, Dismiss far right.
    expect(rowNames[0]).toBe("Like");
    expect(rowNames.at(-1)).toBe("Dismiss");
    expect(rowNames.indexOf("Save")).toBeLessThan(rowNames.indexOf("Not interested"));
    await userEvent.click(within(row).getByRole("button", { name: "Save" }));
    expect(api.pulseAct).toHaveBeenCalledWith("post-1", "save");
    expect(await within(card).findByRole("button", { name: "Saved" })).toBeDisabled();

    await userEvent.click(screen.getByRole("button", { name: "Saved", pressed: false }));
    const saved = await screen.findByRole("listitem", { name: TITLE });
    expect(saved).toHaveTextContent("Async closures are stable.");
    expect(saved).toHaveTextContent("Rust Blog · You follow Rust");
    // No row of buttons: only the ⋯ menu.
    expect(within(saved).queryByRole("group", { name: "Card actions" })).not.toBeInTheDocument();
    expect(within(saved).getAllByRole("button").map((b) => b.textContent)).toEqual(["⋯"]);
    await userEvent.click(within(saved).getByRole("button", { name: "More" }));
    const items = within(saved).getAllByRole("menuitem");
    expect(items.map((i) => i.textContent)).toEqual(["Remove from saved"]);
    await userEvent.click(items[0]);
    expect(api.pulseUnsave).toHaveBeenCalledWith("https://blog.rust-lang.org/1.99.html");
    expect(await screen.findByText(/Nothing saved yet/)).toBeInTheDocument();
  });

  it("sends dwell after 8000 ms at least 60% visible, not at 7999", async () => {
    installMockIO();
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);
    const card = await screen.findByRole("listitem", { name: TITLE });
    vi.useFakeTimers();
    try {
      trigger(card, DWELL_RATIO);
      await vi.advanceTimersByTimeAsync(7999);
      expect(api.pulseAct).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(api.pulseAct).toHaveBeenCalledWith("post-1", "dwell");
      expect(api.pulseAct).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("resets the dwell timer on scroll-out", async () => {
    installMockIO();
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);
    const card = await screen.findByRole("listitem", { name: TITLE });
    vi.useFakeTimers();
    try {
      trigger(card, DWELL_RATIO);
      await vi.advanceTimersByTimeAsync(4000);
      trigger(card, 0);
      trigger(card, DWELL_RATIO);
      await vi.advanceTimersByTimeAsync(7999);
      expect(api.pulseAct).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      expect(api.pulseAct).toHaveBeenCalledWith("post-1", "dwell");
      expect(api.pulseAct).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("a hidden document sends nothing", async () => {
    installMockIO();
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);
    const card = await screen.findByRole("listitem", { name: TITLE });
    vi.useFakeTimers();
    try {
      setHidden(true);
      trigger(card, DWELL_RATIO);
      await vi.advanceTimersByTimeAsync(DWELL_MS);
      expect(api.pulseAct).not.toHaveBeenCalled();
      trigger(card, 0);
      setHidden(false);
      trigger(card, DWELL_RATIO);
      await vi.advanceTimersByTimeAsync(4000);
      setHidden(true);
      await vi.advanceTimersByTimeAsync(DWELL_MS);
      expect(api.pulseAct).not.toHaveBeenCalled();
    } finally {
      setHidden(false);
      vi.useRealTimers();
    }
  });

  it("sends nothing without IntersectionObserver", async () => {
    api.getPulse.mockResolvedValue(PAGE);
    api.pulseAct.mockResolvedValue(PAGE);
    render(<Pulse onRun={() => {}} />);
    expect(await screen.findByRole("listitem", { name: TITLE })).toBeInTheDocument();
    vi.useFakeTimers();
    try {
      await vi.advanceTimersByTimeAsync(DWELL_MS);
      expect(api.pulseAct).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
