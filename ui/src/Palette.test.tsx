import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";
import { NO_SOURCES, Palette, paletteRows, type PaletteExtras, type PaletteSources } from "./Palette";
import { answersTo, PALETTE_SYNONYMS, shellKeys, type ChatHit, type CommandRunners } from "./paletteCommands";

vi.mock("./api", () => ({}));

const DESKTOP: Capabilities = {
  desktopControl: true,
  pulse: true,
  localAi: true,
  tray: true,
  webAccess: true,
  plugins: true,
  library: true,
};
const PHONE: Capabilities = { ...DESKTOP, desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false, library: false };
const WEB: Capabilities = { ...PHONE, remote: true };

const labels = (caps: Capabilities, q: string) => paletteRows(caps, q).map((r) => r.label);

const run = () => {};
/** Every command has a runner, as once the shell can halt. */
const ALL_RUNNERS: CommandRunners = { "new-chat": run, halt: run, "sign-in": run, shortcuts: run };
const EXTRAS: PaletteExtras = { runners: ALL_RUNNERS, signedOut: true, chats: [], slash: [] };
const withExtras = (caps: Capabilities, q: string, extras = EXTRAS) =>
  paletteRows(caps, q, extras).map((r) => r.label);

describe("paletteRows", () => {
  it("lists this platform's pages, then its Settings sections", () => {
    expect(labels(DESKTOP, "")).toEqual([
      "Chat",
      "Pulse",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Workboard",
      "Library",
      "Settings",
      "Settings: General",
      "Settings: Account",
      "Settings: Defaults",
      "Settings: Behavior",
      "Settings: Appearance",
      "Settings: Personality",
      "Settings: Playbooks",
      "Settings: Permissions",
      "Settings: Privacy & data",
      "Settings: Background",
      "Settings: Pulse",
      "Settings: Local AI",
      "Settings: Web access",
      "Settings: Labs",
      "Settings: About",
    ]);
    expect(labels(PHONE, "")).toEqual([
      "Chat",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Workboard",
      "Settings",
      "Settings: General",
      "Settings: Account",
      "Settings: Defaults",
      "Settings: Behavior",
      "Settings: Appearance",
      "Settings: Personality",
      "Settings: Playbooks",
      "Settings: Permissions",
      "Settings: Privacy & data",
      "Settings: Background",
      "Settings: About",
    ]);
    expect(labels(WEB, "")).toEqual(["Chat", "Automations", "Memory", "Connectors", "Workboard", "Settings"]);
  });

  it("puts the commands this shell can run after the pages", () => {
    expect(withExtras(DESKTOP, "").slice(0, 13)).toEqual([
      "Chat",
      "Pulse",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Workboard",
      "Library",
      "Settings",
      "New chat",
      "Halt",
      "Sign in",
      "Shortcuts",
    ]);
    // Only the runners the shell hands over get rows.
    expect(withExtras(DESKTOP, "", { ...EXTRAS, runners: { shortcuts: run } }).slice(9, 11)).toEqual([
      "Shortcuts",
      "Settings: General",
    ]);
  });

  it("hides Halt without desktop control, and Halt and Sign in on the web", () => {
    expect(withExtras(PHONE, "").slice(7, 10)).toEqual(["New chat", "Sign in", "Shortcuts"]);
    expect(withExtras(WEB, "")).toEqual([
      "Chat",
      "Automations",
      "Memory",
      "Connectors",
      "Workboard",
      "Settings",
      "New chat",
      "Shortcuts",
    ]);
  });

  it("names Grok's sign-in where this device offers it", () => {
    expect(withExtras({ ...DESKTOP, grokSignIn: true }, "sign")).toEqual(["Sign in with Grok", "Ask Loadout: sign"]);
  });

  it("shows Sign in only while signed out", () => {
    expect(withExtras(DESKTOP, "sign", { ...EXTRAS, signedOut: false })).toEqual(["Ask Loadout: sign"]);
    expect(withExtras(DESKTOP, "sign")).toEqual(["Sign in", "Ask Loadout: sign"]);
  });

  it("shows each row's key at its right", () => {
    const keys = Object.fromEntries(paletteRows(DESKTOP, "", EXTRAS).map((r) => [r.label, r.keys]));
    expect(keys["New chat"]).toBe("Ctrl+N");
    expect(keys["Halt"]).toBe("Ctrl+Alt+H");
    expect(keys["Shortcuts"]).toBe("Ctrl+/");
    expect(keys["Settings"]).toBe("Ctrl+,");
    expect(keys["Sign in"]).toBeUndefined();
    expect(keys["Chat"]).toBeUndefined();
  });

  it("finds rows by the cabin's older words", () => {
    expect(withExtras(DESKTOP, "feed")).toEqual(["Pulse", "Ask Loadout: feed"]);
    expect(withExtras(DESKTOP, "sched")).toEqual(["Automations", "Ask Loadout: sched"]);
    expect(withExtras(DESKTOP, "keys")).toEqual(["Shortcuts", "Ask Loadout: keys"]);
    expect(withExtras(DESKTOP, "hotk")).toEqual(["Shortcuts", "Ask Loadout: hotk"]);
    // A synonym answers to its start, not to any part of it.
    expect(answersTo("Pulse", "eed")).toBe(false);
  });

  it("names a row the palette lists for every synonym", () => {
    const listed = new Set(withExtras(DESKTOP, ""));
    for (const { row } of PALETTE_SYNONYMS) expect(listed).toContain(row);
  });

  it("lists slash commands for a query that starts with a slash", () => {
    const slash = [{ name: "/plan", insert: "/plan " }];
    const chats = [{ id: "c1", title: "plans" }];
    expect(paletteRows(DESKTOP, "/pl", { ...EXTRAS, slash, chats })).toEqual([
      { label: "/plan", action: { kind: "ask", text: "/plan " } },
      { label: "Ask Loadout: /pl", action: { kind: "ask", text: "/pl" } },
    ]);
  });

  it("lists recent chats, five at most, before the Settings sections", () => {
    const chats = Array.from({ length: 7 }, (_, i) => ({ id: `c${i}`, title: `Trip ${i}` }));
    const rows = paletteRows(PHONE, "", { ...EXTRAS, chats }).map((r) => r.label);
    expect(rows.slice(10, 16)).toEqual([
      "Chat: Trip 0",
      "Chat: Trip 1",
      "Chat: Trip 2",
      "Chat: Trip 3",
      "Chat: Trip 4",
      "Settings: General",
    ]);
  });

  it("filters by the typed text and ends with Ask Loadout", () => {
    expect(labels(DESKTOP, "  ACCO ")).toEqual(["Settings: Account", "Ask Loadout: ACCO"]);
    expect(paletteRows(DESKTOP, "zzz")).toEqual([
      { label: "Ask Loadout: zzz", action: { kind: "ask", text: "zzz" } },
    ]);
  });
});

describe("shellKeys", () => {
  it("lists the palette, the commands the shell can run, Settings and the chat box", () => {
    expect(shellKeys(DESKTOP, { "new-chat": run, shortcuts: run, "sign-in": run })).toEqual([
      { keys: "Ctrl+K", action: "Command palette" },
      { keys: "Ctrl+N", action: "New chat" },
      { keys: "Ctrl+/", action: "This sheet" },
      { keys: "Ctrl+,", action: "Settings" },
      { keys: "Enter", action: "Send message" },
      { keys: "Shift+Enter", action: "New line" },
      { keys: "Esc", action: "Close the palette or this sheet" },
    ]);
    expect(shellKeys(DESKTOP, ALL_RUNNERS)[2]).toEqual({ keys: "Ctrl+Alt+H", action: "Halt" });
    expect(shellKeys(PHONE, ALL_RUNNERS).map((k) => k.keys)).not.toContain("Ctrl+Alt+H");
  });
});

describe("Palette", () => {
  it("a chat title query lists that chat, and Enter opens it", async () => {
    const onPick = vi.fn();
    const chats = vi.fn(async (q: string): Promise<ChatHit[]> =>
      q === "budget" ? [{ id: "shell-c1", title: "Budget for the trip" }] : [],
    );
    const sources: PaletteSources = { ...NO_SOURCES, chats };
    render(<Palette caps={PHONE} sources={sources} onPick={onPick} onClose={vi.fn()} />);
    await userEvent.keyboard("budget");
    const row = await screen.findByRole("option", { name: "Chat: Budget for the trip" });
    expect(row).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{Enter}");
    expect(onPick).toHaveBeenCalledWith({ kind: "chat", id: "shell-c1" });
    expect(chats).toHaveBeenCalledWith("budget");
  });

  it("asks the slash source for a slash query, and a pick fills the chat box", async () => {
    const onPick = vi.fn();
    const slash = vi.fn(async (q: string) =>
      [
        { name: "/plan", insert: "/plan " },
        { name: "/new", insert: "/new" },
      ].filter((hit) => hit.name.startsWith(q)),
    );
    const chats = vi.fn(async () => [{ id: "c1", title: "plans" }]);
    const sources: PaletteSources = { ...NO_SOURCES, chats, slash };
    render(<Palette caps={PHONE} sources={sources} onPick={onPick} onClose={vi.fn()} />);
    expect(await screen.findByRole("option", { name: "Chat: plans" })).toBeInTheDocument();
    await userEvent.keyboard("/pl");
    expect(await screen.findByRole("option", { name: "/plan" })).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("option", { name: "/new" })).toBeNull();
    expect(screen.queryByRole("option", { name: "Chat: plans" })).toBeNull();
    await userEvent.keyboard("{Enter}");
    expect(onPick).toHaveBeenCalledWith({ kind: "ask", text: "/plan " });
    expect(slash).toHaveBeenLastCalledWith("/pl");
  });

  it("shows a row's key beside it and names it for screen readers", () => {
    const sources: PaletteSources = { ...NO_SOURCES, runners: { "new-chat": run } };
    render(<Palette caps={PHONE} sources={sources} onPick={vi.fn()} onClose={vi.fn()} />);
    const row = screen.getByRole("option", { name: "New chat" });
    expect(row).toHaveTextContent("New chatCtrl+N");
    expect(row).toHaveAttribute("aria-keyshortcuts", "Control+N");
    expect(screen.getByRole("option", { name: "Settings" })).toHaveAttribute("aria-keyshortcuts", "Control+,");
  });

  it("moves with the arrows, wraps, and picks with Enter", async () => {
    const onPick = vi.fn();
    const onClose = vi.fn();
    render(<Palette caps={PHONE} onPick={onPick} onClose={onClose} />);
    expect(screen.getByLabelText("Search pages and commands")).toHaveFocus();
    expect(screen.getByRole("option", { name: "Chat" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onPick).toHaveBeenCalledWith({ kind: "view", view: "journal" });
  });

  it("Escape and a click outside close it without a pick", async () => {
    const onPick = vi.fn();
    const onClose = vi.fn();
    const { container } = render(<Palette caps={PHONE} onPick={onPick} onClose={onClose} />);
    await userEvent.keyboard("{Escape}");
    await userEvent.click(container.querySelector(".palette-backdrop")!);
    await userEvent.click(screen.getByRole("dialog"));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(onPick).not.toHaveBeenCalled();
  });

  describe("with layout", () => {
    afterEach(() => {
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    });

    /** Rows 36 px tall, 40 px apart, in a list that shows 100 px. */
    function layout(reduced: boolean) {
      vi.stubGlobal(
        "matchMedia",
        vi.fn(() => ({ matches: reduced, addEventListener: () => {}, removeEventListener: () => {} })),
      );
      const row = (el: HTMLElement) => Number(/^palette-row-(\d+)$/.exec(el.id)?.[1] ?? 0);
      vi.spyOn(HTMLElement.prototype, "offsetTop", "get").mockImplementation(function (this: HTMLElement) {
        return row(this) * 40;
      });
      vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockReturnValue(36);
      vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(100);
      render(<Palette caps={PHONE} onPick={vi.fn()} onClose={vi.fn()} />);
      const list = screen.getByRole("listbox");
      let top = 0;
      Object.defineProperty(list, "scrollTop", { get: () => top, configurable: true });
      const scrollTo = vi.fn((to: ScrollToOptions) => {
        top = to.top ?? top;
      });
      list.scrollTo = scrollTo as unknown as typeof list.scrollTo;
      return { list, scrollTo, top: () => top };
    }

    it("is cut partway into a row, so the next one peeks", () => {
      const { list } = layout(false);
      // Half of jsdom's 768 px window is 384: rows 0-8 show, row 9 shows half.
      expect(list.style.maxHeight).toBe("378px");
    });

    it("arrow down near the bottom scrolls so the next row shows", async () => {
      const { scrollTo, top } = layout(false);
      await userEvent.keyboard("{ArrowDown}");
      // Row 1 picked: row 2 (80-116) must show in the 100 px view.
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 16, behavior: "smooth" });
      await userEvent.keyboard("{ArrowDown}");
      expect(screen.getByRole("option", { name: "Automations" })).toHaveAttribute("aria-selected", "true");
      // Row 3 ends at 156.
      expect(top()).toBe(56);
      await userEvent.keyboard("{ArrowUp}");
      // Back on row 1: row 0 starts at 0.
      expect(top()).toBe(0);
    });

    it("wrapping up to the last row lands at the bottom, instantly with reduced motion", async () => {
      const { scrollTo } = layout(true);
      await userEvent.keyboard("{ArrowUp}");
      // Row 17, the last, ends at 716.
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 616, behavior: "auto" });
    });

    it("a hover picks without scrolling", async () => {
      const { scrollTo } = layout(false);
      await userEvent.hover(screen.getByRole("option", { name: "Settings: About" }));
      expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute("aria-selected", "true");
      expect(scrollTo).not.toHaveBeenCalled();
    });
  });
});
