import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatEnvelope, ChatEvent, PersonalityPage, Preset, Settings } from "./api";

const api = vi.hoisted(() => ({
  handler: null as ((envelope: ChatEnvelope) => void) | null,
  sendChat: vi.fn<(id: string, text: string) => Promise<string>>(),
  getSettings: vi.fn<() => Promise<Settings>>(),
  getPersonalities: vi.fn<() => Promise<PersonalityPage>>(),
}));

const desktop = vi.hoisted(() => ({
  call: vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>(),
}));

vi.mock("./api", () => ({
  sendChat: api.sendChat,
  getSettings: api.getSettings,
  saveSettings: vi.fn(),
  answerPermission: vi.fn(),
  answerElicit: vi.fn(),
  cancelChat: vi.fn(),
  closeChat: vi.fn(() => Promise.resolve()),
  getPersonalities: api.getPersonalities,
  setPersonality: vi.fn(),
  setExplicitPersonalities: vi.fn(),
  duplicatePersonality: vi.fn(),
  savePersonality: vi.fn(),
  deletePersonality: vi.fn(),
  onChatEvent: (handler: (envelope: ChatEnvelope) => void) => {
    api.handler = handler;
    return Promise.resolve(() => {});
  },
  onEventGap: () => Promise.resolve(() => {}),
}));

vi.mock("./transport", () => ({ call: desktop.call }));

import { CHAT_EMPTY, Chat } from "./Chat";
import { Personality } from "./Personality";
import { activityOf, artUrl, PoseCycle } from "./nyx";

const SETTINGS: Settings = {
  model: "grok-4.7",
  closeToTray: true,
  hasApiKey: true,
  workspace: "",
  workspacePath: "/home/user",
  approvals: "ask",
  desktopControl: false,
  theme: "system",
  version: "3.1.0",
  summonHotkey: false,
  notifications: false,
};

/** The desktop's nyx_look for Nyx herself: one set per mood, as the shell
 * names them (src-tauri/src/nyx.rs). */
const SETS: Record<string, string[]> = {
  "hanging-out": ["brand/nyx-hanging-out.jpg", "brand/nyx/defaults/nyx/4.jpg"],
  thinking: ["brand/nyx/actions/thinking/1.jpg", "brand/nyx/actions/thinking/2.jpg"],
  working: ["brand/nyx-working.jpg", "brand/nyx/actions/working/1.jpg"],
};

function nyxLookCalls() {
  return desktop.call.mock.calls.filter(([command]) => command === "nyx_look");
}

function dockPicture(): string {
  const dock = screen.getByRole("complementary", { name: "Nyx" });
  return within(dock).getByRole("img").getAttribute("src") ?? "";
}

function emit(conversationId: string, event: ChatEvent) {
  act(() => api.handler?.({ conversationId, event }));
}

describe("activityOf", () => {
  it("names what the chat does, not each chunk of text", () => {
    expect(activityOf([], false)).toBe("hanging-out");
    expect(activityOf([{ kind: "user", text: "hi" }], true)).toBe("thinking");
    expect(activityOf([{ kind: "assistant", text: "Sure", streaming: true }], true)).toBe("thinking");
    expect(
      activityOf([{ kind: "tool", id: "t1", title: "web_fetch https://example.com", status: "running", detail: "" }], true),
    ).toBe("tool:web_fetch https://example.com");
    expect(
      activityOf(
        [{ kind: "permission", id: "p1", title: "bash", action: "rm -rf build", reason: "", answer: null }],
        true,
      ),
    ).toBe("working");
    expect(
      activityOf(
        [{ kind: "permission", id: "p1", title: "bash", action: "ls", reason: "", answer: "allow" }],
        true,
      ),
    ).toBe("thinking");
  });
});

describe("PoseCycle", () => {
  it("holds a pose while the set and mood hold and moves on one when either changes", () => {
    const cycle = new PoseCycle();
    expect(cycle.pick("pirate", "hanging-out", 5)).toBe(0);
    expect(cycle.pick("pirate", "hanging-out", 5)).toBe(0);
    expect(cycle.pick("pirate", "thinking", 5)).toBe(1);
    expect(cycle.pick("working", "working", 10)).toBe(0);
    expect(cycle.pick("pirate", "working", 5)).toBe(2);
    expect(cycle.pick("one", "working", 1)).toBe(0);
    expect(cycle.pick("one", "reading", 1)).toBe(0);
  });
});

describe("artUrl", () => {
  it("bundles her pictures and nothing else", () => {
    expect(artUrl("brand/nyx/defaults/pirate/1.jpg")).toContain("pirate/1.jpg");
    expect(artUrl("brand/nyx-working.jpg")).toContain("nyx-working.jpg");
    expect(artUrl("brand/nyx-gaming.jpg")).toBeNull();
    expect(artUrl("brand/nyx/actions/gaming/2.jpg")).toBeNull();
    expect(artUrl("brand/logo-mark.svg")).toBeNull();
    expect(artUrl("../../etc/passwd")).toBeNull();
  });
});

describe("Nyx in the chat", () => {
  beforeEach(() => {
    api.sendChat.mockReset();
    api.getSettings.mockReset().mockResolvedValue(SETTINGS);
    api.handler = null;
    desktop.call.mockReset().mockImplementation((command, args) => {
      if (command !== "nyx_look") return Promise.reject(new Error(`unexpected ${command}`));
      const mood = args?.tool !== undefined ? "working" : String(args?.mood);
      return Promise.resolve({ set: mood, poses: SETS[mood], explicit: false, mood });
    });
    window.innerWidth = 1280;
  });

  afterEach(() => {
    window.innerWidth = 1024;
  });

  it("shows her face in the empty chat and by Grok's replies", async () => {
    let finish: () => void = () => {};
    api.sendChat.mockImplementation(() => new Promise((resolve) => (finish = () => resolve("end_turn"))));
    const { container } = render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(container.querySelector(".nyx-face-96")).not.toBeNull());
    expect(container.querySelector(".nyx-face-96")?.getAttribute("src")).toContain("nyx-hanging-out.jpg");

    await userEvent.type(screen.getByLabelText("Message"), "hello{Enter}");
    await waitFor(() => expect(api.sendChat).toHaveBeenCalled());
    const id = api.sendChat.mock.calls[0][0];
    emit(id, { kind: "text", text: "Hi there" });
    const reply = await screen.findByText("Hi there");
    const row = reply.closest(".nyx-row");
    expect(row).not.toBeNull();
    expect(row?.querySelector(".nyx-face-32")).not.toBeNull();
    expect(screen.getByText("hello").closest(".nyx-row")).toBeNull();
    act(() => finish());
  });

  it("changes pose on a tool event and not on every chunk of text", async () => {
    let finish: () => void = () => {};
    api.sendChat.mockImplementation(() => new Promise((resolve) => (finish = () => resolve("end_turn"))));
    render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(dockPicture()).toContain("nyx-hanging-out.jpg"));

    await userEvent.type(screen.getByLabelText("Message"), "run the tests{Enter}");
    const id = await waitFor(() => api.sendChat.mock.calls[0][0]);
    await waitFor(() => expect(dockPicture()).toContain("thinking/1.jpg"));
    expect(screen.getByText("Thinking")).toBeVisible();
    const asked = nyxLookCalls().length;

    for (const text of ["Running ", "the ", "tests"]) emit(id, { kind: "text", text });
    await screen.findByText("Running the tests");
    expect(nyxLookCalls()).toHaveLength(asked);
    expect(dockPicture()).toContain("thinking/1.jpg");

    emit(id, { kind: "tool", id: "t1", title: "bash cargo test", status: "running", detail: "" });
    await waitFor(() => expect(dockPicture()).toContain("nyx-working.jpg"));
    expect(nyxLookCalls().at(-1)?.[1]).toEqual({ tool: "bash cargo test" });
    expect(screen.getByText("Working")).toBeVisible();

    // The same tool finishing is no new activity.
    emit(id, { kind: "tool", id: "t1", title: "bash cargo test", status: "done", detail: "ok" });
    expect(nyxLookCalls()).toHaveLength(asked + 1);

    // Back to thinking: that set moves on to its next pose.
    emit(id, { kind: "text", text: "All green." });
    await waitFor(() => expect(dockPicture()).toContain("thinking/2.jpg"));
    act(() => finish());
  });

  it("leaves the dock out on narrow windows and keeps the face", async () => {
    window.innerWidth = 700;
    const { container } = render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(container.querySelector(".nyx-face-96")).not.toBeNull());
    expect(screen.queryByRole("complementary", { name: "Nyx" })).toBeNull();

    act(() => {
      window.innerWidth = 1200;
      window.dispatchEvent(new Event("resize"));
    });
    expect(screen.getByRole("complementary", { name: "Nyx" })).toBeVisible();
  });

  it("shows no Nyx at all for a voice with no art", async () => {
    desktop.call.mockReset().mockResolvedValue(null);
    const { container } = render(<Chat onOpenSettings={() => {}} />);
    await waitFor(() => expect(nyxLookCalls()).toHaveLength(1));
    expect(container.querySelector(".nyx-face")).toBeNull();
    expect(screen.queryByRole("complementary", { name: "Nyx" })).toBeNull();
    expect(screen.getByText(CHAT_EMPTY)).toBeVisible();
  });
});

function preset(id: string, name: string, custom = false): Preset {
  return {
    id,
    name,
    description: `${name}'s voice.`,
    sample: "",
    toneRules: "",
    emoji: "none",
    verbosity: "normal",
    explicit: false,
    custom,
  };
}

describe("Personality tiles", () => {
  it("show each voice's picture, and none for a voice with no art", async () => {
    api.getPersonalities.mockResolvedValue({
      current: "nyx",
      presets: [preset("nyx", "Nyx"), preset("pirate", "Pirate"), preset("pirate-copy", "Pirate copy", true)],
      problems: [],
      explicit: false,
    });
    desktop.call.mockReset().mockImplementation((command, args) => {
      expect(command).toBe("nyx_tiles");
      expect(args).toEqual({ ids: ["nyx", "pirate", "pirate-copy"] });
      return Promise.resolve({
        nyx: "brand/nyx-hanging-out.jpg",
        pirate: "brand/nyx/defaults/pirate/1.jpg",
      });
    });
    render(<Personality />);
    const pirate = await screen.findByRole("listitem", { name: "Pirate" });
    await waitFor(() => expect(pirate.querySelector(".persona-thumb")).not.toBeNull());
    expect(pirate.querySelector(".persona-thumb")?.getAttribute("src")).toContain("pirate/1.jpg");
    expect(
      screen.getByRole("listitem", { name: "Nyx" }).querySelector(".persona-thumb")?.getAttribute("src"),
    ).toContain("nyx-hanging-out.jpg");
    expect(screen.getByRole("listitem", { name: "Pirate copy" }).querySelector(".persona-thumb")).toBeNull();
  });
});
