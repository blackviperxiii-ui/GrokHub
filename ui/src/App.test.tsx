import "@testing-library/jest-dom/vitest";
import { Profiler } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Capabilities, RecordStatus } from "./api";

// The App's own chat listener (the Activity Dock); tests push events through it.
const chatHandlers = vi.hoisted(() => [] as ((envelope: unknown) => void)[]);
// The tray's Halt and Super+Shift+Esc reach the window through this.
const haltHandlers = vi.hoisted(() => [] as ((stopped: string[]) => void)[]);
const ranHandlers = vi.hoisted(() => [] as ((run: unknown) => void)[]);
const updateHandlers = vi.hoisted(() => [] as ((update: unknown) => void)[]);
const api = vi.hoisted(() => ({
  CORE_ONLY: { desktopControl: false, pulse: false, localAi: false, tray: false, webAccess: false, plugins: false, library: false },
  WEB: { desktopControl: false, pulse: true, localAi: false, tray: false, webAccess: false, plugins: false, library: false, remote: true },
  getCapabilities: vi.fn<() => Promise<Capabilities>>(),
  getSettings: vi.fn(() => Promise.resolve({ theme: "light", desktopControl: false })),
  haltDesktop: vi.fn<() => Promise<string[]>>(() => Promise.resolve([])),
  slashHits: vi.fn(async (draft: string) =>
    [
      { cmd: "/plan", hint: "Plan first", insert: "/plan ", runOnPick: false, disabled: null },
      { cmd: "/play", hint: "", insert: "/play", runOnPick: true, disabled: "In the cabin" },
    ].filter((hit) => hit.cmd.startsWith(draft)),
  ),
  // One card, worked by chat "shell-c1", so its dock item can open it.
  getWorkboard: vi.fn(() =>
    Promise.resolve({
      columns: [{ lane: "blocked", label: "Needs Attention", cards: [{ id: "w1", chat: "shell-c1" }] }],
      archived: [],
      stamp: 1,
    }),
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
  onUpdateAvailable: vi.fn((handler: (update: unknown) => void) => {
    updateHandlers.push(handler);
    return Promise.resolve(() => {});
  }),
  onLocalEngineEvent: vi.fn(() => Promise.resolve(() => {})),
  workboardSweepUndo: vi.fn(() => Promise.resolve({})),
  listChats: vi.fn(() => Promise.resolve([{ id: "shell-c9", title: "Trip plans", created: 1, updated: 2 }])),
  shareSchedulerClock: vi.fn(() => Promise.resolve()),
  quickChips: vi.fn(() => Promise.resolve([])),
  chipOutcome: vi.fn(() => Promise.resolve(true)),
  recordStatus: vi.fn<() => Promise<RecordStatus>>(() => Promise.resolve({ recording: null, listening: null, events: [] })),
  recordStop: vi.fn(() => Promise.resolve(false)),
}));

vi.mock("./api", async () => {
  const { chatTabApiStubs } = await import("./chatTabApiStubs");
  return { ...chatTabApiStubs(), ...api };
});
// Each page has its own tests; here only the tabs matter.
vi.mock("./Chat", () => ({
  newConversationId: () => "shell-new",
  Chat: ({
    prefill,
    open,
    deleted,
    clear,
    onSignIn,
    onQueue,
    queueAct,
  }: {
    onQueue?: (conversationId: string, texts: string[]) => void;
    queueAct?: { op: string; index: number; n: number };
    prefill?: { text: string; n: number; conversationId?: string; send?: boolean };
    open?: { id: string; n: number };
    deleted?: { id: string; ids?: string[]; n: number };
    clear?: { n: number };
    onSignIn: () => void;
  }) => (
    <div>
      <p>Chat page{prefill && `: ${prefill.text} in ${prefill.conversationId ?? "this chat"}`}</p>
      {prefill?.send && <p>Sends {prefill.text}</p>}
      {open && <p>Opened {open.id}</p>}
      {deleted && <p>Deleted {deleted.id}</p>}
      {deleted && <p>Every deleted chat: {(deleted.ids ?? []).join(", ")}</p>}
      {clear && <p>Cleared {clear.n}</p>}
      {queueAct && (
        <p>
          Act {queueAct.op} {queueAct.index}
        </p>
      )}
      <button type="button" onClick={() => onQueue?.("shell-c1", ["then deploy", "and tell me"])}>
        Line up two
      </button>
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
      <button
        type="button"
        onClick={() => {
          // Delete all reports every chat it deleted in one tick.
          for (const id of ["shell-a", "shell-b", "shell-c"]) onDeleted(id);
        }}
      >
        Delete three chats
      </button>
    </div>
  ),
}));
vi.mock("./Workboard", () => ({
  Workboard: ({
    onWork,
    resumable = true,
    openCard,
    onResume,
  }: {
    onWork: (prompt: string) => void;
    resumable?: boolean;
    openCard?: { id: string };
    onResume?: (r: { conversationId: string; prompt: string }) => void;
  }) => (
    <div>
      <p>
        Workboard page{resumable ? "" : " without Resume"}
        {openCard && ` at ${openCard.id}`}
      </p>
      <button type="button" onClick={() => onWork("Work on this workboard card: Ship it")}>
        Work
      </button>
      <button type="button" onClick={() => onResume?.({ conversationId: "chat-w1", prompt: "Resume the work card" })}>
        Resume
      </button>
    </div>
  ),
}));
vi.mock("./Automations", () => ({
  Automations: () => null,
  resultLine: (r: { text: string; error: string }) => r.error || r.text,
}));
const memoryThrows = vi.hoisted(() => ({ current: false }));
vi.mock("./Memory", () => ({
  Memory: () => {
    if (memoryThrows.current) throw new Error("Corrupt vector index");
    return <p>Memory page</p>;
  },
}));
vi.mock("./Code", () => ({
  Code: (p: {
    shown: boolean;
    chatSession?: string;
    onShowChat?: () => void;
    onSession?: (id: string) => void;
    deleted?: { id: string };
  }) => (
    <div hidden={!p.shown}>
      <p>Code page</p>
      {p.deleted && <p>Code heard deleted {p.deleted.id}</p>}
      <button type="button" onClick={() => p.onSession?.("shell-old")}>
        Agent window shows shell-old
      </button>
      <button type="button" onClick={p.onShowChat}>
        That session is in Chat
      </button>
    </div>
  ),
}));
vi.mock("./Agents", () => ({
  Agents: ({
    conversationId,
    onOpenChat,
    onRun,
  }: {
    conversationId?: string;
    onOpenChat: () => void;
    onRun?: (prompt: string, threadId: string) => void;
  }) => (
    <>
      <button type="button" onClick={onOpenChat}>
        Agents for {conversationId ?? "no chat"}
      </button>
      <button type="button" onClick={() => onRun?.("Summarize the logs", "shell-t7")}>
        Run queued job
      </button>
    </>
  ),
}));
vi.mock("./Connectors", () => ({ Connectors: () => null }));
vi.mock("./Devices", () => ({
  Devices: ({ onOpenWebAccess, onRun }: { onOpenWebAccess: () => void; onRun?: (text: string) => void }) => (
    <>
      <button type="button" onClick={onOpenWebAccess}>
        Devices page
      </button>
      <button type="button" onClick={() => onRun?.("tidy my downloads")}>
        Devices run
      </button>
    </>
  ),
}));
vi.mock("./Skills", () => ({
  Skills: ({ onUse }: { onUse: (text: string) => void }) => (
    <button type="button" onClick={() => onUse("deploy the site")}>
      Use
    </button>
  ),
}));
vi.mock("./Home", () => ({
  Home: ({ onOpen, conversationId }: { onOpen: (view: string) => void; conversationId?: string }) => (
    <div>
      <p>Home page for {conversationId ?? "the last chat"}</p>
      <button type="button" onClick={() => onOpen("objectives")}>
        Objectives tile
      </button>
    </div>
  ),
}));
vi.mock("./Objectives", () => ({
  Objectives: ({ onHome }: { onHome: () => void }) => (
    <button type="button" onClick={onHome}>
      Objectives page
    </button>
  ),
}));
vi.mock("./Pulse", () => ({ Pulse: () => <p>Pulse page</p> }));
vi.mock("./Imagine", () => ({ Imagine: () => <p>Imagine page</p> }));
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
import { dispatch } from "./transport";
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

/** The rail's items, top to bottom, by their (visually hidden) labels. */
function rail() {
  const nav = screen.getByRole("navigation", { name: "Views" });
  return within(nav).getAllByRole("button").map((item) => item.textContent);
}

/** One rail item. */
function view(name: string) {
  return within(screen.getByRole("navigation", { name: "Views" })).getByRole("button", { name });
}

/** The pages More lists, in order; the list is closed again after. */
async function morePages() {
  await userEvent.click(view("More"));
  const more = screen.getByRole("dialog", { name: "More pages" });
  const names = within(more).getAllByRole("option").map((row) => row.textContent);
  await userEvent.keyboard("{Escape}");
  return names;
}

/** Opens a page that is not on the rail, through More. */
async function openPage(name: string) {
  await userEvent.click(view("More"));
  const more = screen.getByRole("dialog", { name: "More pages" });
  await userEvent.click(await within(more).findByRole("option", { name }));
}

describe("App", () => {
  beforeEach(() => { api.getCapabilities.mockReset(); });

  it("puts the main pages on the rail and the rest under More on a desktop, and hands Settings the report", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Pulse" }));
    expect(await screen.findByText("Pulse page")).toBeInTheDocument();
    expect(rail()).toEqual(["Home", "Chat", "Pulse", "Library", "More", "Settings"]);
    expect(await morePages()).toEqual([
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
    // Escape closed More and gave the focus back to it.
    expect(screen.queryByRole("dialog", { name: "More pages" })).toBeNull();
    expect(view("More")).toHaveFocus();
    expect(view("More")).toHaveAttribute("aria-haspopup", "dialog");
    await userEvent.click(screen.getByRole("button", { name: "Library" }));
    expect(await screen.findByText("Library page")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Settings" }));
    expect(await screen.findByText("Settings for a desktop")).toBeInTheDocument();
  });

  it("opens on Chat with Home as the first tab, and Home's tile opens Objectives", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    expect(rail()[0]).toBe("Home");
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
    expect(screen.queryByText(/Home page/)).toBeNull();
    // Objectives is not on the rail; Home, More and the palette open it.
    expect(rail()).not.toContain("Objectives");
    await userEvent.click(screen.getByRole("button", { name: "Home" }));
    expect(await screen.findByText("Home page for the last chat")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Objectives tile" }));
    await userEvent.click(await screen.findByRole("button", { name: "Objectives page" }));
    expect(await screen.findByText("Home page for the last chat")).toBeVisible();
    expect(view("Home")).toHaveAttribute("aria-current", "page");
  });

  it("gives a page opened from More a rail slot while it is open, so one item always marks where you are", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    await openPage("Workboard");
    expect(rail()).toEqual(["Home", "Chat", "Pulse", "Library", "Workboard", "More", "Settings"]);
    expect(view("Workboard")).toHaveAttribute("aria-current", "page");
    const nav = screen.getByRole("navigation", { name: "Views" });
    expect(nav.querySelectorAll("[aria-current]")).toHaveLength(1);
    await userEvent.click(view("Chat"));
    expect(rail()).toEqual(["Home", "Chat", "Pulse", "Library", "More", "Settings"]);
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
    expect(nav.querySelectorAll("[aria-current]")).toHaveLength(1);
  });

  it("moves through the rail with the arrow keys, Home and End", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Pulse" });
    view("Home").focus();
    await userEvent.keyboard("{ArrowDown}");
    expect(view("Chat")).toHaveFocus();
    await userEvent.keyboard("{End}");
    expect(view("Settings")).toHaveFocus();
    await userEvent.keyboard("{ArrowDown}");
    expect(view("Home")).toHaveFocus();
    await userEvent.keyboard("{ArrowUp}");
    expect(view("Settings")).toHaveFocus();
    await userEvent.keyboard("{Home}");
    expect(view("Home")).toHaveFocus();
    await userEvent.keyboard("{Enter}");
    expect(await screen.findByText("Home page for the last chat")).toBeVisible();
  });

  it("opens Devices, whose Web access link goes to that Settings section", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Devices");
    await userEvent.click(await screen.findByRole("button", { name: "Devices page" }));
    expect(await screen.findByText("Settings for a desktop at webAccess")).toBeInTheDocument();
  });

  it("Run it here on Devices opens Chat and sends the task", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Devices");
    await userEvent.click(await screen.findByRole("button", { name: "Devices run" }));
    expect(await screen.findByText("Sends tidy my downloads")).toBeInTheDocument();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
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
    expect(rail()).toEqual(["Home", "Chat", "More", "Settings"]);
    expect(await morePages()).toEqual([
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
      "Updates & Drivers",
    ]);
  });

  it("leaves the Journal and History off the web app and keeps Resume on the desktop", async () => {
    api.getCapabilities.mockResolvedValue({ ...api.CORE_ONLY, remote: true });
    render(<App />);
    await waitFor(() => expect(api.getCapabilities).toHaveBeenCalled());
    expect(rail()).toEqual(["Home", "Chat", "More", "Settings"]);
    expect(await morePages()).toEqual(["Objectives", "Imagine", "Automations", "Agents", "Personal", "Skills", "Connectors", "Workboard"]);
    await openPage("Workboard");
    expect(await screen.findByText("Workboard page without Resume")).toBeVisible();
  });

  it("Resume on the Workboard sends the resume block once in the card's chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Workboard");
    await userEvent.click(await screen.findByRole("button", { name: "Resume" }));
    expect(screen.getByText("Chat page: Resume the work card in chat-w1")).toBeVisible();
    expect(screen.getByText("Sends Resume the work card")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("To mission puts the prompt in the mission's chat and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Journal");
    await userEvent.click(await screen.findByRole("button", { name: "To mission" }));
    expect(screen.getByText("Chat page: Mission: Q in shell-m1")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("every chat Delete all deleted reaches Chat, not only the last one", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("History");
    await userEvent.click(await screen.findByRole("button", { name: "Delete a chat" }));
    await userEvent.click(screen.getByRole("button", { name: "Delete three chats" }));
    expect(screen.getByText("Deleted shell-c")).toBeInTheDocument();
    expect(
      screen.getByText("Every deleted chat: shell-gone, shell-a, shell-b, shell-c"),
    ).toBeInTheDocument();
  });

  it("a chat picked in History opens in Chat, and a deleted one is passed on", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("History");
    await userEvent.click(await screen.findByRole("button", { name: "Delete a chat" }));
    expect(screen.getByText("Deleted shell-gone")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Open old chat" }));
    expect(screen.getByText("Opened shell-old")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("right-clicking the Chat tab offers Clear chat, which asks first and says it's permanent", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    const tab = await screen.findByRole("button", { name: "Chat" });
    await userEvent.pointer({ keys: "[MouseRight]", target: tab });
    await userEvent.click(screen.getByRole("menuitem", { name: "Clear chat…" }));
    const ask = screen.getByRole("alertdialog", { name: "Clear this chat?" });
    expect(within(ask).getByText(/This is permanent/)).toBeVisible();
    await userEvent.click(within(ask).getByRole("button", { name: "Keep" }));
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(screen.queryByText(/Cleared/)).toBeNull();

    await userEvent.pointer({ keys: "[MouseRight]", target: tab });
    await userEvent.click(screen.getByRole("menuitem", { name: "Clear chat…" }));
    await userEvent.click(screen.getByRole("button", { name: "Clear chat" }));
    expect(screen.getByText("Cleared 1")).toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).toBeNull();
  });

  it("the web app's Chat tab has no Clear chat", async () => {
    api.getCapabilities.mockResolvedValue({ ...api.CORE_ONLY, remote: true });
    render(<App />);
    const tab = await screen.findByRole("button", { name: "Chat" });
    await userEvent.pointer({ keys: "[MouseRight]", target: tab });
    expect(screen.queryByRole("menuitem", { name: "Clear chat…" })).toBeNull();
  });

  it("a chat the Code tab's agent window has open shows there, not in Chat as a second copy", async () => {
    api.getCapabilities.mockResolvedValue({ ...DESKTOP, code: true });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Code" }));
    await userEvent.click(await screen.findByRole("button", { name: "Agent window shows shell-old" }));
    await openPage("History");
    await userEvent.click(await screen.findByRole("button", { name: "Open old chat" }));
    expect(view("Code")).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("Code page")).toBeVisible();
    expect(screen.queryByText("Opened shell-old")).not.toBeInTheDocument();
    // A delete in History reaches the agent window too.
    await openPage("History");
    await userEvent.click(screen.getByRole("button", { name: "Delete a chat" }));
    expect(screen.getByText("Code heard deleted shell-gone")).toBeInTheDocument();
    // A session the Chat tab holds is shown there.
    await userEvent.click(screen.getByRole("button", { name: "Code" }));
    await userEvent.click(screen.getByRole("button", { name: "That session is in Chat" }));
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("Run on a queued job opens Chat in the job's chat and sends its prompt", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Agents");
    await userEvent.click(await screen.findByRole("button", { name: "Run queued job" }));
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("Chat page: Summarize the logs in shell-t7")).toBeInTheDocument();
    expect(screen.getByText("Sends Summarize the logs")).toBeInTheDocument();
  });

  it("Agents does not follow the agent window's session, which has its own Tasks panel", async () => {
    api.getCapabilities.mockResolvedValue({ ...DESKTOP, code: true });
    render(<App />);
    await userEvent.click(await screen.findByRole("button", { name: "Code" }));
    await userEvent.click(await screen.findByRole("button", { name: "Agent window shows shell-old" }));
    await openPage("Agents");
    act(() => {
      for (const handler of chatHandlers) handler({ conversationId: "shell-old", event: { kind: "text", text: "hi" } });
    });
    expect(await screen.findByRole("button", { name: "Agents for no chat" })).toBeVisible();
    act(() => {
      for (const handler of chatHandlers) handler({ conversationId: "shell-c7", event: { kind: "text", text: "hi" } });
    });
    expect(screen.getByRole("button", { name: "Agents for shell-c7" })).toBeVisible();
  });

  it("Agents follows the chat the window last heard from and links back to Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Agents");
    expect(await screen.findByRole("button", { name: "Agents for no chat" })).toBeVisible();
    act(() => {
      for (const handler of chatHandlers) handler({ conversationId: "shell-c7", event: { kind: "text", text: "hi" } });
    });
    await userEvent.click(screen.getByRole("button", { name: "Agents for shell-c7" }));
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("Work puts the card's prompt in the chat box and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Workboard");
    await userEvent.click(await screen.findByRole("button", { name: "Work" }));
    expect(screen.getByText("Chat page: Work on this workboard card: Ship it in this chat")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
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
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("a skill's Use puts its trigger in the chat box and opens Chat", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await openPage("Skills");
    await userEvent.click(await screen.findByRole("button", { name: "Use" }));
    expect(screen.getByText("Chat page: deploy the site in this chat")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
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
    await openPage("Personal");
    await userEvent.keyboard("{Control>}n{/Control}");
    expect(screen.getByText("Chat page: in shell-new")).toBeVisible();
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
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
    expect(within(dock).queryByRole("button", { name: "Open card" })).toBeNull();
    await userEvent.click(within(dock).getByRole("button", { name: "Review" }));
    expect(view("Chat")).toHaveAttribute("aria-current", "page");
  });

  it("a dock item whose chat works a card opens that card on the Workboard", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    chatHandlers.length = 0;
    render(<App />);
    await waitFor(() => expect(chatHandlers.length).toBe(1));
    await waitFor(() => expect(api.getWorkboard).toHaveBeenCalled());
    act(() => {
      chatHandlers[0]({ conversationId: "shell-c1", event: { kind: "text", text: "On it" } });
    });
    const dock = screen.getByRole("complementary", { name: "Activity" });
    await userEvent.click(within(dock).getByRole("button", { name: /Activity/ }));
    await userEvent.click(within(dock).getByRole("button", { name: "Open card" }));
    expect(await screen.findByText("Workboard page at w1")).toBeVisible();
    expect(view("Workboard")).toHaveAttribute("aria-current", "page");
  });

  it("streamed reply text doesn't re-render the app per chunk", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    chatHandlers.length = 0;
    let commits = 0;
    render(
      <Profiler id="app" onRender={() => (commits += 1)}>
        <App />
      </Profiler>,
    );
    await waitFor(() => expect(chatHandlers.length).toBe(1));
    act(() => {
      chatHandlers[0]({ conversationId: "shell-c9", event: { kind: "text", text: "Hi" } });
    });
    const before = commits;
    // Chunks arrive in separate tasks, as a stream does: one act each.
    for (let i = 0; i < 100; i++) {
      act(() => {
        chatHandlers[0]({ conversationId: "shell-c9", event: { kind: "text", text: ` word${i}` } });
      });
    }
    expect(commits - before).toBeLessThanOrEqual(2);
  });

  it("keeps the bar, with the recording's clock and Stop, in a narrow window while a recording runs", async () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn((query: string) => ({
        matches: query === "(max-width: 600px)",
        addEventListener: () => {},
        removeEventListener: () => {},
      })),
    );
    api.getCapabilities.mockResolvedValue(DESKTOP);
    // The user scrolls the page down, far enough for the bar to slide away.
    const swipeDown = (page: HTMLElement) => {
      for (const top of [50, 200]) {
        fireEvent.touchMove(page);
        page.scrollTop = top;
        fireEvent.scroll(page);
      }
    };
    try {
      const idle = render(<App />);
      swipeDown(await screen.findByText(/^Chat page/));
      expect(idle.container.querySelector(".app")).toHaveClass("bar-away");
      idle.unmount();

      api.recordStatus.mockResolvedValue({
        recording: { secs: 42, text: "● Recording 0:42 of 2:00" },
        listening: null,
        events: [],
      });
      const live = render(<App />);
      expect(await screen.findByText("● Recording 0:42 of 2:00")).toBeInTheDocument();
      swipeDown(screen.getByText(/^Chat page/));
      expect(live.container.querySelector(".app")).not.toHaveClass("bar-away");
      expect(screen.getByRole("button", { name: "Stop" })).toBeInTheDocument();
    } finally {
      api.recordStatus.mockResolvedValue({ recording: null, listening: null, events: [] });
      vi.unstubAllGlobals();
    }
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
      await openPage("Personal");
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

  it("puts a scheduled run's result in the bell, not the chat, with a follow-up", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    ranHandlers.length = 0;
    render(<App />);
    await waitFor(() => expect(ranHandlers.length).toBe(1));
    expect(screen.getByRole("button", { name: "Notifications" })).toBeVisible();
    act(() => {
      ranHandlers[0]({
        id: "auto-2",
        name: "board",
        result: { text: "", error: "API key rejected", stop: "error" },
      });
    });
    const bell = screen.getByRole("button", { name: "Notifications, 1 new" });
    await userEvent.click(bell);
    expect(screen.getByRole("button", { name: "Notifications" })).toBeVisible();
    const menu = screen.getByRole("region", { name: "Notifications" });
    expect(menu).toHaveTextContent("board failed");
    await userEvent.click(within(menu).getByRole("button", { name: "Find the cause" }));
    expect(screen.queryByRole("region", { name: "Notifications" })).toBeNull();
    expect(screen.getByText(/^Chat page: The "board" automation failed: API key rejected/)).toBeVisible();
  });

  it("an auto-resume that stopped for the day lands in the bell", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    render(<App />);
    await screen.findByRole("button", { name: "Notifications" });
    const text = "Auto-resume stopped: Morning digest. Resume it from the Workboard.";
    act(() => dispatch(`data: ${JSON.stringify({ event: "workboard://auto-resume", payload: { cardId: "w-1", text } })}`));
    await userEvent.click(screen.getByRole("button", { name: "Notifications, 1 new" }));
    expect(screen.getByRole("region", { name: "Notifications" })).toHaveTextContent(text);
  });

  it("an update found in the background lands in the bell", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    updateHandlers.length = 0;
    render(<App />);
    await waitFor(() => expect(updateHandlers.length).toBe(1));
    act(() => updateHandlers[0]({ version: "1.0.0", channel: "beta", updatable: true, note: null, available: "1.0.1" }));
    await userEvent.click(screen.getByRole("button", { name: "Notifications, 1 new" }));
    expect(screen.getByRole("region", { name: "Notifications" })).toHaveTextContent(
      "Version 1.0.1 is ready to install.",
    );
  });

  it("while the window is in the background, a finished reply and a Workboard tidy land in the bell", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    api.getWorkboard.mockResolvedValueOnce({
      columns: [],
      archived: [],
      stamp: 1,
      swept: { text: "Archived 2 finished cards", ids: ["a", "b"], at: 7 },
    } as never);
    chatHandlers.length = 0;
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    render(<App />);
    await waitFor(() => expect(chatHandlers.length).toBeGreaterThan(0));
    act(() => {
      for (const h of chatHandlers) h({ conversationId: "shell-c9", event: { kind: "text", text: "Hi" } });
      for (const h of chatHandlers) h({ conversationId: "shell-c9", event: { kind: "done", stop: "end_turn" } });
    });
    focus.mockRestore();
    await userEvent.click(await screen.findByRole("button", { name: "Notifications, 2 new" }));
    const menu = screen.getByRole("region", { name: "Notifications" });
    // Each entry says what happened, to what, and where it came from.
    expect(menu).toHaveTextContent('Reply finished in "Trip plans"');
    expect(menu).toHaveTextContent("Chat: Trip plans");
    expect(menu).toHaveTextContent("Archived 2 finished cards");
    expect(menu).toHaveTextContent("Workboard");
    await userEvent.click(within(menu).getByRole("button", { name: "Undo" }));
    expect(api.workboardSweepUndo).toHaveBeenCalledTimes(1);
    // Open chat opens that very chat, not just the Chat page.
    await userEvent.click(screen.getByRole("button", { name: /^Notifications/ }));
    await userEvent.click(
      within(screen.getByRole("region", { name: "Notifications" })).getByRole("button", { name: "Open chat" }),
    );
    expect(await screen.findByText("Opened shell-c9")).toBeInTheDocument();
  });

  it("lists messages queued behind a reply in Activity, reading 1 of 3 running, with Steer now and Remove", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    chatHandlers.length = 0;
    render(<App />);
    await waitFor(() => expect(chatHandlers.length).toBeGreaterThan(0));
    act(() => {
      for (const h of chatHandlers) h({ conversationId: "shell-c1", event: { kind: "tool", title: "Reading notes.md", status: "in_progress" } });
    });
    await userEvent.click(screen.getByRole("button", { name: "Line up two" }));
    const dock = screen.getByRole("complementary", { name: "Activity" });
    const toggle = within(dock).getByRole("button", { name: "Activity, 1 of 3 running" });
    expect(toggle).toHaveTextContent("1 of 3 running");
    await userEvent.click(toggle);
    const next = within(dock).getByRole("listitem", { name: "then deploy" });
    expect(next).toHaveTextContent("Next up");
    expect(within(dock).getByRole("listitem", { name: "and tell me" })).toHaveTextContent("2nd in line");
    await userEvent.click(within(next).getByRole("button", { name: "Steer now" }));
    expect(screen.getByText("Act steer 0")).toBeVisible();
    await userEvent.click(
      within(within(dock).getByRole("listitem", { name: "and tell me" })).getByRole("button", { name: "Remove" }),
    );
    expect(screen.getByText("Act remove 1")).toBeVisible();
  });

  it("catches a render throw in a tab and recovers on Try again without unmounting the shell", async () => {
    api.getCapabilities.mockResolvedValue(DESKTOP);
    memoryThrows.current = true;
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      render(<App />);
      await openPage("Personal");
      const alert = await screen.findByRole("alert");
      expect(alert).toHaveTextContent("This page hit a snag: Corrupt vector index");
      expect(screen.getByRole("navigation", { name: "Views" })).toBeVisible();

      memoryThrows.current = false;
      await userEvent.click(within(alert).getByRole("button", { name: "Try again" }));
      expect(await screen.findByText("Memory page")).toBeVisible();
      expect(screen.queryByRole("alert")).toBeNull();
    } finally {
      memoryThrows.current = false;
      errSpy.mockRestore();
    }
  });
});
