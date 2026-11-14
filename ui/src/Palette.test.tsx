import "@testing-library/jest-dom/vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, QuickChip } from "./api";
import { hasPage, moreViews, NO_SOURCES, Palette, paletteRows, railViews, type PaletteExtras, type PaletteSources } from "./Palette";
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
  autoSetup: true,
};
const PHONE: Capabilities = { ...DESKTOP, desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false, library: false, autoSetup: false };
const WEB: Capabilities = { ...PHONE, remote: true };

const PLAN: QuickChip = { id: "pred-1", label: "Plan my day", value: "Plan my day", action: "ask", hint: "", primary: true };
const NIGHT: QuickChip = { id: "nav-1", label: "Automations", value: "__nav:night", action: "go", hint: "", primary: false, view: "automations" };
const HISTORY: QuickChip = { id: "nav-2", label: "History", value: "__nav:history", action: "go", hint: "", primary: false, view: "history" };

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
      "Home",
      "Objectives",
      "Chat",
      "History",
      "Pulse",
      "Imagine",
      "Journal",
      "Automations",
      "Agents",
      "Personal",
      "Skills",
      "Connectors",
      "Workboard",
      "Devices",
      "Library",
      "Updates & Drivers",
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
      "Settings: Auto setup",
      "Settings: PC helpers",
      "Settings: Pulse",
      "Settings: Local AI",
      "Settings: AI models",
      "Settings: Web access",
      "Settings: Labs",
      "Settings: About",
    ]);
    expect(labels(PHONE, "")).toEqual([
      "Home",
      "Objectives",
      "Chat",
      "History",
      "Imagine",
      "Journal",
      "Automations",
      "Agents",
      "Personal",
      "Skills",
      "Connectors",
      "Workboard",
      "Updates & Drivers",
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
    expect(labels(WEB, "")).toEqual(["Home", "Objectives", "Chat", "Imagine", "Automations", "Agents", "Personal", "Skills", "Connectors", "Workboard", "Settings"]);
  });

  it("lists Mods right after Library on a desktop that has it, never on the web", () => {
    const rows = labels({ ...DESKTOP, mods: true }, "");
    expect(rows.slice(rows.indexOf("Library"), rows.indexOf("Library") + 2)).toEqual(["Library", "Mods"]);
    expect(labels(DESKTOP, "")).not.toContain("Mods");
    expect(labels({ ...WEB, mods: true }, "")).not.toContain("Mods");
  });

  it("lists the Code tab and its Settings page when the Code tab is on", () => {
    const rows = labels({ ...DESKTOP, code: true }, "");
    expect(rows.slice(rows.indexOf("Chat"), rows.indexOf("Chat") + 2)).toEqual(["Chat", "Code"]);
    expect(rows).toContain("Settings: Code");
    expect(labels(DESKTOP, "")).not.toContain("Settings: Code");
  });

  it("opens the Command line right after Code, on the desktop only", () => {
    const rows = labels({ ...DESKTOP, code: true }, "");
    expect(rows.slice(rows.indexOf("Code"), rows.indexOf("Code") + 3)).toEqual(["Code", "Command line", "History"]);
    expect(labels(DESKTOP, "")).not.toContain("Command line");
    expect(labels({ ...WEB, code: true }, "")).not.toContain("Command line");
    expect(labels({ ...DESKTOP, code: true }, "command")).toContain("Command line");
  });

  it("puts the commands this shell can run after the pages", () => {
    // The commands follow the last page, however many pages this shell has.
    const afterPages = (rows: string[], n: number) =>
      rows.slice(rows.indexOf("Settings") + 1, rows.indexOf("Settings") + 1 + n);
    const rows = withExtras(DESKTOP, "");
    const pages = labels(DESKTOP, "");
    expect(rows.slice(0, rows.indexOf("Settings") + 1)).toEqual(pages.slice(0, pages.indexOf("Settings") + 1));
    expect(afterPages(rows, 4)).toEqual(["New chat", "Halt", "Sign in", "Shortcuts"]);
    // Only the runners the shell hands over get rows.
    expect(afterPages(withExtras(DESKTOP, "", { ...EXTRAS, runners: { shortcuts: run } }), 2)).toEqual([
      "Shortcuts",
      "Settings: General",
    ]);
  });

  it("hides Halt without desktop control, and Halt and Sign in on the web", () => {
    expect(withExtras(PHONE, "").slice(14, 17)).toEqual(["New chat", "Sign in", "Shortcuts"]);
    expect(withExtras(WEB, "")).toEqual([
      "Home",
      "Objectives",
      "Chat",
      "Imagine",
      "Automations",
      "Agents",
      "Personal",
      "Skills",
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
    expect(rows.slice(17, 23)).toEqual([
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

describe("hasPage", () => {
  it("Devices opens only on a desktop with Web access, and Updates only on this computer", () => {
    expect(hasPage(DESKTOP, "devices")).toBe(true);
    expect(hasPage(PHONE, "devices")).toBe(false);
    expect(hasPage({ ...DESKTOP, remote: true }, "devices")).toBe(false);
    expect(hasPage(DESKTOP, "updates")).toBe(true);
    expect(hasPage(WEB, "updates")).toBe(false);
    expect(hasPage(WEB, "workboard")).toBe(true);
    expect(hasPage(WEB, "history")).toBe(false);
  });
});

describe("the rail and its More list", () => {
  it("puts six pages on the rail and the rest under More, by platform", () => {
    expect(railViews(DESKTOP)).toEqual(["home", "chat", "pulse", "library", "settings"]);
    expect(railViews({ ...DESKTOP, code: true })).toEqual(["home", "chat", "code", "pulse", "library", "settings"]);
    expect(railViews(WEB)).toEqual(["home", "chat", "settings"]);
    expect(moreViews(DESKTOP)).toEqual([
      "objectives",
      "history",
      "imagine",
      "journal",
      "automations",
      "agents",
      "memory",
      "skills",
      "connectors",
      "workboard",
      "devices",
      "updates",
    ]);
    expect(moreViews(WEB)).toEqual(["objectives", "imagine", "automations", "agents", "memory", "skills", "connectors", "workboard"]);
  });

  it("lists only the given pages: no suggestions, commands, chats, sections or Ask", () => {
    const extras = { ...EXTRAS, chips: [PLAN], chats: [{ id: "c1", title: "Journal trip" }], only: moreViews(DESKTOP) };
    expect(withExtras(DESKTOP, "", extras)).toEqual([
      "Objectives",
      "History",
      "Imagine",
      "Journal",
      "Automations",
      "Agents",
      "Personal",
      "Skills",
      "Connectors",
      "Workboard",
      "Devices",
      "Updates & Drivers",
    ]);
    expect(withExtras(DESKTOP, "jour", extras)).toEqual(["Journal"]);
    expect(withExtras(DESKTOP, "zzz", extras)).toEqual([]);
  });

  it("opens as More pages, and an empty match says so without picking", async () => {
    const onPick = vi.fn();
    const onClose = vi.fn();
    render(<Palette caps={DESKTOP} only={moreViews(DESKTOP)} onPick={onPick} onClose={onClose} />);
    expect(screen.getByRole("dialog", { name: "More pages" })).toBeInTheDocument();
    const search = screen.getByRole("combobox", { name: "Search pages" });
    expect(search).toHaveAttribute("aria-activedescendant", "palette-row-0");
    await userEvent.type(search, "zzz");
    expect(screen.queryAllByRole("option")).toEqual([]);
    expect(screen.getByText("No page matches “zzz”.")).toBeInTheDocument();
    expect(search).not.toHaveAttribute("aria-activedescendant");
    await userEvent.keyboard("{ArrowDown}{ArrowUp}{Enter}");
    expect(onPick).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    await userEvent.clear(search);
    await userEvent.type(search, "upd{Enter}");
    expect(onPick).toHaveBeenCalledExactlyOnceWith({ kind: "view", view: "updates" });
  });
});

describe("paletteRows suggestions", () => {
  it("lead the empty palette, drop a page this app lacks, and go once something is typed", () => {
    const extras = { ...EXTRAS, chips: [PLAN, NIGHT, HISTORY] };
    const rows = paletteRows(WEB, "", extras);
    expect(rows.slice(0, 2)).toEqual([
      { label: "Suggested: Plan my day", chip: PLAN, action: { kind: "ask", text: "Plan my day" } },
      { label: "Suggested: Automations", chip: NIGHT, action: { kind: "view", view: "automations" } },
    ]);
    expect(rows.map((r) => r.label)).not.toContain("Suggested: History");
    expect(paletteRows(WEB, "plan", extras).some((r) => r.chip)).toBe(false);
  });
});

describe("shellKeys", () => {
  it("lists the palette, the commands the shell can run, Settings, the chat and the chat box", () => {
    expect(shellKeys(DESKTOP, { "new-chat": run, shortcuts: run, "sign-in": run })).toEqual([
      { keys: "Ctrl+K", action: "Command palette" },
      { keys: "Ctrl+N", action: "New chat" },
      { keys: "Ctrl+/", action: "This sheet" },
      { keys: "Ctrl+,", action: "Settings" },
      { keys: "Ctrl+F", action: "Find in this chat (Enter next, Shift+Enter previous, Esc close)" },
      { keys: "Ctrl+Shift+A", action: "Archive or unarchive this chat" },
      { keys: "Enter", action: "Send message" },
      { keys: "Shift+Enter", action: "New line" },
      { keys: "Esc", action: "Close the palette or this sheet" },
    ]);
    expect(shellKeys(DESKTOP, ALL_RUNNERS)[2]).toEqual({ keys: "Ctrl+Alt+H", action: "Halt" });
    expect(shellKeys(PHONE, ALL_RUNNERS).map((k) => k.keys)).not.toContain("Ctrl+Alt+H");
    // The web app finds in the chat but has no chat tabs to archive.
    const web = shellKeys({ ...DESKTOP, remote: true }, ALL_RUNNERS).map((k) => k.keys);
    expect(web).toContain("Ctrl+F");
    expect(web).not.toContain("Ctrl+Shift+A");
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

  it("a picked suggestion is taught and then acts", async () => {
    const onPick = vi.fn();
    const onChip = vi.fn();
    render(
      <Palette caps={PHONE} sources={{ ...NO_SOURCES, chips: [PLAN], onChip }} onPick={onPick} onClose={vi.fn()} />,
    );
    expect(screen.getByRole("option", { name: "Suggested: Plan my day" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{Enter}");
    expect(onChip).toHaveBeenCalledWith(PLAN);
    expect(onPick).toHaveBeenCalledWith({ kind: "ask", text: "Plan my day" });
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
    expect(screen.getByRole("option", { name: "Home" })).toHaveAttribute("aria-selected", "true");
    await userEvent.keyboard("{ArrowUp}");
    expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await userEvent.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(onPick).toHaveBeenCalledWith({ kind: "view", view: "objectives" });
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
      expect(screen.getByRole("option", { name: "Chat" })).toHaveAttribute("aria-selected", "true");
      // Row 3 ends at 156.
      expect(top()).toBe(56);
      await userEvent.keyboard("{ArrowUp}");
      // Back on row 1: row 0 starts at 0.
      expect(top()).toBe(0);
    });

    it("wrapping up to the last row lands at the bottom, instantly with reduced motion", async () => {
      const { scrollTo } = layout(true);
      await userEvent.keyboard("{ArrowUp}");
      // Row 24, the last, ends at 996.
      expect(scrollTo).toHaveBeenLastCalledWith({ top: 896, behavior: "auto" });
    });

    it("a hover picks without scrolling", async () => {
      const { scrollTo } = layout(false);
      await userEvent.hover(screen.getByRole("option", { name: "Settings: About" }));
      expect(screen.getByRole("option", { name: "Settings: About" })).toHaveAttribute("aria-selected", "true");
      expect(scrollTo).not.toHaveBeenCalled();
    });
  });
});
