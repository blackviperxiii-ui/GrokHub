import "@testing-library/jest-dom/vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities } from "./api";

// The App's own chat listener (the Activity Dock); tests push events through it.
const chatHandlers = vi.hoisted(() => [] as ((envelope: unknown) => void)[]);
// The tray's Halt and Super+Shift+Esc reach the window through this.
const haltHandlers = vi.hoisted(() => [] as ((stopped: string[]) => void)[]);
const ranHandlers = vi.hoisted(() => [] as ((run: unknown) => void)[]);
const api = vi.hoisted(() => ({
  CORE_ONLY: { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false, library: false },
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
  getSettings: vi.fn(() => Promise.resolve({ theme: "light", desktopControl: false })),
  haltDesktop: vi.fn<() => Promise<string[]>>(() => Promise.resolve([])),
  slashHits: vi.fn(async (draft: string) =>
    [
      { cmd: "/plan", hint: "Plan first", insert: "/plan ", runOnPick: false, disabled: null },
      { cmd: "/play", hint: "", insert: "/play", runOnPick: true, disabled: "In the cabin" },
    ].filter((hit) => hit.cmd.startsWith(draft)),
  ),
  onChatEvent: vi.fn((handler: (envelope: unknown) => void) => {
    chatHandlers.push(handler);
    return Promise.resolve(() => {});
  }),
  onHalt: vi.fn((handler: (stopped: string[]) => void) => {
    haltHandlers.push(handler);
    return Promise.resolve(() => {});
  }),
  onAutomationRan: vi.fn((handler: (run: unknown) => void) => {
    ranHandlers.push(handler);
    return Promise.resolve(() => {});
  }),
  shareSchedulerClock: vi.fn(() => Promise.resolve()),
}));

vi.mock("./api", () => api);
// Each page has its own tests; here only the tabs matter.
vi.mock("./Chat", () => ({
  newConversationId: () => "shell-new",
  Chat: ({
    prefill,
    open,
    deleted,
    onSignIn,
  }: {
    prefill?: { text: string; n: number; conversationId?: string };
    open?: { id: string; n: number };
    deleted?: { id: string; n: number };
    onSignIn: () => void;
  }) => (
    <div>
      <p>Chat page{prefill && `: ${prefill.text} in ${prefill.conversationId ?? "this chat"}`}</p>
      {open && <p>Opened {open.id}</p>}
      {deleted && <p>Deleted {deleted.id}</p>}
      <button type="button" onClick={onSignIn}>
        Sign in
      </button>
    </div>
  ),
}));
vi.mock("./Journal", () => ({
  Journal: ({ onMission }: { onMission: (m: { conversationId: string; title: string; prompt: string }) => void }) => (
    <button type="button" onClick={() => onMission({ conversationId: "shell-m1", title: "Q", prompt: "Mission: Q" })}>
      To mission
    </button>
  ),
}));
vi.mock("./History", () => ({
  History: ({ onOpen, onDeleted }: { onOpen: (id: string) => void; onDeleted: (id: string) => void }) => (
    <div>
      <button type="button" onClick={() => onOpen("shell-old")}>
        Open old chat
      </button>
      <button type="button" onClick={() => onDeleted("shell-gone")}>
        Delete a chat
      </button>
    </div>
  ),
}));
vi.mock("./Automations", () => ({
  Automations: () => null,
  resultLine: (r: { text: string; error: string }) => r.error || r.text,
}));
vi.mock("./Memory", () => ({ Memory: () => null }));
vi.mock("./Connectors", () => ({ Connectors: () => null }));
vi.mock("./Pulse", () => ({ Pulse: () => <p>Pulse page</p> }));
vi.mock("./Library", () => ({ Library: () => <p>Library page</p> }));
vi.mock("./Settings", () => ({
  Settings: ({ caps, goTo }: { caps: Capabilities; goTo?: { section: string } }) => (
    <p>
      Settings for {caps.desktopControl ? "a desktop" : "a phone"}
      {goTo && ` at ${goTo.section}`}
    </p>
  ),
}));

import { App } from "./App";
import { HALT_DONE } from "./Halt";

const DESKTOP: Capabilities = {
  desktopControl: true,
  pulse: true,
  localAi: true,
  tray: true,
  webAccess: true,
  plugins: true,
  library: true,
};

function tabs() {
  const nav = screen.getByRole("navigation", { name: "Views" });
  return within(nav).getAllByRole("button").map((tab) => tab.textContent);
}

describe("App", () => {
  beforeEach(() => { api.getCapabilities.mockReset(); });

  it("shows every tab on a desktop and hands Settings the report", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    expect(await screen.findByText("Pulse page")).toBeInTheDocument();
    expect(tabs()).toEqual([
      "Chat",
      "History",
      "Pulse",
      "Journal",
      "Automations",
      "Memory",
      "Connectors",
      "Library",
      "Settings",
    ]);
    await userEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(await screen.findByText("Library page")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a desktop")).toBeInTheDocument();
  });

  it("puts the saved theme on the page at startup", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    delete document.documentElement.dataset.theme;
    render(<App />);
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
    delete document.documentElement.dataset.theme;
  });

  it("shows the brand mark and name in the top bar, and an icon on every tab", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    const { container } = render(<App />);
    const bar = container.querySelector(".topbar");
    expect(bar?.querySelector(".brand-mark")).toHaveTextContent("Loadout");
    expect(bar?.querySelector(".brand-mark img")).toHaveAttribute("alt", "");
    await screen.findByRole("button", { name: "Pulse" });
    const nav = screen.getByRole("navigation", { name: "Views" });
    for (const tab of within(nav).getAllByRole("button")) {
      expect(tab.querySelector("svg.tab-icon")).not.toBeNull();
      expect(tab).toHaveAttribute("title", tab.textContent);
    }
  });

  it("hides Pulse on a phone", async () => {
    api.getCapabilities.mockResolvedValue(api.CORE_ONLY);
    render(<App />);
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(tabs()).toEqual(["Chat", "History", "Journal", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("leaves the Journal and History off the web app", async () => {
    api.getCapabilities.mockResolvedValue({ ...api.CORE_ONLY, remote: true });
    render(<App />);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Journal" })).toBeNull());
    expect(tabs()).toEqual(["Chat", "Automations", "Memory", "Connectors", "Settings"]);
  });

  it("To mission puts the prompt in the mission's chat and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Journal" }));
    await userEvent.click(await screen.findByRole("button", { name: "To mission" }));
    expect(screen.getByText("Chat page: Mission: Q in shell-m1")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  it("a chat picked in History opens in Chat, and a deleted one is passed on", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "History" }));
    await userEvent.click(await screen.findByRole("button", { name: "Delete a chat" }));
    expect(screen.getByText("Deleted shell-gone")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open old chat" }));
    expect(screen.getByText("Opened shell-old")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  it("falls back to the core features when the report fails", async () => {
    api.getCapabilities.mockRejectedValue("no report");
    render(<App />);
    await waitFor(() =>
      expect(api.getCapabilities.mock.settledResults[0]?.type).toBe("rejected"),
    );
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a phone")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pulse" })).toBeNull();
  });

  it("Ctrl+K opens the palette, and a pick opens its page", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.keyboard("{Control>}k{/Control}");
    const dialog = screen.getByRole("dialog", { name: "Command palette" });
    expect(within(dialog).getByLabelText("Search pages and commands")).toHaveFocus();
    await userEvent.keyboard("local{Enter}");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(await screen.findByText("Settings for a desktop at localAi")).toBeVisible();
  });

  it("the Command button opens it too, and Ask Loadout fills the chat box", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    await userEvent.keyboard("tidy the board");
    await userEvent.click(screen.getByRole("option", { name: "Ask Loadout: tidy the board" }));
    expect(screen.getByText("Chat page: tidy the board in this chat")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  it("the chat's sign-in opens Settings at Account", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(await screen.findByText("Settings for a desktop at account")).toBeVisible();
  });

  it("Ctrl+N starts a new chat from any page", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Memory" }));
    await userEvent.keyboard("{Control>}n{/Control}");
    expect(screen.getByText("Chat page: in shell-new")).toBeVisible();
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  it("Ctrl+, opens Settings and Ctrl+/ toggles the shortcut sheet", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.keyboard("{Control>},{/Control}");
    expect(await screen.findByText("Settings for a desktop")).toBeVisible();
    await userEvent.keyboard("{Control>}/{/Control}");
    const sheet = screen.getByRole("dialog", { name: "Keyboard shortcuts" });
    expect(within(sheet).getByRole("row", { name: "Ctrl+N New chat" })).toBeInTheDocument();
    // Desktop control is in this shell, so Halt has its keys.
    expect(within(sheet).getByRole("row", { name: "Ctrl+Alt+H Halt" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("the palette's commands start a chat, sign in and show the keys", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    // Desktop control is in this shell, so the palette offers Halt.
    expect(screen.getByRole("option", { name: "Halt" })).toBeInTheDocument();
    await userEvent.keyboard("sign");
    await userEvent.click(await screen.findByRole("option", { name: "Sign in" }));
    expect(await screen.findByText("Settings for a desktop at account")).toBeVisible();
    await userEvent.keyboard("{Control>}k{/Control}");
    await userEvent.keyboard("keys{Enter}");
    expect(screen.getByRole("dialog", { name: "Keyboard shortcuts" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Close" }));
    await userEvent.keyboard("{Control>}k{/Control}");
    await userEvent.keyboard("new chat{Enter}");
    expect(screen.getByText("Chat page: in shell-new")).toBeVisible();
  });

  it("a slash query lists the composer's commands, and a pick fills the chat box", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    await userEvent.keyboard("/pl");
    expect(await screen.findByRole("option", { name: "/plan" })).toBeInTheDocument();
    // A command that can't run in this shell has no row.
    expect(screen.queryByRole("option", { name: "/play" })).toBeNull();
    await userEvent.keyboard("{Enter}");
    expect(screen.getByText("Chat page: /plan in this chat")).toBeVisible();
  });

  it("the Sign in row goes once signed in with Grok", async () => {
    api.getCapabilities.mockResolvedValue({ ...DESKTOP, grokSignIn: true });
    api.getSettings.mockResolvedValue({ theme: "light", hasApiKey: false, signedIn: { name: "user" } } as never);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    // The theme, Halt's desktop-control check, then the palette's sign-in check.
    await waitFor(() => expect(api.getSettings).toHaveBeenCalledTimes(3));
    await userEvent.keyboard("sign");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Ask Loadout: sign"]);
    api.getSettings.mockReset();
    api.getSettings.mockImplementation(() => Promise.resolve({ theme: "light", desktopControl: false }));
  });

  it("the Sign in row goes once the key is in", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    api.getSettings.mockResolvedValue({ theme: "light", hasApiKey: true } as never);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await userEvent.click(screen.getByRole("button", { name: "Command" }));
    // The theme, Halt's desktop-control check, then the palette's sign-in check.
    await waitFor(() => expect(api.getSettings).toHaveBeenCalledTimes(3));
    await userEvent.keyboard("sign");
    expect(screen.getAllByRole("option").map((o) => o.textContent)).toEqual(["Ask Loadout: sign"]);
    api.getSettings.mockReset();
    api.getSettings.mockImplementation(() => Promise.resolve({ theme: "light", desktopControl: false }));
  });

  it("the Activity Dock shows an approval waiting from any page and opens the chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    chatHandlers.length = 0;
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(screen.queryByRole("complementary", { name: "Activity" })).toBeNull();
    await waitFor(() => expect(chatHandlers.length).toBe(1));
    act(() => {
      chatHandlers[0]({
        conversationId: "shell-1",
        event: { kind: "permission", id: "p1", title: "Run npm test", action: "run", reason: "" },
      });
    });
    const dock = screen.getByRole("complementary", { name: "Activity" });
    await userEvent.click(within(dock).getByRole("button", { name: /Activity/ }));
    expect(within(dock).getByRole("listitem", { name: "Chat reply" })).toHaveTextContent(
      "Needs your OK: Run npm test",
    );
    await userEvent.click(within(dock).getByRole("button", { name: "Review" }));
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute("aria-pressed", "true");
  });

  describe("Halt", () => {
    beforeEach(() => {
      api.haltDesktop.mockClear();
      api.getSettings.mockResolvedValue({ theme: "light", desktopControl: false });
      chatHandlers.length = 0;
      haltHandlers.length = 0;
    });

    it("stays out of the top bar while desktop control is off and nothing runs", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      render(<App />);
      await screen.findByRole("button", { name: "Pulse" });
      await waitFor(() => expect(chatHandlers.length).toBe(1));
      expect(screen.queryByRole("button", { name: "Halt" })).toBeNull();
    });

    it("shows while a reply runs, and a press halts and says so", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      api.haltDesktop.mockResolvedValue(["shell-1"]);
      render(<App />);
      await waitFor(() => expect(chatHandlers.length).toBe(1));
      act(() => chatHandlers[0]({ conversationId: "shell-1", event: { kind: "text", text: "Typing" } }));

      await userEvent.click(await screen.findByRole("button", { name: "Halt" }));
      expect(api.haltDesktop).toHaveBeenCalledTimes(1);
      expect(await screen.findByRole("status")).toHaveTextContent(HALT_DONE);
      act(() => chatHandlers[0]({ conversationId: "shell-1", event: { kind: "done", stop: "halted" } }));
      expect(screen.queryByRole("button", { name: "Halt" })).toBeNull();
      await userEvent.click(screen.getByRole("button", { name: "Dismiss" }));
      expect(screen.queryByText(HALT_DONE)).toBeNull();
    });

    it("shows whenever desktop control is on", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      api.getSettings.mockResolvedValue({ theme: "light", desktopControl: true });
      render(<App />);
      expect(await screen.findByRole("button", { name: "Halt" })).toHaveClass("halt");
    });

    it("Ctrl+Alt+H halts from any page, button shown or not", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      render(<App />);
      await userEvent.click(await screen.findByRole("button", { name: "Memory" }));
      await userEvent.keyboard("{Control>}{Alt>}h{/Alt}{/Control}");
      expect(api.haltDesktop).toHaveBeenCalledTimes(1);
      expect(await screen.findByText(HALT_DONE)).toBeInTheDocument();
      await userEvent.keyboard("{Control>}h{/Control}");
      expect(api.haltDesktop).toHaveBeenCalledTimes(1);
    });

    it("a Halt from the tray or Super+Shift+Esc shows the same note", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      render(<App />);
      await waitFor(() => expect(haltHandlers.length).toBe(1));
      act(() => haltHandlers[0]([]));
      expect(screen.getByRole("status")).toHaveTextContent(HALT_DONE);
      expect(api.haltDesktop).not.toHaveBeenCalled();
    });

    it("a failed Halt says so", async () => {
      api.getCapabilities.mockResolvedValue(DESKTOP);
      api.haltDesktop.mockRejectedValueOnce("The window is closing.");
      api.getSettings.mockResolvedValue({ theme: "light", desktopControl: true });
      render(<App />);
      await userEvent.click(await screen.findByRole("button", { name: "Halt" }));
      expect(await screen.findByRole("status")).toHaveTextContent("Halt didn't go through: The window is closing.");
    });

    it("the web app and phones get no Halt, and the keys do nothing", async () => {
      for (const caps of [{ ...api.CORE_ONLY, remote: true }, { ...DESKTOP, remote: true }, api.CORE_ONLY]) {
        api.getCapabilities.mockResolvedValue(caps);
        api.getSettings.mockResolvedValue({ theme: "light", desktopControl: true });
        const { unmount } = render(<App />);
        await screen.findByRole("button", { name: "Settings" });
        await waitFor(() => expect(api.getCapabilities).toHaveBeenCalled());
        await userEvent.keyboard("{Control>}{Alt>}h{/Alt}{/Control}");
        expect(screen.queryByRole("button", { name: "Halt" })).toBeNull();
        unmount();
      }
      expect(api.haltDesktop).not.toHaveBeenCalled();
    });
  });

  it("shares the clock with the scheduler and shows a scheduled run in the dock", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    ranHandlers.length = 0;
    api.shareSchedulerClock.mockClear();
    render(<App />);
    expect(api.shareSchedulerClock).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(ranHandlers.length).toBe(1));
    act(() => {
      ranHandlers[0]({
        id: "auto-1",
        name: "summarize my inbox",
        result: { text: "3 new mails.", error: "", stop: "end_turn" },
      });
    });
    const dock = screen.getByRole("complementary", { name: "Activity" });
    await userEvent.click(within(dock).getByRole("button", { name: /Activity/ }));
    expect(within(dock).getByRole("listitem", { name: "summarize my inbox" })).toHaveTextContent(
      "3 new mails.",
    );
  });
});
